use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use std::sync::atomic::AtomicI64;
use tower::ServiceExt;

mod common;

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

use common::{
    build_app_no_secret, fixture_workflow_job_completed, fixture_workflow_job_queued,
    fixture_workflow_run_completed, fixture_workflow_run_in_progress,
    fixture_workflow_run_requested,
};

/// workflow_run event parsed and applied to RunStateMachine, returns {"status": "accepted", "seq": <u64>}
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_workflow_run_returns_accepted() {
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
    assert_eq!(json["status"], "accepted");
    assert!(json["seq"].is_number(), "response must include numeric seq");
}

/// workflow_job event parsed and applied to RunStateMachine, returns {"status": "accepted", "seq": <u64>}
#[tokio::test]
#[serial_test::serial]
async fn webhook_ingestion_workflow_job_returns_accepted() {
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
    assert_eq!(json["status"], "accepted");
    assert!(json["seq"].is_number(), "response must include numeric seq");
}

/// Unknown event type (e.g., push) returns {"status": "skipped"}
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

/// Missing X-GitHub-Event header returns 400
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

/// Malformed JSON body returns 422
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

/// Backward state transition (completed run receiving in_progress)
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
    assert_eq!(
        seq_event1.seq, 1,
        "pre-increment: first broadcast must have seq=1"
    );

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

    // Verify the backward transition returns "rejected" (not "accepted")
    let body2 = to_bytes(response2.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json2: serde_json::Value = serde_json::from_slice(&body2).expect("response is valid JSON");
    assert_eq!(json2["status"], "rejected");

    // Verify the rejected transition does NOT produce a SeqEvent.
    // A short timeout on recv() should return Err, confirming no broadcast occurred.
    let no_event = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(
        no_event.is_err(),
        "expected no broadcast for rejected backward transition, but received an event"
    );
}

/// Processed event is broadcast as SeqEvent with seq value
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

    assert_eq!(
        seq_event.seq, 1,
        "pre-increment: first broadcast must have seq=1"
    );
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

    // The critical invariant: seq values are strictly ordered 1, 2.
    // With pre-increment, seq=0 is the cold-start sentinel and first broadcast is seq=1.
    // With the old AtomicU64 code, concurrent requests could assign
    // seq values out of store-commit order.
    assert_eq!(
        ev1.seq, 1,
        "first broadcast event should have seq 1 (pre-increment)"
    );
    assert_eq!(
        ev2.seq, 2,
        "second broadcast event should have seq 2 (pre-increment)"
    );
    assert!(ev2.seq > ev1.seq, "seq must be strictly increasing");
}

/// Consecutive events have strictly increasing seq values (1, 2, ... — pre-increment, seq=0 is cold-start sentinel)
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

    assert_eq!(
        seq_event1.seq, 1,
        "pre-increment: first broadcast must have seq=1"
    );

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

    assert_eq!(
        seq_event2.seq, 2,
        "pre-increment: second broadcast must have seq=2"
    );
}

/// Pre-increment invariant: the first webhook after server start broadcasts seq=1, never seq=0.
///
/// The pre-increment fix eliminates the cold-start race between snapshot reads
/// and the first broadcast. With pre-increment, seq=0
/// is an unambiguous sentinel meaning "no events committed" and seq=1 is the
/// smallest valid broadcast seq.
#[tokio::test]
#[serial_test::serial]
async fn first_webhook_broadcasts_seq_1_not_seq_0() {
    let layer = common::PROMETHEUS_INIT
        .get_or_init(common::install_test_recorder)
        .0
        .clone();

    let state_machine = std::sync::Arc::new(atc_core::RunStateMachine::new(
        std::sync::Arc::new(atc_core::SystemClock),
        std::time::Duration::from_secs(3600),
    ));
    let (webhook_tx, _rx) = tokio::sync::broadcast::channel(256);
    let mut subscriber = webhook_tx.subscribe();
    let seq = std::sync::Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = std::sync::Arc::new(atc_server::persist::InMemoryStore::new(
        state_machine.clone(),
        seq.clone(),
        webhook_tx.clone(),
    )) as std::sync::Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = std::sync::Arc::new(atc_server::state::AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: None,
        min_pending_seq: std::sync::Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: std::sync::Arc::new(AtomicI64::new(now_millis_for_test())),
        broadcast_watermark: std::sync::Arc::new(AtomicI64::new(0)),
        persist,
    });

    let main_router = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, main_router).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://{}/v1/webhooks/github", addr))
        .header("X-GitHub-Event", "workflow_run")
        .body(common::fixture_workflow_run_requested())
        .send()
        .await
        .expect("POST failed");

    assert_eq!(resp.status(), 200);

    let seq_event = subscriber.recv().await.expect("should receive event");
    assert_eq!(
        seq_event.seq, 1,
        "first broadcast must have seq=1 (pre-increment: seq=0 is the cold-start sentinel)"
    );
}

