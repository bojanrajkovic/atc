use std::sync::Arc;

use crate::common;
use common::{fixture_workflow_job_queued, fixture_workflow_run_requested};

/// GET /v1/state with no prior events returns seq: 0, empty collections
#[tokio::test]
#[serial_test::serial]
async fn test_empty_state() {
    let (server_addr, _) = common::spawn_in_memory_server().await;

    let client = reqwest::Client::new();
    let state_url = format!("http://{}/v1/state", server_addr);

    let resp = client
        .get(&state_url)
        .send()
        .await
        .expect("GET /v1/state failed");

    assert_eq!(resp.status(), 200, "should return 200 OK");

    let json: serde_json::Value = resp.json().await.expect("response is valid JSON");

    assert_eq!(
        json["lastSeq"], 0,
        "lastSeq should be 0 when no events ingested"
    );
    assert_eq!(json["runs"], serde_json::json!([]), "runs should be empty");
    assert_eq!(json["jobs"], serde_json::json!([]), "jobs should be empty");
    // Default `AppState` carries no operator-declared pools — surface as
    // an empty array so the frontend's merge step is a no-op.
    assert_eq!(
        json["runnerPoolCapacities"],
        serde_json::json!([]),
        "runnerPoolCapacities should be empty when no operator config is loaded"
    );
}

/// Snapshot composition: with operator-declared capacities in `AppState`,
/// `GET /v1/state` returns them in the canonical sorted-labels form so the
/// frontend can merge directly against `RunnerPoolStats.labels`.
#[tokio::test]
#[serial_test::serial]
async fn snapshot_carries_runner_pool_capacities_from_app_state() {
    use atc_core::{LabelSet, RunnerPoolCapacity};
    use atc_server::state::AppState;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use tokio_util::task::TaskTracker;

    common::ensure_recorder_installed();

    let clock: Arc<dyn atc_core::Clock> = Arc::new(atc_core::SystemClock);
    let persist = atc_store_mem::InMemoryStore::new_for_test(
        Arc::clone(&clock),
        Duration::from_secs(3600),
        256,
    ) as Arc<dyn atc_persist::PersistentStore>;

    // Capacities are populated post-validation in `main.rs`; here we hand-build
    // the same shape to exercise the route-layer composition without going
    // through file IO. The unbounded entry (`capacity: None`) exercises the
    // `capacity: null` rail end-to-end through serialization.
    let app_state = Arc::new(AppState {
        persist,
        clock,
        display_ttl: Duration::from_secs(3600),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(vec![
            RunnerPoolCapacity {
                labels: LabelSet::new(["self-hosted", "linux", "x64"]),
                capacity: Some(10),
            },
            RunnerPoolCapacity {
                labels: LabelSet::new(["ubuntu-latest"]),
                capacity: None,
            },
        ]),
        config_events_tx: tokio::sync::broadcast::channel(16).0,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
    });

    let app = atc_server::routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/v1/state"))
        .send()
        .await
        .expect("GET /v1/state failed");
    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.unwrap();
    let caps = json["runnerPoolCapacities"]
        .as_array()
        .expect("runnerPoolCapacities should be an array");
    assert_eq!(caps.len(), 2, "expected two declared pools, got {caps:?}");

    // First pool: labels canonicalize to sorted order on the wire so the
    // frontend can match against any insertion order.
    assert_eq!(
        caps[0]["labels"],
        serde_json::json!(["linux", "self-hosted", "x64"])
    );
    assert_eq!(caps[0]["capacity"], 10);
    assert_eq!(caps[1]["labels"], serde_json::json!(["ubuntu-latest"]));
    assert_eq!(
        caps[1]["capacity"],
        serde_json::Value::Null,
        "explicit-null capacity for an unbounded pool must round-trip on the wire"
    );
}

