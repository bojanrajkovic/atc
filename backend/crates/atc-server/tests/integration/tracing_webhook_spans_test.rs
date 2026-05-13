//! Span-side instrumentation tests.
//!
//! Asserts that the webhook handler, persistence layer, and drain pipeline
//! emit the boundary spans declared in the architecture-doc table, and that
//! W3C `traceparent` headers propagate into the root request span. Spans flow
//! into an in-memory `InMemorySpanExporter` installed by the
//! `ensure_span_exporter_installed()` helper in `common/mod.rs`.

use std::time::Duration;

use axum::http::StatusCode;
use opentelemetry_sdk::trace::SpanData;
use serial_test::serial;
use tokio::time::timeout;

use crate::common;

const TRACEPARENT_TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";
const TRACEPARENT_HEADER: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

fn span_named<'a>(spans: &'a [SpanData], name: &str) -> Option<&'a SpanData> {
    spans.iter().find(|s| s.name.as_ref() == name)
}

fn spans_named<'a>(spans: &'a [SpanData], name: &str) -> Vec<&'a SpanData> {
    spans.iter().filter(|s| s.name.as_ref() == name).collect()
}

fn parent_of<'a>(spans: &'a [SpanData], child: &SpanData) -> Option<&'a SpanData> {
    if !child.parent_span_id.to_bytes().iter().any(|b| *b != 0) {
        return None;
    }
    spans
        .iter()
        .find(|s| s.span_context.span_id() == child.parent_span_id)
}

fn attribute_str(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

#[tokio::test]
#[serial]
async fn webhook_post_emits_expected_span_hierarchy() {
    common::ensure_span_exporter_installed();
    common::reset_spans();

    let (app, _state) = common::build_app_no_secret();

    let (status, body) = common::post_webhook_to_router(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "accepted");

    let spans = common::read_finished_spans();

    let handler =
        span_named(&spans, "webhook.handler").expect("webhook.handler span must be exported");
    assert!(
        attribute_str(handler, "webhook.event_type").as_deref() == Some("workflow_run"),
        "webhook.handler must record webhook.event_type=workflow_run; got attributes {:?}",
        handler.attributes,
    );

    let verify = span_named(&spans, "webhook.parse").expect("webhook.parse span must be exported");
    assert!(
        parent_of(&spans, verify)
            .map(|p| p.name.as_ref() == "webhook.handler")
            .unwrap_or(false),
        "webhook.parse must be a child of webhook.handler"
    );

    let apply = span_named(&spans, "persist.apply.run_event")
        .expect("persist.apply.run_event span must be exported");
    assert!(
        parent_of(&spans, apply)
            .map(|p| p.name.as_ref() == "webhook.handler")
            .unwrap_or(false),
        "persist.apply.run_event must be a child of webhook.handler",
    );
}

#[tokio::test]
#[serial]
async fn traceparent_header_propagates_to_root_span() {
    common::ensure_span_exporter_installed();
    common::reset_spans();

    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    let (app, _state) = common::build_app_no_secret();

    let req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "workflow_run")
        .header("traceparent", TRACEPARENT_HEADER)
        .body(Body::from(common::fixture_workflow_run_requested()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let spans = common::read_finished_spans();
    let handler =
        span_named(&spans, "webhook.handler").expect("webhook.handler span must be exported");

    let actual = format!(
        "{:032x}",
        u128::from_be_bytes(handler.span_context.trace_id().to_bytes())
    );
    assert_eq!(
        actual, TRACEPARENT_TRACE_ID,
        "webhook.handler trace ID must match the incoming traceparent",
    );
}

#[tokio::test]
#[serial]
async fn drain_pass_span_is_child_of_drain_task() {
    common::ensure_span_exporter_installed();
    common::reset_spans();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Fire a webhook so the drain has work to do — pass and broadcast spans
    // are only emitted when an outbox row exists.
    let (status, _body) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Wait for the drain to broadcast at least one event.
    let mut rx = fixture.state.persist.subscribe();
    let _ = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for drain broadcast");

    // Poll up to ~500 ms for the SimpleSpanProcessor to flush the ended
    // drain.pass child. `drain.task` itself only ends when the spawned task
    // exits (after `fixture.shutdown.cancel()` below + record_shutdown_remaining),
    // so we assert the parent linkage via parent_span_id (set at child
    // creation time) rather than by lookup against the not-yet-exported
    // parent. Polling absorbs scheduler jitter under heavy concurrent test
    // load (testcontainers PG shared, axum-otel pipeline shared) without
    // committing to a fixed-budget sleep that can underprovision in CI.
    let spans = {
        let mut spans = Vec::new();
        for _ in 0..50 {
            spans = common::read_finished_spans();
            if span_named(&spans, "drain.pass").is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        spans
    };

    let pass = span_named(&spans, "drain.pass")
        .expect("drain.pass span must be exported after a drain pass runs");
    assert!(
        pass.parent_span_id.to_bytes().iter().any(|b| *b != 0),
        "drain.pass must carry a non-zero parent_span_id (the spawn-site \
         drain.task span). A zero parent_span_id means tokio::spawn detached \
         the future from its parent — the Instrument-trait gotcha."
    );

    let broadcasts = spans_named(&spans, "drain.broadcast");
    assert!(
        !broadcasts.is_empty(),
        "at least one drain.broadcast span must be exported after a webhook is drained"
    );
    for b in &broadcasts {
        let parent = parent_of(&spans, b);
        assert!(
            parent
                .map(|p| p.name.as_ref() == "drain.pass")
                .unwrap_or(false),
            "drain.broadcast must be a child of drain.pass; parent={:?}",
            parent.map(|p| p.name.as_ref()),
        );
        assert_eq!(
            b.span_context.trace_id(),
            pass.span_context.trace_id(),
            "drain.broadcast must share its trace id with drain.pass",
        );
    }

    // Cancel the shutdown token so drain.task ends and exports. After cancel,
    // the in-memory exporter holds the parent span too — assert by name lookup.
    fixture.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), fixture.state.persist.shutdown()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    let spans_after = common::read_finished_spans();
    let task = span_named(&spans_after, "drain.task")
        .expect("drain.task span must be exported after the task exits");
    assert_eq!(
        task.span_context.trace_id(),
        pass.span_context.trace_id(),
        "drain.task and drain.pass must share a trace id",
    );
    assert_eq!(
        task.span_context.span_id(),
        pass.parent_span_id,
        "drain.pass parent_span_id must match drain.task span_id",
    );
}
