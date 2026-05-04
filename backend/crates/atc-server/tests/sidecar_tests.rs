//! Sidecar lifecycle tests for pool_stats_after broadcast emission.
//! Organized by acceptance criteria (AC1.1, AC1.2, AC1.3, AC1.5).
//!
//! These tests verify that:
//! - AC1.1: Successful Job events emit a sidecar equal to store.pool_stats() at that moment
//! - AC1.2: Successive Job events show evolving pool stats (queued → running → completed)
//! - AC1.3: Run events emit a sidecar with None
//! - AC1.5: Failed Job transitions produce no broadcast

use std::sync::Arc;
use tokio::sync::broadcast::error::TryRecvError;

mod common;
use common::fixture_workflow_run_requested;

/// Build a minimal workflow_job webhook payload with configurable IDs and status.
/// Used by AC1.2 and AC1.5 tests to construct job events with matching (run_id, job_id).
fn job_payload(
    action: &str,
    status: &str,
    conclusion: Option<&str>,
    run_id: u64,
    job_id: u64,
    labels: &[&str],
) -> Vec<u8> {
    let labels_json = labels
        .iter()
        .map(|l| format!("\"{}\"", l))
        .collect::<Vec<_>>()
        .join(",");

    let payload = format!(
        r#"{{
  "action": "{}",
  "workflow_job": {{
    "id": {},
    "run_id": {},
    "workflow_name": "TestWorkflow",
    "head_branch": "main",
    "run_url": "https://api.github.com/repos/test/repo/actions/runs/{}",
    "run_attempt": 1,
    "node_id": "CR_test",
    "head_sha": "abc123",
    "url": "https://api.github.com/repos/test/repo/actions/jobs/{}",
    "html_url": "https://github.com/test/repo/actions/runs/{}/job/{}",
    "status": "{}",
    "conclusion": {},
    "created_at": "2026-04-18T12:00:00Z",
    "started_at": "2026-04-18T12:00:00Z",
    "completed_at": null,
    "name": "test-job",
    "steps": [],
    "check_run_url": "https://api.github.com/repos/test/repo/check-runs/{}",
    "labels": [{}],
    "runner_id": null,
    "runner_name": null,
    "runner_group_id": null,
    "runner_group_name": null
  }},
  "repository": {{
    "id": 1,
    "node_id": "R_test",
    "name": "repo",
    "full_name": "test/repo",
    "private": false,
    "owner": {{
      "login": "test",
      "id": 1,
      "node_id": "U_test",
      "avatar_url": "https://example.com/avatar.jpg",
      "gravatar_id": "",
      "url": "https://api.github.com/users/test",
      "html_url": "https://github.com/test",
      "followers_url": "https://api.github.com/users/test/followers",
      "following_url": "https://api.github.com/users/test/following{{/other_user}}",
      "gists_url": "https://api.github.com/users/test/gists{{/gist_id}}",
      "starred_url": "https://api.github.com/users/test/starred{{/owner}}{{/repo}}",
      "subscriptions_url": "https://api.github.com/users/test/subscriptions",
      "organizations_url": "https://api.github.com/users/test/orgs",
      "repos_url": "https://api.github.com/users/test/repos",
      "events_url": "https://api.github.com/users/test/events{{/privacy}}",
      "received_events_url": "https://api.github.com/users/test/received_events",
      "type": "User",
      "user_view_type": "public",
      "site_admin": false
    }},
    "html_url": "https://github.com/test/repo",
    "description": "Test Repo",
    "fork": false,
    "url": "https://api.github.com/repos/test/repo",
    "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-04-18T12:00:00Z",
    "pushed_at": "2026-04-18T12:00:00Z",
    "git_url": "git://github.com/test/repo.git",
    "ssh_url": "git@github.com:test/repo.git",
    "clone_url": "https://github.com/test/repo.git",
    "svn_url": "https://svn.github.com/test/repo",
    "homepage": null,
    "size": 1,
    "stargazers_count": 0,
    "watchers_count": 0,
    "language": null,
    "has_issues": true,
    "has_projects": true,
    "has_downloads": true,
    "has_wiki": true,
    "has_pages": false,
    "forks_count": 0,
    "mirror_url": null,
    "open_issues_count": 0,
    "forks": 0,
    "open_issues": 0,
    "watchers": 0,
    "default_branch": "main"
  }}
}}"#,
        action,
        job_id,
        run_id,
        run_id,
        job_id,
        run_id,
        job_id,
        status,
        conclusion
            .map(|c| format!("\"{}\"", c))
            .unwrap_or_else(|| "null".to_string()),
        job_id,
        labels_json
    );

    payload.into_bytes()
}

