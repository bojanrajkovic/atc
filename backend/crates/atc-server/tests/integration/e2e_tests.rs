//! End-to-end integration tests.
//!
//! These tests use an ephemeral server with real HTTP clients (reqwest) and WebSocket clients
//! (tokio-tungstenite). Each test starts a complete server with all endpoints, sends webhook POSTs,
//! and verifies state via GET /v1/state and WebSocket messages. Tests exercise the full stack
//! through real network I/O with no shortcuts.

use crate::common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;

use atc_core::{RunStateMachine, SystemClock};
use atc_server::routes;
use atc_server::state::AppState;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
use futures_util::stream::StreamExt;

/// Start an ephemeral server with full AppState, all routes, and return the server address.
///
/// The server is configured with:
/// - `Arc<RunStateMachine>` with `SystemClock` and 1-hour TTL
/// - Broadcast channel with capacity 256
/// - `Arc<AppState>` with `webhook_secret: None` (HMAC tested separately)
/// - `seq: Arc::new(Mutex::new(0))` (shared with `InMemoryStore`)
/// - `persist: Arc<dyn PersistentStore>` (`InMemoryStore` for in-memory mode)
/// - OTel test harness via `common::ensure_recorder_installed`, with `#[serial_test::serial]`
/// - Ephemeral `TcpListener::bind("127.0.0.1:0")`
/// - `tokio::spawn(axum::serve(...))`
///
/// Returns `SocketAddr` for HTTP/WS clients to connect to.
async fn start_test_server() -> SocketAddr {
    common::ensure_recorder_installed();

    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = Arc::new(atc_server::persist::InMemoryStore::new(
        state_machine.clone(),
        seq.clone(),
        webhook_tx.clone(),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: None,
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(now_millis_for_test())),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });

    let main_router = routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    main_addr
}

// ============================================================================
// Webhook → REST state e2e test
// ============================================================================

/// POST webhook → GET /v1/state reflects the ingested event and returns matching seq
#[tokio::test]
#[serial_test::serial]
async fn webhook_to_rest_state() {
    let addr = start_test_server().await;

    let client = reqwest::Client::new();
    let webhook_body = common::fixture_workflow_run_requested();

    // POST webhook
    let webhook_url = format!("http://{}/v1/webhooks/github", addr);
    let response = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(webhook_body)
        .send()
        .await
        .expect("webhook POST should succeed");

    assert_eq!(response.status(), 200, "webhook should return 200 OK");

    // GET /v1/state
    let state_url = format!("http://{}/v1/state", addr);
    let state_response = client
        .get(&state_url)
        .send()
        .await
        .expect("GET /v1/state should succeed");

    assert_eq!(
        state_response.status(),
        200,
        "GET /v1/state should return 200"
    );

    let state_json = state_response
        .text()
        .await
        .expect("should read response body");
    let state: serde_json::Value =
        serde_json::from_str(&state_json).expect("should parse state JSON");

    // Assert lastSeq is 1 (highest committed; one event committed with seq=1)
    assert_eq!(
        state["lastSeq"], 1,
        "lastSeq should be 1 (highest committed) after one event"
    );

    // Assert runs array has 1 entry with run_id matching the fixture
    let runs = &state["runs"];
    assert!(runs.is_array(), "runs should be an array");
    let runs_array = runs.as_array().expect("runs is array");
    assert_eq!(
        runs_array.len(),
        1,
        "runs should have exactly 1 entry after one workflow_run event"
    );

    let run = &runs_array[0];
    let run_id = run["id"].as_u64().expect("run id should be a u64");
    assert_eq!(
        run_id, 24290980517,
        "run_id should match fixture (24290980517)"
    );

    // Assert jobs is empty (only a run event was sent)
    let jobs = &state["jobs"];
    assert!(jobs.is_array(), "jobs should be an array");
    let jobs_array = jobs.as_array().expect("jobs is array");
    assert_eq!(
        jobs_array.len(),
        0,
        "jobs should be empty after only workflow_run event"
    );
}

// ============================================================================
// Webhook → WebSocket e2e test
// ============================================================================