/// Writes through `AppState.runner_pool_capacities` are visible to the next
/// `/v1/state` snapshot. This is the contract the hot-reload watcher depends
/// on: it takes `write().await` and replaces the inner Vec, and the next
/// `state_handler` invocation reads the new value under a short `read().await`
/// guard.
#[tokio::test]
#[serial_test::serial]
async fn mutating_app_state_capacities_reflects_in_next_snapshot() {
    use atc_core::{LabelSet, RunnerPoolCapacity};
    use atc_server::state::AppState;
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;
    use tokio_util::task::TaskTracker;

    common::ensure_recorder_installed();

    let clock: Arc<dyn atc_core::Clock> = Arc::new(atc_core::SystemClock);
    let persist = atc_store_mem::InMemoryStore::new_for_test(
        Arc::clone(&clock),
        Duration::from_secs(3600),
        256,
    ) as Arc<dyn atc_persist::PersistentStore>;

    let app_state = Arc::new(AppState {
        persist,
        clock,
        display_ttl: Duration::from_secs(3600),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: tokio::sync::broadcast::channel(16).0,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
    });

    let app = atc_server::routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Initial snapshot: no capacities declared.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/v1/state"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["runnerPoolCapacities"], serde_json::json!([]));

    // Take a write guard and replace the inner Vec — the same path the
    // watcher uses.
    {
        let mut guard = app_state.runner_pool_capacities.write().await;
        *guard = vec![RunnerPoolCapacity {
            labels: LabelSet::new(["self-hosted", "linux", "x64"]),
            capacity: Some(42),
        }];
    }

    // Next snapshot reflects the write.
    let resp = client
        .get(format!("http://{addr}/v1/state"))
        .send()
        .await
        .unwrap();
    let json: serde_json::Value = resp.json().await.unwrap();
    let caps = json["runnerPoolCapacities"].as_array().unwrap();
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0]["capacity"], 42);
    assert_eq!(
        caps[0]["labels"],
        serde_json::json!(["linux", "self-hosted", "x64"])
    );
}

/// Rolling-deploy tolerance: a snapshot serialized without
/// `runnerPoolCapacities` (e.g., from an older replica's struct shape) still
/// deserializes to an empty vec via `#[serde(default)]`.
#[test]
fn state_snapshot_deserializes_without_runner_pool_capacities_field() {
    let payload = serde_json::json!({
        "lastSeq": 7,
        "runs": [],
        "jobs": [],
    });
    let snap: atc_wire::StateSnapshot =
        serde_json::from_value(payload).expect("missing field should default to empty vec");
    assert_eq!(snap.last_seq, 7);
    assert!(snap.runner_pool_capacities.is_empty());
}

/// GET /v1/state after workflow_run_requested webhook returns seq: 1, run in runs
#[tokio::test]
#[serial_test::serial]
async fn test_state_after_run_event() {
    let (server_addr, _) = common::spawn_in_memory_server().await;

    let client = reqwest::Client::new();

    // POST the webhook
    let body = fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200, "webhook should be accepted");

    // Now GET /v1/state
    let state_url = format!("http://{}/v1/state", server_addr);
    let resp = client
        .get(&state_url)
        .send()
        .await
        .expect("GET /v1/state failed");

    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.expect("response is valid JSON");

    assert_eq!(json["lastSeq"], 1, "lastSeq should be 1 after one event");
    assert!(
        !json["runs"].as_array().unwrap().is_empty(),
        "runs should contain the workflow run"
    );
    assert_eq!(
        json["jobs"],
        serde_json::json!([]),
        "jobs should still be empty"
    );
}