/// AC1.1: A successful Job event produces a SeqEvent whose pool_stats_after
/// equals store.pool_stats() evaluated immediately after the event applies.
///
/// This is a full-stack test using broadcast channel subscription.
#[tokio::test]
#[serial_test::serial]
async fn job_event_produces_populated_sidecar_equal_to_pool_stats_after_apply() {
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(atc_core::StateStore::new(
        Arc::new(atc_core::SystemClock),
        std::time::Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(atc_server::state::AppState {
        store: store.clone(),
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });

    let main_router = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    let client = reqwest::Client::new();

    // Subscribe to broadcast BEFORE ingesting any events
    let mut rx = app_state.webhook_tx.subscribe();

    // Ingest workflow_run first
    let run_fixture = fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", main_addr);

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(run_fixture)
        .send()
        .await
        .expect("Run webhook POST failed");
    assert_eq!(resp.status(), 200);

    // Ingest workflow_job_queued
    let job_fixture = job_payload("queued", "queued", None, 999, 888, &["ubuntu-latest"]);
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(job_fixture)
        .send()
        .await
        .expect("Job webhook POST failed");
    assert_eq!(resp.status(), 200);

    // Receive the two broadcasts (run event + job event)
    // Skip the Run event
    let _ = rx.recv().await.expect("should receive run event");

    // Receive and check the Job event
    let seq_event = rx.recv().await.expect("should receive job event");

    // AC1.1: pool_stats_after should be Some(vec) and equal to store.pool_stats()
    assert!(
        seq_event.pool_stats_after.is_some(),
        "Job event should have populated pool_stats_after"
    );

    let pool_stats_after = seq_event.pool_stats_after.unwrap();
    let store_pool_stats = store.pool_stats().await;

    // Both should be equal (same sort order guaranteed by Task 2)
    assert_eq!(
        pool_stats_after, store_pool_stats,
        "pool_stats_after should match store.pool_stats() at apply time"
    );

    // Sanity check: should have at least one pool (the job's label set)
    assert!(
        !pool_stats_after.is_empty(),
        "pool_stats_after should not be empty"
    );
}

/// AC1.2: Successive Job events for the same job (same run_id, job_id) evolve the sidecar state.
/// Queued → InProgress → Completed should show:
/// - After Queued: {queued: 1, running: 0}
/// - After InProgress: {queued: 0, running: 1}
/// - After Completed: empty (no active jobs remain)
#[tokio::test]
#[serial_test::serial]
async fn successive_job_events_evolve_sidecar_state() {
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(atc_core::StateStore::new(
        Arc::new(atc_core::SystemClock),
        std::time::Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(atc_server::state::AppState {
        store: store.clone(),
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });

    let main_router = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    let client = reqwest::Client::new();
    let mut rx = app_state.webhook_tx.subscribe();

    // Ingest workflow_run
    let run_fixture = fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", main_addr);

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(run_fixture)
        .send()
        .await
        .expect("Run webhook POST failed");
    assert_eq!(resp.status(), 200);

    // Drain the run event
    let _ = rx.recv().await;

    // AC1.2: Three job events for the same (run_id=777, job_id=888) with matching labels
    let run_id = 777u64;
    let job_id = 888u64;
    let labels = vec!["self-hosted", "linux"];

    // Event 1: Queued
    let payload_queued = job_payload("queued", "queued", None, run_id, job_id, &labels);
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(payload_queued)
        .send()
        .await
        .expect("Queued webhook POST failed");
    assert_eq!(resp.status(), 200);

    let event1 = rx.recv().await.expect("should receive queued event");
    assert!(event1.pool_stats_after.is_some());
    let stats1 = event1.pool_stats_after.unwrap();
    assert_eq!(stats1.len(), 1, "should have one pool after queued");
    assert_eq!(
        stats1[0].queued, 1,
        "queued count should be 1 after queued event"
    );
    assert_eq!(
        stats1[0].running, 0,
        "running count should be 0 after queued event"
    );

    // Event 2: InProgress
    let payload_in_progress =
        job_payload("in_progress", "in_progress", None, run_id, job_id, &labels);
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(payload_in_progress)
        .send()
        .await
        .expect("InProgress webhook POST failed");
    assert_eq!(resp.status(), 200);

    let event2 = rx.recv().await.expect("should receive in_progress event");
    assert!(event2.pool_stats_after.is_some());
    let stats2 = event2.pool_stats_after.unwrap();
    assert_eq!(stats2.len(), 1, "should have one pool after in_progress");
    assert_eq!(
        stats2[0].queued, 0,
        "queued count should be 0 after in_progress event"
    );
    assert_eq!(
        stats2[0].running, 1,
        "running count should be 1 after in_progress event"
    );

    // Event 3: Completed
    let payload_completed = job_payload(
        "completed",
        "completed",
        Some("success"),
        run_id,
        job_id,
        &labels,
    );
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(payload_completed)
        .send()
        .await
        .expect("Completed webhook POST failed");
    assert_eq!(resp.status(), 200);

    let event3 = rx.recv().await.expect("should receive completed event");
    assert!(event3.pool_stats_after.is_some());
    let stats3 = event3.pool_stats_after.unwrap();
    // After completion, the job is removed from active tracking (no queued or running)
    assert!(
        stats3.is_empty(),
        "pool_stats_after should be empty after completed event (no active jobs remain)"
    );
}

/// AC1.3: A Run event produces a SeqEvent whose pool_stats_after is None.
#[tokio::test]
#[serial_test::serial]
async fn run_event_produces_none_sidecar() {
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(atc_core::StateStore::new(
        Arc::new(atc_core::SystemClock),
        std::time::Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(atc_server::state::AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });

    let main_router = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    let client = reqwest::Client::new();
    let mut rx = app_state.webhook_tx.subscribe();

    // Ingest workflow_run
    let run_fixture = fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", main_addr);

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(run_fixture)
        .send()
        .await
        .expect("Run webhook POST failed");
    assert_eq!(resp.status(), 200);

    // Receive the broadcast
    let seq_event = rx.recv().await.expect("should receive run event");

    // AC1.3: pool_stats_after should be None for Run events
    assert!(
        seq_event.pool_stats_after.is_none(),
        "Run event should have None pool_stats_after"
    );
}

/// AC1.5: A Job event that returns a store transition error results in no broadcast.
/// We drive a valid Completed Job, then a backward Queued transition (invalid),
/// and assert the second event is not broadcast.
#[tokio::test]
#[serial_test::serial]
async fn failed_job_transition_produces_no_broadcast() {
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(atc_core::StateStore::new(
        Arc::new(atc_core::SystemClock),
        std::time::Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(atc_server::state::AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });

    let main_router = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    let client = reqwest::Client::new();
    let mut rx = app_state.webhook_tx.subscribe();

    // Ingest workflow_run first (needed for store to accept job events)
    let run_fixture = fixture_workflow_run_requested();
    let webhook_url = format!("http://{}/v1/webhooks/github", main_addr);

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(run_fixture)
        .send()
        .await
        .expect("Run webhook POST failed");
    assert_eq!(resp.status(), 200);

    // Drain the run event
    let _ = rx.recv().await;

    let run_id = 555u64;
    let job_id = 666u64;

    // Event 1: Completed job (valid forward transition)
    let payload_completed = job_payload(
        "completed",
        "completed",
        Some("success"),
        run_id,
        job_id,
        &["ubuntu-latest"],
    );
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(payload_completed)
        .send()
        .await
        .expect("Completed webhook POST failed");
    assert_eq!(resp.status(), 200, "first job event should return 200");

    // Receive the first job event
    let first_event = rx.recv().await.expect("should receive completed event");
    assert!(
        first_event.pool_stats_after.is_some(),
        "first job event (completed) should have pool_stats_after"
    );

    // Event 2: Queued job for the same (run_id, job_id) — invalid backward transition
    // atc-core's forward-only invariant will return Err
    let payload_queued = job_payload("queued", "queued", None, run_id, job_id, &["ubuntu-latest"]);
    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_job")
        .body(payload_queued)
        .send()
        .await
        .expect("Queued webhook POST failed");
    // AC1.5: Handler still returns 200 even though the transition failed
    assert_eq!(
        resp.status(),
        200,
        "failed transition should still return 200"
    );

    // AC1.5: No second SeqEvent should be broadcast (no seq bump, no broadcast)
    // The HTTP response was already awaited above, which happens-after the handler
    // dropped the seq mutex (which gates the broadcast send). So the broadcast's
    // send() is either complete or will never happen — try_recv() is synchronous.
    match rx.try_recv() {
        Err(TryRecvError::Empty) => {
            // Expected: no broadcast for failed transition
        }
        Ok(_) => {
            panic!("should not receive broadcast for failed transition")
        }
        Err(TryRecvError::Lagged(_)) => {
            panic!("lagged, but no broadcast expected")
        }
        Err(TryRecvError::Closed) => {
            panic!("channel closed unexpectedly")
        }
    }
}
