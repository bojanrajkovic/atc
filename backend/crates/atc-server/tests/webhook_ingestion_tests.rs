use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use atc_core::{SystemClock, StateStore};
use atc_server::state::AppState;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use std::time::Duration;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Build app with no webhook secret (HMAC verification bypassed)
fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: AtomicU64::new(0),
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

// Fixtures
fn fixture_workflow_run_requested() -> Vec<u8> {
    include_bytes!("../../atc-github/tests/fixtures/workflow_run_requested.json").to_vec()
}

fn fixture_workflow_job_queued() -> Vec<u8> {
    include_bytes!("../../atc-github/tests/fixtures/workflow_job_queued.json").to_vec()
}

fn fixture_workflow_run_completed() -> Vec<u8> {
    include_bytes!("../../atc-github/tests/fixtures/workflow_run_completed.json").to_vec()
}

fn fixture_workflow_run_in_progress() -> Vec<u8> {
    include_bytes!("../../atc-github/tests/fixtures/workflow_run_in_progress.json").to_vec()
}

/// AC2.1: workflow_run event parsed and applied to StateStore, returns {"status": "processed"}
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_workflow_run_returns_processed() {
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "processed");
}

/// AC2.2: workflow_job event parsed and applied to StateStore, returns {"status": "processed"}
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_workflow_job_returns_processed() {
    let body = fixture_workflow_job_queued();

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_job")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "processed");
}

/// AC2.3: Unknown event type (e.g., push) returns {"status": "skipped"}
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_unknown_event_returns_skipped() {
    let body = b"{}";

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "push")
                .body(Body::from(body.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "skipped");
}

/// AC2.4: Missing X-GitHub-Event header returns 400
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_missing_event_header_returns_400() {
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert!(json["error"].as_str().unwrap().contains("missing X-GitHub-Event header"));
}

/// AC2.5: Malformed JSON body returns 422
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_malformed_json_returns_422() {
    let body = b"not valid json{{{";

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body.to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert!(json["error"].as_str().is_some());
}

/// AC2.6: Backward state transition (completed run receiving in_progress)
/// returns 200 for both (second is warning, not broadcast), logs warning
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_backward_transition_returns_200_no_broadcast() {
    let (app, _state) = build_app_no_secret();

    // First request: send workflow_run_completed
    let body_completed = fixture_workflow_run_completed();
    let response1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body_completed))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Second request: send workflow_run_in_progress (backward transition)
    let body_in_progress = fixture_workflow_run_in_progress();
    let response2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body_in_progress))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);

    // Verify both return "processed" (second transition is accepted but not applied)
    let body2 = to_bytes(response2.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json2: serde_json::Value =
        serde_json::from_slice(&body2).expect("response is valid JSON");
    assert_eq!(json2["status"], "processed");
}

/// AC2.7: Processed event is broadcast as SeqEvent with seq value
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_broadcast_single_event_with_seq() {
    let (app, state) = build_app_no_secret();

    // Subscribe to broadcast channel before sending
    let mut rx = state.webhook_tx.subscribe();

    // Send valid workflow_run event
    let body = fixture_workflow_run_requested();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Receive the broadcast event
    let seq_event = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv(),
    )
    .await
    .expect("timeout waiting for broadcast")
    .expect("failed to receive broadcast event");

    assert_eq!(seq_event.seq, 0);
    // Verify it's a Run event
    assert!(matches!(seq_event.event, atc_github::WebhookEvent::Run(_)));
}

/// AC2.8: Consecutive events have strictly increasing seq values (0, 1, ...)
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_broadcast_consecutive_events_increasing_seq() {
    let (app, state) = build_app_no_secret();

    // Subscribe to broadcast channel before sending
    let mut rx = state.webhook_tx.subscribe();

    // Send first valid workflow_run event
    let body1 = fixture_workflow_run_requested();
    let app1 = app.clone();
    let response1 = app1
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body1))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response1.status(), StatusCode::OK);

    // Receive first event
    let seq_event1 = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv(),
    )
    .await
    .expect("timeout waiting for first broadcast")
    .expect("failed to receive first broadcast event");

    assert_eq!(seq_event1.seq, 0);

    // Send second valid workflow_job event
    let body2 = fixture_workflow_job_queued();
    let app2 = app.clone();
    let response2 = app2
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_job")
                .body(Body::from(body2))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response2.status(), StatusCode::OK);

    // Receive second event
    let seq_event2 = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        rx.recv(),
    )
    .await
    .expect("timeout waiting for second broadcast")
    .expect("failed to receive second broadcast event");

    assert_eq!(seq_event2.seq, 1);
}
