//! Integration tests: PG state snapshot read path.
//!
//! Covers GET /v1/state in PG mode (snapshot reads runs and jobs from PG with
//! correct lastSeq under both quiet and concurrent-commit conditions) and the
//! in-memory fallback (when `pg_pool` is None, the handler reads from the
//! in-memory RunStateMachine, original behavior unchanged).
//!
//! Docker/OrbStack required for the PG-backed cases.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

// ─── snapshot returns PG state ──────────────────────────────────────────────

/// After committing a run and job via webhooks in PG mode, GET /v1/state
/// returns a snapshot containing both entities and lastSeq = 2.
///
/// In PG mode the state handler reads `broadcast_watermark` (the commit-order
/// cursor advanced by the drain after each successful pass) and then opens a
/// REPEATABLE READ transaction to read runs/jobs from a consistent snapshot.
/// The drain is asynchronous: a webhook returns 200 with `seq=N` BEFORE the
/// drain has necessarily processed seq=N. The test must wait for the drain
/// to catch up before asserting `lastSeq == N` — otherwise we race the
/// drain's first wake-up after each webhook.
#[tokio::test]
#[serial]
async fn snapshot_returns_pg_state() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Fire run webhook → seq=1.
    let (s1, b1) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(b1["status"], "accepted");
    assert_eq!(b1["seq"], 1);

    // Fire job webhook → seq=2.
    let (s2, b2) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b2["status"], "accepted");
    assert_eq!(b2["seq"], 2);

    // Wait for the drain to advance broadcast_watermark to 2 — that's the
    // commit-order cursor `state_handler` returns as `lastSeq`.
    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if fixture
                .state
                .broadcast_watermark
                .load(std::sync::atomic::Ordering::Acquire)
                >= 2
            {
                return;
            }
        }
    })
    .await
    .expect("drain did not advance broadcast_watermark to 2 within 5s");

    // GET /v1/state — REPEATABLE READ snapshot from PG.
    let (status, body) = get_state(&fixture.router).await;
    assert_eq!(status, StatusCode::OK, "GET /v1/state should return 200");

    // lastSeq reflects the drain's commit-order cursor, now caught up.
    assert_eq!(
        body["lastSeq"], 2,
        "lastSeq must be 2 after the drain catches up to two committed webhooks"
    );

    // runs array contains the run from the fixture.
    let runs = body["runs"].as_array().expect("runs should be an array");
    assert!(
        !runs.is_empty(),
        "runs should contain at least one entry after run webhook"
    );

    // The run ID from workflow_run_requested.json is 24290980517.
    let run_ids: Vec<i64> = runs.iter().filter_map(|r| r["id"].as_i64()).collect();
    assert!(
        run_ids.contains(&24290980517),
        "runs should contain run id 24290980517, got: {run_ids:?}"
    );

    // jobs array contains the job.
    let jobs = body["jobs"].as_array().expect("jobs should be an array");
    assert!(
        !jobs.is_empty(),
        "jobs should contain at least one entry after job webhook"
    );

    fixture.shutdown.cancel();
}

/// Placeholder run rows (created by job-before-run FK stubs) are excluded
/// from the /v1/state snapshot.
///
/// In PG mode, when a job event arrives before its run, `upsert_job_in_txn`
/// inserts a stub run row with `placeholder=true`. The state handler's
/// `read_all_runs` filters `WHERE placeholder=false`, so stub rows should never
/// appear in the snapshot.
#[tokio::test]
#[serial]
async fn placeholder_runs_excluded_from_snapshot() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Fire ONLY the job webhook — no run webhook. The job arrives before its run,
    // causing upsert_job_in_txn to create a placeholder run row.
    let (status, body) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "accepted");

    // GET /v1/state — runs should be empty (only the placeholder stub exists).
    let (status, snapshot) = get_state(&fixture.router).await;
    assert_eq!(status, StatusCode::OK);

    let runs = snapshot["runs"]
        .as_array()
        .expect("runs should be an array");
    assert!(
        runs.is_empty(),
        "runs should be empty when only a placeholder stub exists; got: {runs:?}"
    );

    // jobs should be present.
    let jobs = snapshot["jobs"]
        .as_array()
        .expect("jobs should be an array");
    assert!(!jobs.is_empty(), "jobs should contain the queued job");

    fixture.shutdown.cancel();
}

// ─── consistency under concurrent commits ──────────────────────────────────

