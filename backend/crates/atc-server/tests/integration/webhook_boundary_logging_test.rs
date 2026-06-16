//! Boundary-event logging tests for the webhook handler.
//!
//! Asserts that ping, skipped (unhandled type), accepted, and parse-failure
//! outcomes each emit a log line at the expected level carrying the correlation
//! fields operators rely on (issue #338): `delivery_id` on every boundary line,
//! `run_id` on the accepted line. Log *events* (not spans) are captured by a
//! thread-local subscriber installed via `set_default` — each `#[tokio::test]`
//! runs on a current-thread runtime, so the handler future polls on this thread
//! and only its events land in the buffer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

use crate::common;

#[derive(Debug, Clone)]
struct CapturedEvent {
    level: Level,
    message: String,
    fields: BTreeMap<String, String>,
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: BTreeMap<String, String>,
}

impl FieldVisitor {
    fn put(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = value;
        } else {
            self.fields.insert(field.name().to_string(), value);
        }
    }
}

impl Visit for FieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value.to_string());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, value.to_string());
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, value.to_string());
    }
}

#[derive(Clone, Default)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        self.events.lock().unwrap().push(CapturedEvent {
            level: *event.metadata().level(),
            message: visitor.message,
            fields: visitor.fields,
        });
    }
}

fn find<'a>(events: &'a [CapturedEvent], message: &str) -> &'a CapturedEvent {
    events
        .iter()
        .find(|e| e.message == message)
        .unwrap_or_else(|| panic!("expected a log line {message:?}; captured: {events:?}"))
}

/// Fire one webhook request against a fresh in-memory app while capturing log
/// events on this thread, returning the captured events.
async fn capture_webhook_logs(
    event_type: &str,
    delivery_id: &str,
    body: Vec<u8>,
) -> Vec<CapturedEvent> {
    let capture = CaptureLayer::default();
    let events = capture.events.clone();
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));

    let (app, _state) = common::build_app_no_secret();
    let request = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header("x-github-event", event_type)
        .header("x-github-delivery", delivery_id)
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    drop(guard);

    // Surface the HTTP status through a synthetic event so callers assert it
    // alongside the captured log lines.
    let mut events = events.lock().unwrap().clone();
    events.push(CapturedEvent {
        level: Level::TRACE,
        message: "__status__".to_string(),
        fields: BTreeMap::from([("code".to_string(), status.as_u16().to_string())]),
    });
    events
}

fn status_of(events: &[CapturedEvent]) -> u16 {
    find(events, "__status__").fields["code"].parse().unwrap()
}

#[tokio::test]
#[serial_test::serial]
async fn ping_logs_at_info_with_delivery_id() {
    let events = capture_webhook_logs(
        "ping",
        "d-ping-1",
        br#"{"zen": "Keep it simple.", "hook_id": 42}"#.to_vec(),
    )
    .await;

    assert_eq!(status_of(&events), 200, "ping returns 200");

    let line = find(&events, "ping received");
    assert_eq!(line.level, Level::INFO, "ping must log at INFO");
    assert_eq!(
        line.fields.get("event_type").map(String::as_str),
        Some("ping")
    );
    assert!(
        line.fields
            .get("delivery_id")
            .is_some_and(|v| v.contains("d-ping-1")),
        "ping line must carry delivery_id; got {:?}",
        line.fields
    );
}

#[tokio::test]
#[serial_test::serial]
async fn skipped_event_logs_at_info_with_delivery_id() {
    let events = capture_webhook_logs("push", "d-push-1", b"{}".to_vec()).await;

    assert_eq!(status_of(&events), 200, "skipped returns 200");

    let line = find(&events, "event skipped");
    assert_eq!(line.level, Level::INFO, "skipped events must log at INFO");
    assert_eq!(
        line.fields.get("event_type").map(String::as_str),
        Some("push")
    );
    assert!(
        line.fields
            .get("delivery_id")
            .is_some_and(|v| v.contains("d-push-1")),
        "skipped line must carry delivery_id; got {:?}",
        line.fields
    );
}

#[tokio::test]
#[serial_test::serial]
async fn accepted_run_logs_with_run_id() {
    let events = capture_webhook_logs(
        "workflow_run",
        "d-run-1",
        common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status_of(&events), 200, "accepted returns 200");

    let line = find(&events, "event accepted");
    assert_eq!(line.level, Level::INFO, "accepted must log at INFO");
    assert!(
        line.fields.contains_key("seq"),
        "accepted line must carry seq"
    );
    assert!(
        line.fields.contains_key("run_id"),
        "accepted line must carry run_id; got {:?}",
        line.fields
    );
}

#[tokio::test]
#[serial_test::serial]
async fn parse_failure_logs_at_error_with_delivery_id() {
    let events =
        capture_webhook_logs("workflow_run", "d-bad-1", b"not valid json{{{".to_vec()).await;

    assert_eq!(status_of(&events), 422, "parse failure returns 422");

    let line = find(&events, "webhook parse error");
    assert_eq!(line.level, Level::ERROR, "parse failure must log at ERROR");
    assert_eq!(
        line.fields.get("event_type").map(String::as_str),
        Some("workflow_run")
    );
    assert!(
        line.fields
            .get("delivery_id")
            .is_some_and(|v| v.contains("d-bad-1")),
        "parse-error line must carry delivery_id; got {:?}",
        line.fields
    );
}
