use std::net::SocketAddr;
use std::sync::Arc;

use crate::common;
use common::{fixture_workflow_job_queued, fixture_workflow_run_requested};

/// Setup an ephemeral HTTP server for testing (in-memory mode).
async fn test_setup() -> (SocketAddr, Arc<atc_server::state::AppState>) {
    common::ensure_recorder_installed();

    let (app, app_state) = common::build_app_no_secret();

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, app).await.unwrap();
    });

    (main_addr, app_state)
}

/// GET /v1/state with no prior events returns seq: 0, empty collections
#[tokio::test]
#[serial_test::serial]
async fn test_empty_state() {
    let (server_addr, _) = test_setup().await;

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
}

/// GET /v1/state after workflow_run_requested webhook returns seq: 1, run in runs
#[tokio::test]
#[serial_test::serial]
async fn test_state_after_run_event() {
    let (server_addr, _) = test_setup().await;

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

    let (server_addr, _) = test_setup().await;

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
    let (server_addr, _) = test_setup().await;

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
