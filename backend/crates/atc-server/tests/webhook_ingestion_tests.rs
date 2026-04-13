use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

mod common;

use common::{
    build_app_no_secret, fixture_workflow_job_queued, fixture_workflow_run_completed,
    fixture_workflow_run_in_progress, fixture_workflow_run_requested,
};

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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert!(
        json["error"]
            .as_str()
            .unwrap()
            .contains("missing X-GitHub-Event header")
    );
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
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert!(json["error"].as_str().is_some());
}

/// AC2.6: Backward state transition (completed run receiving in_progress)
/// returns 200 for both (second is warning, not broadcast), logs warning
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_backward_transition_returns_200_no_broadcast() {
    let (app, state) = build_app_no_secret();

    // Subscribe to broadcast channel before sending any requests
    let mut rx = state.webhook_tx.subscribe();

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

    // Receive the first broadcast event (successful transition)
    let seq_event1 = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for first broadcast")
        .expect("failed to receive first broadcast event");
    assert_eq!(seq_event1.seq, 0);

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

    // Verify both return "processed"
    let body2 = to_bytes(response2.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json2: serde_json::Value = serde_json::from_slice(&body2).expect("response is valid JSON");
    assert_eq!(json2["status"], "processed");

    // Verify the rejected transition does NOT produce a SeqEvent.
    // A short timeout on recv() should return Err, confirming no broadcast occurred.
    let no_event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        no_event.is_err(),
        "expected no broadcast for rejected backward transition, but received an event"
    );
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
    let seq_event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for broadcast")
        .expect("failed to receive broadcast event");

    assert_eq!(seq_event.seq, 0);
    // Verify it's a Run event
    assert!(matches!(seq_event.event, atc_github::WebhookEvent::Run(_)));
}

/// Concurrent webhooks assign seq values that match store commit order.
///
/// This test would have caught the bug where AtomicU64::fetch_add was
/// called outside the store lock — two concurrent webhooks could
/// interleave between store mutation and seq assignment, producing WS
/// events whose seq values didn't match the committed state order.
///
/// The fix holds a Mutex across store mutation + seq assignment,
/// serializing the critical section so seq order matches commit order.
#[tokio::test]
#[serial_test::serial]
async fn webhook_concurrent_requests_produce_ordered_seq() {
    let (app, state) = build_app_no_secret();

    let mut rx = state.webhook_tx.subscribe();

    // Fire two webhooks concurrently via spawned tasks.
    let app1 = app.clone();
    let body1 = fixture_workflow_run_requested();
    let handle1 = tokio::spawn(async move {
        app1.oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body1))
                .unwrap(),
        )
        .await
        .unwrap()
    });

    let app2 = app.clone();
    let body2 = fixture_workflow_job_queued();
    let handle2 = tokio::spawn(async move {
        app2.oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_job")
                .body(Body::from(body2))
                .unwrap(),
        )
        .await
        .unwrap()
    });

    let (r1, r2) = tokio::join!(handle1, handle2);
    assert_eq!(r1.unwrap().status(), StatusCode::OK);
    assert_eq!(r2.unwrap().status(), StatusCode::OK);

    // Collect both broadcast events.
    let ev1 = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for first broadcast")
        .expect("recv failed");
    let ev2 = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for second broadcast")
        .expect("recv failed");

    // The critical invariant: seq values are strictly ordered 0, 1.
    // With the old AtomicU64 code, concurrent requests could assign
    // seq values out of store-commit order.
    assert_eq!(ev1.seq, 0, "first broadcast event should have seq 0");
    assert_eq!(ev2.seq, 1, "second broadcast event should have seq 1");
    assert!(ev2.seq > ev1.seq, "seq must be strictly increasing");
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
    let seq_event1 = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
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
    let seq_event2 = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
        .await
        .expect("timeout waiting for second broadcast")
        .expect("failed to receive second broadcast event");

    assert_eq!(seq_event2.seq, 1);
}
