//! End-to-end integration tests covering AC5 and AC6.3.
//!
//! These tests use an ephemeral server with real HTTP clients (reqwest) and WebSocket clients
//! (tokio-tungstenite). Each test starts a complete server with all endpoints, sends webhook POSTs,
//! and verifies state via GET /v1/state and WebSocket messages. Tests exercise the full stack
//! through real network I/O with no shortcuts.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::routes;
use atc_server::state::AppState;
use futures_util::stream::StreamExt;

/// Start an ephemeral server with full AppState, all routes, and return the server address.
///
/// The server is configured with:
/// - `Arc<StateStore>` with `SystemClock` and 1-hour TTL
/// - Broadcast channel with capacity 256
/// - `Arc<AppState>` with `webhook_secret: None` (HMAC tested separately in Phase 2)
/// - `seq: Mutex::new(0)`
/// - OnceLock `PrometheusMetricLayer` with `#[serial_test::serial]`
/// - Ephemeral `TcpListener::bind("127.0.0.1:0")`
/// - `tokio::spawn(axum::serve(...))`
///
/// Returns `SocketAddr` for HTTP/WS clients to connect to.
async fn start_test_server() -> SocketAddr {
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });

    let main_router = routes::api_routes(layer.clone())
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
// Task 2: Webhook → REST state e2e test (AC5.1)
// ============================================================================

/// AC5.1: POST webhook → GET /v1/state reflects the ingested event and returns matching seq
#[tokio::test]
#[serial_test::serial]
async fn ac5_1_webhook_to_rest_state() {
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

    // Assert seq is 1 (next seq to assign; one event committed with seq=0)
    assert_eq!(
        state["seq"], 1,
        "seq should be 1 (next to assign) after one event"
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
// Task 3: Webhook → WebSocket e2e test (AC5.2)
// ============================================================================

/// AC5.2: POST webhook → WS client receives SeqEvent with matching domain event
#[tokio::test]
#[serial_test::serial]
async fn ac5_2_webhook_to_websocket() {
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

    // Assert seq is 0 (first event)
    assert_eq!(seq_event.seq, 0, "first event should have seq=0");

    // Assert event is a Run variant
    match seq_event.event {
        atc_github::WebhookEvent::Run(_) => {
            // Expected
        }
        _ => panic!("expected Run variant in SeqEvent"),
    }
}

// ============================================================================
// Task 4: Multi-event sequence e2e test (AC5.3, AC6.3)
// ============================================================================

/// AC5.3, AC6.3: Multi-event sequence produces increasing seq values
#[tokio::test]
#[serial_test::serial]
async fn ac5_3_multi_event_sequence() {
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

    // Assert seq values are 0, 1, 2 (strictly increasing)
    assert_eq!(
        seq_values,
        vec![0, 1, 2],
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

    // Assert seq is 3 (next seq to assign; three events committed with seq 0, 1, 2)
    assert_eq!(
        state["seq"], 3,
        "seq should be 3 (next to assign) after three events"
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

    // Pool stats should be non-empty (both jobs' label sets)
    let pool_stats = &state["poolStats"];
    assert!(pool_stats.is_array(), "poolStats should be an array");
    let pool_stats_array = pool_stats.as_array().expect("poolStats is array");
    assert!(
        !pool_stats_array.is_empty(),
        "poolStats should not be empty after two jobs"
    );
}