/// Snapshot lastSeq is always consistent with snapshot content under concurrent writes.
///
/// This test would have caught the bug where state_handler read the store
/// snapshot and the seq counter separately — a webhook completing between
/// the two reads could produce lastSeq: N+1 with a snapshot missing event N.
///
/// The fix holds the seq Mutex across both reads so no webhook can commit
/// between the snapshot and the cursor read.
///
/// Design: each event creates a DISTINCT entity (1 run + 2 jobs with
/// different job_ids), so entity count correlates 1:1 with lastSeq. For each
/// event we fire the webhook and GET /v1/state concurrently. The snapshot
/// must be in one of two consistent states:
///   - lastSeq=K (before this event): entity count = K
///   - lastSeq=K+1 (after this event): entity count = K+1
/// The old bug would produce lastSeq=K+1 with entity count still at K.
#[tokio::test]
#[serial_test::serial]
async fn test_snapshot_seq_consistent_under_concurrent_writes() {
    use common::fixture_workflow_job_in_progress;

    let (server_addr, _) = common::spawn_in_memory_server().await;

    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);
    let state_url = format!("http://{}/v1/state", server_addr);

    // Three events that each create a distinct entity:
    //   0: workflow_run_requested  → creates run  24290980517
    //   1: workflow_job_queued     → creates job  70928200168
    //   2: workflow_job_in_progress → creates job  70928200174
    // After event K completes: runs.len() + jobs.len() == K+1.
    let events: Vec<(&str, Vec<u8>)> = vec![
        ("workflow_run", fixture_workflow_run_requested()),
        ("workflow_job", fixture_workflow_job_queued()),
        ("workflow_job", fixture_workflow_job_in_progress()),
    ];

    for (i, (event_type, body)) in events.into_iter().enumerate() {
        let wh_client = client.clone();
        let wh_url = webhook_url.clone();
        let et = event_type.to_string();

        let state_client = client.clone();
        let st_url = state_url.clone();

        let (wh_result, state_result) = tokio::join!(
            async move {
                wh_client
                    .post(&wh_url)
                    .header("X-GitHub-Event", et)
                    .body(body)
                    .send()
                    .await
            },
            async move { state_client.get(&st_url).send().await }
        );

        wh_result.expect("webhook POST failed");
        let state_resp = state_result.expect("GET /v1/state failed");
        let json: serde_json::Value = state_resp.json().await.unwrap();

        let last_seq = json["lastSeq"].as_u64().unwrap() as usize;
        let runs = json["runs"].as_array().unwrap().len();
        let jobs = json["jobs"].as_array().unwrap().len();
        let entity_count = runs + jobs;

        // The snapshot must be self-consistent: entity_count == last_seq.
        // Two valid outcomes per iteration:
        //   - State read won the race: last_seq == i,     entity_count == i
        //   - Webhook won the race:    last_seq == i + 1, entity_count == i + 1
        // The old bug: last_seq == i + 1 but entity_count == i (cursor
        // overshoots snapshot content).
        assert_eq!(
            entity_count, last_seq,
            "iteration {i}: entity_count={entity_count} but last_seq={last_seq} — \
             snapshot content does not match cursor"
        );
    }
}

/// GET /v1/state returns seq consistent with all reflected events
#[tokio::test]
#[serial_test::serial]
async fn test_state_seq_consistency() {
    let (server_addr, _) = common::spawn_in_memory_server().await;

    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);

    // POST workflow_run_requested (seq 1 assigned)
    let body = fixture_workflow_run_requested();
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200);

    // POST workflow_job_queued (seq 2 assigned)
    let body = fixture_workflow_job_queued();
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200);

    // GET /v1/state should return lastSeq: 2 (highest committed)
    let state_url = format!("http://{}/v1/state", server_addr);
    let resp = client
        .get(&state_url)
        .send()
        .await
        .expect("GET /v1/state failed");

    assert_eq!(resp.status(), 200);

    let json: serde_json::Value = resp.json().await.expect("response is valid JSON");

    assert_eq!(
        json["lastSeq"], 2,
        "lastSeq should be 2 (highest committed); reflects events with seq 1 and 2"
    );
    let runs = json["runs"].as_array().unwrap();
    let jobs = json["jobs"].as_array().unwrap();
    assert!(
        !runs.is_empty(),
        "runs should reflect the workflow_run event"
    );
    assert!(
        !jobs.is_empty(),
        "jobs should reflect the workflow_job event"
    );
}
