//! Phase 3c integration tests: PG state snapshot read path.
//!
//! T1 — Snapshot returns PG state: GET /v1/state reads runs and jobs from
//!      PG after commit, returns them with correct lastSeq.
//! T2 — Self-consistent snapshot under concurrent commits: REPEATABLE READ
//!      guarantees entity_count == lastSeq (strong equality, not just >=).
//! T3 — In-memory fallback: when pg_pool is None, GET /v1/state reads from
//!      the in-memory StateStore (original behavior unchanged).
//!
//! Docker/OrbStack required for T1 and T2.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

// ─── T1: snapshot returns PG state ──────────────────────────────────────────

/// T1: After committing a run and job via webhooks in PG mode, GET /v1/state
///     returns a snapshot containing both entities and lastSeq = 2.
///
/// In PG mode the state handler uses a REPEATABLE READ transaction to read
/// runs, jobs, and MAX(seq) atomically. This test confirms the full path:
/// webhook → PG UPSERT + outbox row → NOTIFY → drain broadcasts + advances
/// watermark → /v1/state reads from PG.
///
/// Note: /v1/state reads directly from PG tables, NOT from the drain. It does
/// NOT require the drain to have processed the outbox rows — the UPSERT happens
/// in the same transaction as the outbox INSERT, so the run/job are immediately
/// visible to the state handler once the webhook commits.
#[tokio::test]
#[serial]
async fn phase_3c_state_pg_read_t1_snapshot_returns_pg_state() {
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

    // GET /v1/state — REPEATABLE READ snapshot from PG.
    let (status, body) = get_state(&fixture.router).await;
    assert_eq!(status, StatusCode::OK, "GET /v1/state should return 200");

    // lastSeq reflects highest committed outbox seq.
    assert_eq!(
        body["lastSeq"], 2,
        "lastSeq must be 2 after two committed webhooks"
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

/// T1b: Placeholder run rows (created by job-before-run FK stubs) are excluded
///      from the /v1/state snapshot.
///
/// In PG mode, when a job event arrives before its run, `upsert_job_in_txn`
/// inserts a stub run row with `placeholder=true`. The state handler's
/// `read_all_runs` filters `WHERE placeholder=false`, so stub rows should never
/// appear in the snapshot.
#[tokio::test]
#[serial]
async fn phase_3c_state_pg_read_t1b_placeholder_runs_excluded_from_snapshot() {
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

// ─── T2: consistency under concurrent commits ────────────────────────────────

/// T2: Snapshot is self-consistent under concurrent webhook commits.
///
/// For each of three webhooks (each creating a distinct entity), the webhook
/// POST and GET /v1/state are issued concurrently via `tokio::join!`. The
/// REPEATABLE READ snapshot atomically reads entity tables AND `MAX(seq)` from
/// the outbox in the same MVCC snapshot, so the invariant is:
///
///   entity_count == last_seq  (strong equality, not just >=)
///
/// Two valid outcomes per iteration:
///   - State read won the race: last_seq == i,     entity_count == i
///   - Webhook won the race:    last_seq == i + 1, entity_count == i + 1
///
/// The bug this guards against: last_seq == i + 1 but entity_count == i
/// (cursor overshoots snapshot content — possible if seq and entity rows are
/// not read in the same atomic snapshot).
///
/// Distinct entities used (each creating exactly one new entity):
///   0: workflow_run_requested   → run  24290980517
///   1: workflow_job_queued      → job  70928200168
///   2: workflow_job_in_progress → job  70928200174
///
/// All three fixtures reference run 24290980517. Event 0 commits the run first,
/// so by the time job events arrive the run already exists — no placeholder stub
/// is ever created. This preserves entity_count == lastSeq: a placeholder row
/// would increment `MAX(seq)` without adding to `runs.len()`, which would break
/// the invariant for the run-events-concurrent-with-state-read case.
///
/// Note: The PG handler sets `seq` in the outbox (BIGSERIAL), not the
/// in-memory Mutex. The in-memory seq counter stays at 0 in PG mode — the
/// drain increments the broadcast stream. The state handler reads `MAX(seq)`
/// from outbox directly for `lastSeq`.
#[tokio::test]
#[serial]
async fn phase_3c_state_pg_read_t2_snapshot_self_consistent_under_concurrent_writes() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Three events that each create a distinct entity:
    //   0: workflow_run_requested   → run  24290980517
    //   1: workflow_job_queued      → job  70928200168
    //   2: workflow_job_in_progress → job  70928200174
    // After event K commits: runs.len() + jobs.len() == K + 1.
    let events: Vec<(&str, Vec<u8>)> = vec![
        ("workflow_run", common::fixture_workflow_run_requested()),
        ("workflow_job", common::fixture_workflow_job_queued()),
        ("workflow_job", common::fixture_workflow_job_in_progress()),
    ];

    for (i, (event_type, body)) in events.into_iter().enumerate() {
        // Clone the router once per iteration — each oneshot() call consumes it,
        // so we need two independent clones for the concurrent pair.
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

        // Under REPEATABLE READ, entity_count and last_seq come from the same
        // MVCC snapshot — they must agree exactly.
        // Two valid states: (last_seq == i, entity_count == i) if state won the
        // race, or (last_seq == i + 1, entity_count == i + 1) if webhook won.
        assert_eq!(
            entity_count, last_seq,
            "iteration {i}: entity_count={entity_count} != last_seq={last_seq} — \
             REPEATABLE READ snapshot is inconsistent"
        );
    }

    fixture.shutdown.cancel();
}

// ─── T3: in-memory fallback when pg_pool is None ────────────────────────────

/// T3: When pg_pool is None (in-memory-only mode), GET /v1/state reads from
///     the in-memory StateStore, not from PG.
///
/// Uses `build_app_no_secret()` which wires `pg_pool: None`. Fires one run
/// webhook. Expects lastSeq=1 and the run in the snapshot — but via the
/// in-memory path (seq mutex held, StateStore.snapshot() called).
#[tokio::test]
#[serial]
async fn phase_3c_state_pg_read_t3_in_memory_fallback() {
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
    // In-memory mode returns "processed" (not "accepted").
    assert_eq!(
        body["status"], "processed",
        "in-memory mode should return processed"
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
