//! Span-side instrumentation tests.
//!
//! Asserts that the webhook handler, persistence layer, and drain pipeline
//! emit the boundary spans declared in the architecture-doc table, and that
//! W3C `traceparent` headers propagate into the root request span. Spans flow
//! into an in-memory `InMemorySpanExporter` installed by the
//! `ensure_span_exporter_installed()` helper in `common/mod.rs`.

use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

use crate::common;
use crate::common::{attribute_str, parent_of, span_named, spans_named};

const TRACEPARENT_TRACE_ID: &str = "0af7651916cd43dd8448eb211c80319c";
const TRACEPARENT_HEADER: &str = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";

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
async fn drain_pass_is_root_with_drain_broadcast_child() {
    common::ensure_span_exporter_installed();
    common::reset_spans();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Subscribe BEFORE firing the webhook. `tokio::sync::broadcast::Receiver`
    // only observes messages sent after subscription, so a `subscribe()` call
    // placed after `post_webhook_to_router` can race the drain task and miss
    // the only broadcast the test is waiting on (5s timeout trips, test fails).
    let mut rx = fixture.state.persist.subscribe();

    let (status, _body) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Wait for the drain to broadcast at least one event.
    let _ = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for drain broadcast");

    // Poll up to ~500 ms for the SimpleSpanProcessor to flush the ended
    // drain.pass root. Polling absorbs scheduler jitter under heavy concurrent
    // test load (testcontainers PG shared, axum-otel pipeline shared) without
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

    let pass_spans = spans_named(&spans, "drain.pass");
    assert!(
        !pass_spans.is_empty(),
        "drain.pass span must be exported after a drain pass runs"
    );
    for p in &pass_spans {
        assert!(
            p.parent_span_id.to_bytes().iter().all(|b| *b == 0),
            "every drain.pass must be a root span (zero parent_span_id). A \
             non-zero parent here means a task-lifetime wrapper was \
             reintroduced — see the per-tick-root rationale in \
             `eviction_spans_test.rs`."
        );
    }

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
    }

    fixture.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), fixture.state.persist.shutdown()).await;
}

#[tokio::test]
#[serial]
async fn listener_recv_is_root() {
    common::ensure_span_exporter_installed();
    common::reset_spans();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Subscribe BEFORE firing the webhook. `tokio::sync::broadcast::Receiver`
    // only observes messages sent after subscription, so a `subscribe()` call
    // placed after `post_webhook_to_router` can race the drain task and miss
    // the only broadcast the test is waiting on (5s timeout trips, test fails).
    let mut rx = fixture.state.persist.subscribe();

    // Fire a webhook so the listener receives a NOTIFY for the outbox row.
    let (status, _body) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Wait for the drain to broadcast — proves the listener picked the NOTIFY
    // up before we poll the exporter.
    let _ = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for drain broadcast");

    let spans = {
        let mut spans = Vec::new();
        for _ in 0..50 {
            spans = common::read_finished_spans();
            if span_named(&spans, "listener.recv").is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        spans
    };

    let recv = span_named(&spans, "listener.recv")
        .expect("listener.recv span must be exported after a NOTIFY is received");
    assert!(
        recv.parent_span_id.to_bytes().iter().all(|b| *b == 0),
        "listener.recv must be a root span (zero parent_span_id). A non-zero \
         parent here means a task-lifetime wrapper was reintroduced."
    );

    fixture.shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), fixture.state.persist.shutdown()).await;
}