/// POST webhook → WS client receives SeqEvent with matching domain event
#[tokio::test]
#[serial_test::serial]
async fn webhook_to_websocket() {
    let addr = start_test_server().await;

    let client = reqwest::Client::new();
    let webhook_body = common::fixture_workflow_run_requested();

    // Connect WS client
    let ws_url = format!("ws://{}/v1/ws", addr);
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection should succeed");

    let (_ws_write, mut ws_read) = futures_util::stream::StreamExt::split(ws_stream);

    // POST webhook
    let webhook_url = format!("http://{}/v1/webhooks/github", addr);
    let _response = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(webhook_body)
        .send()
        .await
        .expect("webhook POST should succeed");

    // WS client receives a text frame with timeout
    let frame = tokio::time::timeout(Duration::from_secs(5), ws_read.next())
        .await
        .expect("should receive WebSocket message within 5 seconds")
        .expect("WebSocket stream should yield message")
        .expect("WebSocket message should be ok");

    let text = match frame {
        tokio_tungstenite::tungstenite::Message::Text(t) => t,
        _ => panic!("expected text frame from WebSocket"),
    };

    // Deserialize as SeqEvent
    let seq_event: atc_server::state::SeqEvent =
        serde_json::from_str(&text).expect("should deserialize SeqEvent");

    // Assert seq is 1 (first event)
    assert_eq!(seq_event.seq, 1, "first event should have seq=1");

    // Assert event is a Run variant
    match seq_event.event {
        atc_github::WebhookEvent::Run(_) => {
            // Expected
        }
        _ => panic!("expected Run variant in SeqEvent"),
    }
}

// ============================================================================
// Multi-event sequence e2e test
// ============================================================================

/// Multi-event sequence produces increasing seq values
#[tokio::test]
#[serial_test::serial]
async fn multi_event_sequence() {
    let addr = start_test_server().await;

    let client = reqwest::Client::new();

    // Connect WS client
    let ws_url = format!("ws://{}/v1/ws", addr);
    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection should succeed");

    let (_ws_write, mut ws_read) = futures_util::stream::StreamExt::split(ws_stream);

    // Post first webhook: workflow_run_requested
    let run_body = common::fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", addr);

    client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(run_body)
        .send()
        .await
        .expect("first webhook POST should succeed");

    // Post second webhook: workflow_job_queued
    let job_queued_body = common::fixture_workflow_job_queued();
    client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(job_queued_body)
        .send()
        .await
        .expect("second webhook POST should succeed");

    // Post third webhook: workflow_job_in_progress
    let job_in_progress_body = common::fixture_workflow_job_in_progress();
    client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(job_in_progress_body)
        .send()
        .await
        .expect("third webhook POST should succeed");

    // Allow a brief moment for events to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Collect 3 SeqEvents from WS
    let mut seq_values = Vec::new();
    for _ in 0..3 {
        let frame = tokio::time::timeout(Duration::from_secs(5), ws_read.next())
            .await
            .expect("should receive WebSocket message within 5 seconds")
            .expect("WebSocket stream should yield message")
            .expect("WebSocket message should be ok");

        let text = match frame {
            tokio_tungstenite::tungstenite::Message::Text(t) => t,
            _ => panic!("expected text frame from WebSocket"),
        };

        let seq_event: atc_server::state::SeqEvent =
            serde_json::from_str(&text).expect("should deserialize SeqEvent");
        seq_values.push(seq_event.seq);
    }

    // Assert seq values are 1, 2, 3 (strictly increasing)
    assert_eq!(
        seq_values,
        vec![1, 2, 3],
        "WS events should have strictly increasing seq values"
    );

    // GET /v1/state
    let state_url = format!("http://{}/v1/state", addr);
    let state_response = client
        .get(&state_url)
        .send()
        .await
        .expect("GET /v1/state should succeed");

    let state_json = state_response
        .text()
        .await
        .expect("should read response body");
    let state: serde_json::Value =
        serde_json::from_str(&state_json).expect("should parse state JSON");

    // Assert lastSeq is 3 (highest committed; three events committed with seq 1, 2, 3)
    assert_eq!(
        state["lastSeq"], 3,
        "lastSeq should be 3 (highest committed) after three events"
    );

    // Assert runs has 1 entry
    let runs = &state["runs"];
    let runs_array = runs.as_array().expect("runs is array");
    assert_eq!(
        runs_array.len(),
        1,
        "runs should have 1 entry (the single workflow run)"
    );

    // Assert jobs has 2 entries (two different jobs from the same run)
    let jobs = &state["jobs"];
    let jobs_array = jobs.as_array().expect("jobs is array");
    assert_eq!(
        jobs_array.len(),
        2,
        "jobs should have 2 entries (two different jobs from same run)"
    );

    // Verify the two jobs have different job_ids
    let job_id_1 = jobs_array[0]["id"].as_u64().expect("job id is u64");
    let job_id_2 = jobs_array[1]["id"].as_u64().expect("job id is u64");
    assert_ne!(
        job_id_1, job_id_2,
        "jobs should have different job_ids (70928200168 and 70928200174)"
    );
}