/// Snapshot is self-consistent under concurrent webhook commits.
///
/// For each of three webhooks (each creating a distinct entity), the webhook
/// POST and GET /v1/state are issued concurrently via `tokio::join!`. The
/// invariant the frontend depends on is:
///
///   **For every seq ≤ last_seq, the entity that seq mutated is reflected in
///   the snapshot's runs/jobs view.**
///
/// In code terms: `entity_count >= last_seq` (the snapshot includes everything
/// the drain has broadcast, plus possibly more recent commits the drain hasn't
/// caught up on yet). The frontend's filter at connection.ts:113
/// (`if (buffered.seq > snapshotLastSeq)`) drops buffered events whose
/// mutation is already in the snapshot; this is safe iff that inequality holds.
///
/// Why >= and not strict equality: the cursor is `broadcast_watermark` loaded
/// BEFORE the snapshot transaction begins. Any commit that lands between the
/// load and the tx begin is invisible to the cursor but visible to the
/// snapshot — that gives `entity_count > last_seq`. The other direction
/// (`last_seq > entity_count`) would be a real bug: it would mean the cursor
/// advanced past content that the snapshot can't see, and the frontend would
/// permanently drop the buffered event.
///
/// Distinct entities used (each creating exactly one new entity):
///   0: workflow_run_requested   → run  24290980517
///   1: workflow_job_queued      → job  70928200168
///   2: workflow_job_in_progress → job  70928200174
///
/// All three fixtures reference run 24290980517. Event 0 commits the run first,
/// so by the time job events arrive the run already exists — no placeholder
/// stub is ever created. This keeps `entity_count` equal to the number of
/// committed events for the cases where the drain has caught up.
#[tokio::test]
#[serial]
async fn snapshot_self_consistent_under_concurrent_writes() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let events: Vec<(&str, Vec<u8>)> = vec![
        ("workflow_run", common::fixture_workflow_run_requested()),
        ("workflow_job", common::fixture_workflow_job_queued()),
        ("workflow_job", common::fixture_workflow_job_in_progress()),
    ];

    for (i, (event_type, body)) in events.into_iter().enumerate() {
        let wh_router = fixture.router.clone();
        let state_router = fixture.router.clone();

        let (wh_result, state_result) = tokio::join!(
            common::post_webhook_to_router(wh_router, event_type, &body),
            get_state(&state_router),
        );

        let (wh_status, wh_body) = wh_result;
        assert_eq!(wh_status, StatusCode::OK, "webhook {i} should return 200");
        assert_eq!(
            wh_body["status"], "accepted",
            "PG-mode webhook {i} should return accepted"
        );

        let (state_status, snapshot) = state_result;
        assert_eq!(
            state_status,
            StatusCode::OK,
            "GET /v1/state at iteration {i} should return 200"
        );

        let last_seq = snapshot["lastSeq"].as_u64().unwrap_or(0) as usize;
        let runs = snapshot["runs"].as_array().unwrap();
        let jobs = snapshot["jobs"].as_array().unwrap();
        let entity_count = runs.len() + jobs.len();

        // The frontend invariant: entity_count >= last_seq.
        // entity_count > last_seq just means the snapshot caught a commit
        // the drain hasn't yet broadcast (acceptable — the buffered event
        // for that seq will arrive and be applied idempotently).
        // entity_count < last_seq would be a real bug.
        assert!(
            entity_count >= last_seq,
            "iteration {i}: entity_count={entity_count} < last_seq={last_seq} — \
             snapshot is missing content the cursor advertises"
        );
    }

    fixture.shutdown.cancel();
}

// ─── in-memory fallback when pg_pool is None ───────────────────────────────

/// When pg_pool is None (in-memory-only mode), GET /v1/state reads from
/// the in-memory RunStateMachine, not from PG.
///
/// Uses `build_app_no_secret()` which wires `pg_pool: None`. Fires one run
/// webhook. Expects lastSeq=1 and the run in the snapshot — but via the
/// in-memory path (seq mutex held, RunStateMachine.snapshot() called).
#[tokio::test]
#[serial]
async fn in_memory_fallback() {
    // Build app with no PG pool.
    let (router, state) = common::build_app_no_secret();

    // Fire run webhook.
    let (status, body) = common::post_webhook_to_router(
        router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // In-memory mode returns "accepted" with a numeric seq.
    assert_eq!(
        body["status"], "accepted",
        "in-memory mode should return accepted"
    );

    // GET /v1/state should read from in-memory store.
    let (s, snapshot) = get_state(&router).await;
    assert_eq!(s, StatusCode::OK);

    // lastSeq=1 after one in-memory event.
    assert_eq!(
        snapshot["lastSeq"], 1,
        "in-memory lastSeq should be 1 after one event"
    );

    let runs = snapshot["runs"].as_array().unwrap();
    assert!(
        !runs.is_empty(),
        "in-memory snapshot should include the run"
    );

    // pg_pool is None.
    assert!(
        state.pg_pool.is_none(),
        "pg_pool must be None in in-memory mode"
    );
}

// ─── helper ──────────────────────────────────────────────────────────────────

/// GET /v1/state through the given router and return (status, json_body).
async fn get_state(router: &axum::Router) -> (StatusCode, serde_json::Value) {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let req = Request::builder()
        .method("GET")
        .uri("/v1/state")
        .body(Body::empty())
        .unwrap();

    let resp = timeout(Duration::from_secs(5), router.clone().oneshot(req))
        .await
        .expect("GET /v1/state timed out")
        .unwrap();

    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}