/// A Job event that returns a store transition error results in no broadcast.
///
/// Drive a job to Queued via POST webhook, then attempt an invalid backward transition
/// (Queued → Completed, which is invalid for jobs: predecessors_of(Completed) = [InProgress, Completed]).
/// Assert the second POST does NOT cause a broadcast — no SeqEvent is emitted for
/// rejected transitions.
#[tokio::test]
#[serial_test::serial]
async fn failed_job_transition_produces_no_broadcast() {
    let (app, state) = build_app_no_secret();

    // POST workflow_run_requested to create the run first (run_id=24290980517)
    let resp1 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(fixture_workflow_run_requested()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    // POST workflow_job_queued to create job 70928200168 at Queued status
    let resp2 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_job")
                .body(Body::from(fixture_workflow_job_queued()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // Subscribe AFTER setup events so we have a clean baseline.
    // By this point, two SeqEvents have already been broadcast and consumed.
    let mut rx = state.webhook_tx.subscribe();

    // POST workflow_job_completed for the SAME job (still Queued in the store).
    // Queued → Completed is invalid for jobs: predecessors_of(Completed) = [InProgress, Completed].
    // The store apply_job_event will return an InvalidTransition error.
    // The handler must NOT broadcast a SeqEvent for this rejected transition.
    let resp3 = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_job")
                .body(Body::from(fixture_workflow_job_completed()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp3.status(),
        StatusCode::OK,
        "rejected transition still returns 200"
    );

    // No SeqEvent must have been broadcast for the rejected backward transition.
    assert!(
        rx.try_recv().is_err(),
        "expected no broadcast for rejected job backward transition (Queued → Completed), but received an event"
    );
}

/// In-memory mode: invalid backward transition returns 200 OK + {"status":"rejected"}.
///
/// Verifies that the unified `Arc<dyn PersistentStore>` dispatch path in the route
/// handler surfaces `PersistError::InvalidTransition` as `{"status":"rejected"}` for
/// in-memory mode, consistent with the PG mode behavior.
#[tokio::test]
#[serial_test::serial]
async fn in_memory_invalid_transition_returns_rejected() {
    let (app, state) = build_app_no_secret();

    // Subscribe to broadcasts BEFORE firing any webhooks so we observe every
    // emission. The contract: a successful Completed apply emits exactly one
    // SeqEvent; a subsequent rejected InProgress apply emits zero.
    let mut rx = state.webhook_tx.subscribe();

    // Advance to Completed via the route handler.
    let body = fixture_workflow_run_completed();
    let resp1 = app
        .clone()
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
    assert_eq!(resp1.status(), StatusCode::OK);

    // Drain the broadcast for the successful apply and snapshot seq.
    let first_event = rx
        .try_recv()
        .expect("Completed apply must broadcast exactly one SeqEvent");
    let seq_after_success = first_event.seq;
    assert!(
        rx.try_recv().is_err(),
        "successful Completed apply must broadcast exactly one event"
    );
    let seq_counter_after_success = *state.seq.lock().await;
    assert_eq!(
        seq_counter_after_success, seq_after_success,
        "AppState.seq must equal the broadcast seq after a successful apply"
    );

    // Backward transition: Completed → InProgress (parity rejection).
    let body = fixture_workflow_run_in_progress();
    let resp2 = app
        .clone()
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
    assert_eq!(
        resp2.status(),
        StatusCode::OK,
        "invalid transition must return 200, not 4xx"
    );

    let body_bytes = to_bytes(resp2.into_body(), usize::MAX)
        .await
        .expect("failed to read body");
    let json: serde_json::Value =
        serde_json::from_slice(&body_bytes).expect("response must be JSON");
    assert_eq!(
        json["status"], "rejected",
        "in-memory invalid transition must return {{\"status\":\"rejected\"}}, got: {json}"
    );

    // Side-effect contract: rejection must not broadcast and must not
    // bump the seq counter.
    assert!(
        rx.try_recv().is_err(),
        "rejected InProgress must not broadcast a SeqEvent"
    );
    let seq_counter_after_reject = *state.seq.lock().await;
    assert_eq!(
        seq_counter_after_reject, seq_counter_after_success,
        "rejected InProgress must not advance AppState.seq"
    );
}
