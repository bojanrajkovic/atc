//! Integration tests for PgStore persistence against a live PostgreSQL container.
//!
//! Covers run-event durable write, job-event durable write including
//! job-before-run, and field-merge parity with in-memory store.
//!
//! Requires Docker (or OrbStack) to be running.

use crate::common;

use std::time::Duration;

use atc_core::{
    JobStatus, PersistError,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
    fixed_test_timestamp,
    types::{JobId, RunId},
};
use atc_persist::PersistentStore;
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;

fn ts() -> DateTime<Utc> {
    fixed_test_timestamp()
}

/// Minimal RunEventEnvelope for a Requested (Queued) event.
fn run_requested(run_id: i64) -> RunEventEnvelope {
    common::make_run_envelope(RunId(run_id), RunEvent::Requested)
}

/// InProgress run event.
///
/// `workflow_name` and `workflow_path` are deliberately `None` — GitHub omits
/// them on `in_progress` events, and the COALESCE in the UPSERT must preserve
/// the value from the `Requested` row.
fn run_in_progress(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        workflow_name: None,
        workflow_path: None,
        run_started_at: Some(ts()),
        action: RunEvent::InProgress,
        ..common::make_run_envelope(RunId(run_id), RunEvent::Requested)
    }
}

/// Completed run event.
fn run_completed(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        workflow_name: None,
        workflow_path: None,
        run_started_at: Some(ts()),
        ..common::make_run_envelope(
            RunId(run_id),
            RunEvent::Completed {
                conclusion: atc_core::RunConclusion::Success,
            },
        )
    }
}

/// Minimal queued job envelope.
fn job_queued(job_id: i64, run_id: i64) -> JobEventEnvelope {
    common::make_job_envelope(
        JobId(job_id),
        RunId(run_id),
        JobEvent::Queued {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    )
}

/// InProgress job envelope with runner info.
fn job_in_progress(job_id: i64, run_id: i64) -> JobEventEnvelope {
    common::make_job_envelope(
        JobId(job_id),
        RunId(run_id),
        JobEvent::InProgress {
            runner: Some(atc_core::job::RunnerInfo {
                id: 42,
                name: "runner-1".to_string(),
                group_name: Some("default".to_string()),
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    )
}

/// Completed job envelope.
fn job_completed(job_id: i64, run_id: i64) -> JobEventEnvelope {
    common::make_job_envelope(
        JobId(job_id),
        RunId(run_id),
        JobEvent::Completed {
            conclusion: atc_core::JobConclusion::Success,
            runner: Some(atc_core::job::RunnerInfo {
                id: 42,
                name: "runner-1".to_string(),
                group_name: Some("default".to_string()),
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    )
}

// ---------------------------------------------------------------------------
// Run-event durable writes
// ---------------------------------------------------------------------------

/// Unknown run_id, Requested event → 1 row in `runs` with Queued status.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_first_sight_creates_row() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    let env = run_requested(1001);
    let result = store.apply_run_event(env).await;
    assert!(result.is_ok(), "expected Ok but got: {result:?}");

    let row = sqlx::query!("SELECT id, status, workflow_name FROM runs WHERE id = 1001")
        .fetch_one(&pool)
        .await
        .expect("row not found");

    assert_eq!(row.id, 1001i64);
    assert_eq!(row.status, "Queued");
    assert_eq!(row.workflow_name.as_deref(), Some("CI"));
    shutdown.cancel();
}

/// Queued → InProgress is valid; status updates, sticky COALESCE preserved.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_valid_transition_updates_row() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(1002)).await.unwrap();

    let result = store.apply_run_event(run_in_progress(1002)).await;
    assert!(result.is_ok(), "expected Ok but got: {result:?}");

    let row =
        sqlx::query!("SELECT status, workflow_name, run_started_at FROM runs WHERE id = 1002")
            .fetch_one(&pool)
            .await
            .expect("row not found");

    assert_eq!(row.status, "InProgress");
    // workflow_name was set on Requested, omitted on InProgress → COALESCE preserves it
    assert_eq!(row.workflow_name.as_deref(), Some("CI"));
    assert!(row.run_started_at.is_some(), "run_started_at should be set");
    shutdown.cancel();
}

/// Completed → InProgress is invalid; PG returns Err(InvalidTransition) and row unchanged.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_invalid_transition_returns_err() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Bring run to Completed
    store.apply_run_event(run_requested(1003)).await.unwrap();
    store.apply_run_event(run_in_progress(1003)).await.unwrap();
    store.apply_run_event(run_completed(1003)).await.unwrap();

    // Now attempt Completed → InProgress (invalid)
    let result = store.apply_run_event(run_in_progress(1003)).await;
    assert!(
        matches!(result, Err(PersistError::InvalidTransition)),
        "expected InvalidTransition, got: {result:?}"
    );

    // PG row unchanged — still Completed
    let row = sqlx::query!("SELECT status FROM runs WHERE id = 1003")
        .fetch_one(&pool)
        .await
        .expect("row not found");
    assert_eq!(row.status, "Completed");
    shutdown.cancel();
}

/// GitHub re-run: a higher `run_attempt` reopens a Completed run.
///
/// GitHub reuses the same `run_id` for re-runs and increments `run_attempt`.
/// The UPSERT predicate admits the update via `EXCLUDED.run_attempt >
/// runs.run_attempt` even though the stored status is terminal, and the
/// reset CASE expressions clear `conclusion` / `completed_at`. This is the
/// regression guard for the dropped-re-run bug.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_higher_attempt_reopens_completed_run() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Attempt 1: run completes with a Cancelled conclusion.
    store.apply_run_event(run_requested(1010)).await.unwrap();
    store.apply_run_event(run_in_progress(1010)).await.unwrap();
    store
        .apply_run_event(RunEventEnvelope {
            completed_at: Some(ts()),
            ..run_completed(1010)
        })
        .await
        .unwrap();

    // Attempt 2: GitHub re-runs the same run_id with run_attempt = 2,
    // in_progress. The forward-only guard alone would reject this.
    let rerun = RunEventEnvelope {
        run_attempt: 2,
        ..run_in_progress(1010)
    };
    let result = store.apply_run_event(rerun).await;
    assert!(result.is_ok(), "re-run should be admitted, got: {result:?}");

    let row = sqlx::query!(
        "SELECT status, conclusion, completed_at, run_attempt FROM runs WHERE id = 1010"
    )
    .fetch_one(&pool)
    .await
    .expect("row not found");

    assert_eq!(row.status, "InProgress", "re-run should reopen the run");
    assert_eq!(row.run_attempt, 2, "run_attempt should advance to 2");
    assert!(
        row.conclusion.is_none(),
        "terminal conclusion should reset on a new attempt, got {:?}",
        row.conclusion
    );
    assert!(
        row.completed_at.is_none(),
        "completed_at should reset on a new attempt, got {:?}",
        row.completed_at
    );
    shutdown.cancel();
}

/// A stale lower `run_attempt` event must NOT reopen or re-conclude a run that
/// has already advanced to a newer attempt.
///
/// GitHub can deliver a delayed attempt-1 `completed` webhook after attempt 2
/// is already in progress. Without the `EXCLUDED.run_attempt = runs.run_attempt`
/// gate on the status-transition branch, that stale event would match (since
/// `InProgress` is a valid predecessor of `Completed`), regress run_attempt to
/// 1, and close the live attempt with the old conclusion.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_stale_lower_attempt_rejected() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Attempt 1 completes, then attempt 2 reopens the run (now InProgress @ 2).
    store.apply_run_event(run_requested(1011)).await.unwrap();
    store.apply_run_event(run_completed(1011)).await.unwrap();
    store
        .apply_run_event(RunEventEnvelope {
            run_attempt: 2,
            ..run_in_progress(1011)
        })
        .await
        .unwrap();

    // A delayed attempt-1 completed event arrives late. It must be rejected.
    let stale = run_completed(1011); // run_attempt = 1
    let result = store.apply_run_event(stale).await;
    assert!(
        matches!(result, Err(PersistError::InvalidTransition)),
        "stale lower attempt should be rejected, got: {result:?}"
    );

    let row = sqlx::query!("SELECT status, conclusion, run_attempt FROM runs WHERE id = 1011")
        .fetch_one(&pool)
        .await
        .expect("row not found");
    assert_eq!(row.status, "InProgress", "live attempt must stay open");
    assert_eq!(row.run_attempt, 2, "run_attempt must not regress to 1");
    assert!(
        row.conclusion.is_none(),
        "live attempt must not inherit the stale conclusion, got {:?}",
        row.conclusion
    );
    shutdown.cancel();
}

/// A re-run's jobs supersede the prior attempt's in the snapshot: only jobs
/// whose `run_attempt` matches the run's current attempt are returned.
///
/// GitHub assigns fresh job IDs per attempt under the same run_id, so without
/// the `j.run_attempt = r.run_attempt` read filter the card would mix dead
/// attempt-1 jobs with the live attempt-2 ones.
#[tokio::test]
#[serial_test::serial]
async fn pg_jobs_filtered_to_current_attempt() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Attempt 1: a completed job under an in-progress run.
    store.apply_run_event(run_requested(1020)).await.unwrap();
    store.apply_run_event(run_in_progress(1020)).await.unwrap();
    store
        .apply_job_event(job_completed(8001, 1020))
        .await
        .unwrap();

    // Re-run: attempt 2 reopens the run with a fresh job ID.
    store
        .apply_run_event(RunEventEnvelope {
            run_attempt: 2,
            ..run_in_progress(1020)
        })
        .await
        .unwrap();
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 2,
            ..job_in_progress(8002, 1020)
        })
        .await
        .unwrap();

    let snap = store.read_snapshot(None).await.expect("snapshot");
    let job_ids: Vec<i64> = snap
        .jobs
        .iter()
        .filter(|j| j.run_id == RunId(1020))
        .map(|j| j.id.0)
        .collect();
    assert_eq!(
        job_ids,
        vec![8002],
        "snapshot must return only the current attempt's job (8002), not the prior attempt's (8001); got {job_ids:?}"
    );
    shutdown.cancel();
}

/// A higher-attempt queued job stays visible even before the run row advances.
///
/// GitHub emits no `workflow_run.requested` for a queued re-run, so the first
/// signal can be `workflow_job.queued` at attempt 2 while the run is still the
/// completed attempt 1. The `j.run_attempt >= r.run_attempt` read filter keeps
/// that queued demand visible (only strictly-lower attempts are stale).
#[tokio::test]
#[serial_test::serial]
async fn pg_higher_attempt_job_visible_before_run_advances() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Attempt 1 completes.
    store.apply_run_event(run_requested(1030)).await.unwrap();
    store.apply_run_event(run_completed(1030)).await.unwrap();

    // An attempt-2 queued job arrives before the attempt-2 run event would.
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 2,
            ..job_queued(8003, 1030)
        })
        .await
        .unwrap();

    let snap = store.read_snapshot(None).await.expect("snapshot");
    let job_ids: Vec<i64> = snap
        .jobs
        .iter()
        .filter(|j| j.run_id == RunId(1030))
        .map(|j| j.id.0)
        .collect();
    assert_eq!(
        job_ids,
        vec![8003],
        "a higher-attempt queued job must stay visible before the run advances; got {job_ids:?}"
    );
    shutdown.cancel();
}

/// A higher-attempt job survives the display-TTL cutoff even when its parent
/// run has aged out. Re-running a long-completed run sends `workflow_job.queued`
/// (attempt 2) before any run event, so the parent row is still the aged-out
/// attempt-1 Completed run; the fresh job must not be gated on that stale row's
/// cutoff.
#[tokio::test]
#[serial_test::serial]
async fn pg_higher_attempt_job_bypasses_stale_parent_cutoff() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Attempt 1 completed "long ago" — well before any reasonable cutoff.
    let old = ts() - chrono::Duration::hours(48);
    store.apply_run_event(run_requested(1040)).await.unwrap();
    store
        .apply_run_event(RunEventEnvelope {
            completed_at: Some(old),
            updated_at: old,
            ..run_completed(1040)
        })
        .await
        .unwrap();

    // Re-run: attempt-2 queued job arrives before the attempt-2 run event.
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 2,
            ..job_queued(8004, 1040)
        })
        .await
        .unwrap();

    // Snapshot with a cutoff 1h ago: the attempt-1 run is aged out, but the
    // fresh attempt-2 job must still appear (bypasses the stale parent cutoff).
    let cutoff = ts() - chrono::Duration::hours(1);
    let snap = store.read_snapshot(Some(cutoff)).await.expect("snapshot");
    let job_ids: Vec<i64> = snap
        .jobs
        .iter()
        .filter(|j| j.run_id == RunId(1040))
        .map(|j| j.id.0)
        .collect();
    assert_eq!(
        job_ids,
        vec![8004],
        "a higher-attempt job must survive the aged-out parent's cutoff; got {job_ids:?}"
    );
    shutdown.cancel();
}

/// Queued → Queued is idempotent (same-status replay → Ok).
#[tokio::test]
#[serial_test::serial]
async fn pg_run_idempotent_same_status_replay() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(1004)).await.unwrap();
    let result = store.apply_run_event(run_requested(1004)).await;
    assert!(
        result.is_ok(),
        "same-status replay should be Ok: {result:?}"
    );

    let row = sqlx::query!("SELECT status FROM runs WHERE id = 1004")
        .fetch_one(&pool)
        .await
        .expect("row not found");
    assert_eq!(row.status, "Queued");
    shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Job-event durable writes including job-before-run
// ---------------------------------------------------------------------------

/// Job arrives after run → creates job row, no spurious stub.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_first_sight_creates_row_with_existing_run() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Pre-insert real run
    store.apply_run_event(run_requested(2001)).await.unwrap();

    let result = store.apply_job_event(job_queued(3001, 2001)).await;
    assert!(result.is_ok(), "expected Ok: {result:?}");

    let row = sqlx::query!("SELECT id, run_id, status FROM jobs WHERE id = 3001")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(row.id, 3001i64);
    assert_eq!(row.run_id, 2001i64);
    assert_eq!(row.status, "Queued");

    // Exactly one run row should exist (the real one)
    let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE id = 2001")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run_count, 1, "should be exactly one run row");
    shutdown.cancel();
}

/// Valid job transition Queued → InProgress.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_valid_transition_updates_row() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(2002)).await.unwrap();
    store.apply_job_event(job_queued(3002, 2002)).await.unwrap();

    let result = store.apply_job_event(job_in_progress(3002, 2002)).await;
    assert!(result.is_ok(), "expected Ok: {result:?}");

    let row = sqlx::query!("SELECT status, runner_id, runner_name FROM jobs WHERE id = 3002")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(row.status, "InProgress");
    assert_eq!(row.runner_id, Some(42i64));
    assert_eq!(row.runner_name.as_deref(), Some("runner-1"));
    shutdown.cancel();
}

/// Invalid transition Completed → InProgress → Err(InvalidTransition).
#[tokio::test]
#[serial_test::serial]
async fn pg_job_invalid_transition_returns_err() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(2003)).await.unwrap();
    store.apply_job_event(job_queued(3003, 2003)).await.unwrap();
    store
        .apply_job_event(job_in_progress(3003, 2003))
        .await
        .unwrap();
    store
        .apply_job_event(job_completed(3003, 2003))
        .await
        .unwrap();

    // Completed → InProgress is invalid
    let result = store.apply_job_event(job_in_progress(3003, 2003)).await;
    assert!(
        matches!(result, Err(PersistError::InvalidTransition)),
        "expected InvalidTransition, got: {result:?}"
    );

    let row = sqlx::query!("SELECT status FROM jobs WHERE id = 3003")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(row.status, "Completed");
    shutdown.cancel();
}

/// Same-status replay is idempotent.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_idempotent_same_status_replay() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(2004)).await.unwrap();
    store.apply_job_event(job_queued(3004, 2004)).await.unwrap();

    let result = store.apply_job_event(job_queued(3004, 2004)).await;
    assert!(
        result.is_ok(),
        "same-status replay should be Ok: {result:?}"
    );
    shutdown.cancel();
}

/// Queued → Completed is valid for jobs — GitHub sends this when a run is cancelled
/// before the job starts (no InProgress event is emitted in that case).
#[tokio::test]
#[serial_test::serial]
async fn pg_job_queued_to_completed_accepted() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(2005)).await.unwrap();
    store.apply_job_event(job_queued(3005, 2005)).await.unwrap();

    let result = store.apply_job_event(job_completed(3005, 2005)).await;
    assert!(
        result.is_ok(),
        "Queued→Completed must be accepted for jobs (GitHub cancellation path), got: {result:?}"
    );

    let row = sqlx::query!("SELECT status FROM jobs WHERE id = 3005")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(
        row.status, "Completed",
        "job must be Completed after transition"
    );

    // Confirm predecessors_of includes Queued
    let preds = JobStatus::predecessors_of(JobStatus::Completed);
    assert!(
        preds.contains(&JobStatus::Queued),
        "Queued must be a predecessor of Completed for jobs"
    );
    shutdown.cancel();
}

/// Job arrives before its run → stub run row created, job FK satisfied.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_before_run_creates_stub_run() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Fire job event for unknown run 9001
    let result = store.apply_job_event(job_queued(8001, 9001)).await;
    assert!(result.is_ok(), "job-before-run should succeed: {result:?}");

    // Stub run row must exist with status Queued
    let run_row = sqlx::query!("SELECT id, status, head_sha FROM runs WHERE id = 9001")
        .fetch_one(&pool)
        .await
        .expect("stub run row not found");
    assert_eq!(run_row.id, 9001i64);
    assert_eq!(run_row.status, "Queued");
    assert_eq!(run_row.head_sha, "", "stub head_sha is empty placeholder");

    // Job row exists with correct FK
    let job_row = sqlx::query!("SELECT id, run_id, status FROM jobs WHERE id = 8001")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(job_row.run_id, 9001i64);
    assert_eq!(job_row.status, "Queued");
    shutdown.cancel();
}

/// Real run event after job-before-run reconciles the stub.
#[tokio::test]
#[serial_test::serial]
async fn pg_real_run_event_reconciles_stub() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Job-before-run
    store.apply_job_event(job_queued(8002, 9002)).await.unwrap();

    // Now send the real run event
    let result = store.apply_run_event(run_requested(9002)).await;
    assert!(result.is_ok(), "reconciliation should succeed: {result:?}");

    // Stub fields should now be populated by the real run event
    let run_row = sqlx::query!(
        "SELECT status, head_sha, workflow_name, event, display_title, html_url FROM runs WHERE id = 9002"
    )
    .fetch_one(&pool)
    .await
    .expect("run row not found");

    // head_sha, event, display_title were '' in the stub; real run must overwrite them
    assert_eq!(run_row.head_sha, "abc123");
    assert_eq!(run_row.status, "Queued");
    assert_eq!(run_row.workflow_name.as_deref(), Some("CI"));
    assert_eq!(
        run_row.event, "push",
        "event must be overwritten from stub ''"
    );
    assert_eq!(
        run_row.display_title, "Test run",
        "display_title must be overwritten from stub ''"
    );
    assert_ne!(
        run_row.html_url, "",
        "html_url must be overwritten from stub ''"
    );

    // Job still has the right FK
    let job_row = sqlx::query!("SELECT run_id FROM jobs WHERE id = 8002")
        .fetch_one(&pool)
        .await
        .expect("job row not found");
    assert_eq!(job_row.run_id, 9002i64);
    shutdown.cancel();
}

/// Two job events for same unknown run → exactly one stub run row.
#[tokio::test]
#[serial_test::serial]
async fn pg_two_jobs_same_unknown_run_share_stub() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_job_event(job_queued(8003, 9003)).await.unwrap();
    store.apply_job_event(job_queued(8004, 9003)).await.unwrap();

    let run_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs WHERE id = 9003")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(run_count, 1, "should be exactly one stub run row");
    shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Field-merge parity with in-memory store (sticky COALESCE)
// ---------------------------------------------------------------------------

/// workflow_name set on first event, omitted on second → preserved.
#[tokio::test]
#[serial_test::serial]
async fn pg_run_coalesce_preserves_workflow_name() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // First event: workflow_name = Some("CI")
    store.apply_run_event(run_requested(4001)).await.unwrap();

    // Second event: workflow_name = None (in_progress omits it)
    store.apply_run_event(run_in_progress(4001)).await.unwrap();

    let row = sqlx::query!("SELECT workflow_name FROM runs WHERE id = 4001")
        .fetch_one(&pool)
        .await
        .expect("row not found");

    assert_eq!(
        row.workflow_name.as_deref(),
        Some("CI"),
        "workflow_name must be preserved by COALESCE"
    );
    shutdown.cancel();
}

/// runner_* fields set on InProgress, preserved through second InProgress with same runner.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_coalesce_preserves_runner() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(4002)).await.unwrap();
    store.apply_job_event(job_queued(5002, 4002)).await.unwrap();
    store
        .apply_job_event(job_in_progress(5002, 4002))
        .await
        .unwrap();

    // Second InProgress without runner (None) — should preserve from first
    let env_no_runner = JobEventEnvelope {
        job_id: JobId(5002),
        run_id: RunId(4002),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "test-job".to_string(),
        created_at: ts(),
        started_at: Some(ts()),
        completed_at: None,
        run_attempt: 1,
        action: JobEvent::InProgress {
            runner: None, // omit runner — should be preserved
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    };
    store.apply_job_event(env_no_runner).await.unwrap();

    let row = sqlx::query!("SELECT runner_id, runner_name FROM jobs WHERE id = 5002")
        .fetch_one(&pool)
        .await
        .expect("job row not found");

    assert_eq!(
        row.runner_id,
        Some(42i64),
        "runner_id must be preserved by COALESCE"
    );
    assert_eq!(
        row.runner_name.as_deref(),
        Some("runner-1"),
        "runner_name must be preserved by COALESCE"
    );
    shutdown.cancel();
}

/// When a new event carries a runner with null group fields, those fields are cleared —
/// they are NOT preserved from the previous event (runner is replaced as a unit, not merged).
#[tokio::test]
#[serial_test::serial]
async fn pg_job_runner_group_cleared_when_runner_changes() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(4010)).await.unwrap();
    store.apply_job_event(job_queued(5010, 4010)).await.unwrap();
    // First in_progress: runner with group_name="default"
    store
        .apply_job_event(job_in_progress(5010, 4010))
        .await
        .unwrap();

    // Second in_progress: same job, different runner — no group fields
    let env_new_runner = JobEventEnvelope {
        job_id: JobId(5010),
        run_id: RunId(4010),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "test-job".to_string(),
        created_at: ts(),
        started_at: Some(ts()),
        completed_at: None,
        run_attempt: 1,
        action: JobEvent::InProgress {
            runner: Some(atc_core::job::RunnerInfo {
                id: 99,
                name: "runner-2".to_string(),
                group_name: None,
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    };
    store.apply_job_event(env_new_runner).await.unwrap();

    let row =
        sqlx::query!("SELECT runner_id, runner_name, runner_group_name FROM jobs WHERE id = 5010")
            .fetch_one(&pool)
            .await
            .expect("job row not found");

    assert_eq!(
        row.runner_id,
        Some(99i64),
        "runner_id must reflect new runner"
    );
    assert_eq!(
        row.runner_name.as_deref(),
        Some("runner-2"),
        "runner_name must reflect new runner"
    );
    assert!(
        row.runner_group_name.is_none(),
        "runner_group_name must be cleared (was Some(\"default\"), new runner has None)"
    );
    shutdown.cancel();
}

/// name, run_id, created_at are identity fields — never overwritten by job updates.
#[tokio::test]
#[serial_test::serial]
async fn pg_job_coalesce_preserves_name_run_id_created_at() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store.apply_run_event(run_requested(4003)).await.unwrap();

    let env_first = job_queued(5003, 4003);
    let original_name = env_first.name.clone();
    let original_created_at = env_first.created_at;
    store.apply_job_event(env_first).await.unwrap();

    // Update: attempt to clobber name (can't via the API since name is String, not Option, but
    // the UPSERT's SET clause locks it to jobs.name regardless)
    let env_update = JobEventEnvelope {
        job_id: JobId(5003),
        run_id: RunId(4003),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "DIFFERENT-NAME".to_string(), // Envelope has a different name
        created_at: fixed_test_timestamp() + Duration::from_hours(1), // different created_at
        started_at: None,
        completed_at: None,
        run_attempt: 1,
        action: JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    };
    store.apply_job_event(env_update).await.unwrap();

    let row = sqlx::query!("SELECT name, run_id, created_at FROM jobs WHERE id = 5003")
        .fetch_one(&pool)
        .await
        .expect("job row not found");

    assert_eq!(
        row.name, original_name,
        "name must not be overwritten (identity field)"
    );
    assert_eq!(row.run_id, 4003i64, "run_id must not change");
    // created_at is locked to jobs.created_at in the UPSERT
    let stored_ts = row.created_at;
    let diff = (stored_ts - original_created_at).num_seconds().abs();
    assert!(
        diff < 2,
        "created_at must not be overwritten (diff={diff}s)"
    );
    shutdown.cancel();
}

/// PgStore::ping() succeeds against a healthy pool.
#[tokio::test]
#[serial_test::serial]
async fn pg_store_ping_succeeds() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;
    assert!(
        store.ping().await.is_ok(),
        "ping should succeed with a healthy pool"
    );
    shutdown.cancel();
}

/// The `0007_runs_completed_at.sql` migration backfills `completed_at` from
/// `updated_at` for any pre-existing `status='Completed'` row whose
/// `completed_at` is NULL. Migrations run once per database in
/// `common::start_pg()`, so we cannot literally re-apply them — but we can
/// seed a row that matches the pre-migration shape (NULL `completed_at`,
/// status='Completed') and run the same UPDATE statement to verify the
/// backfill clause is correct.
#[tokio::test]
#[serial_test::serial]
async fn migration_backfills_completed_at_from_updated_at_for_legacy_rows() {
    let (pool, _c, _db_url) = common::start_pg().await;
    let now = ts();

    // Seed a Completed row with completed_at = NULL (the pre-migration
    // shape — the column was added nullable and would be NULL for every
    // previously-completed row before the backfill ran).
    sqlx::query(
        r#"
        INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url,
                          status, conclusion, created_at, updated_at, completed_at,
                          placeholder)
        VALUES ($1, 'org', 'repo', 'sha', 'push', 'Test', 'http://x',
                'Completed', 'Success', $2, $2, NULL, false)
        "#,
    )
    .bind(9_000_001i64)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert legacy completed row");

    // Run the same backfill statement the migration uses.
    let result = sqlx::query(
        r#"
        UPDATE runs
           SET completed_at = updated_at
         WHERE status = 'Completed'
           AND completed_at IS NULL
        "#,
    )
    .execute(&pool)
    .await
    .expect("backfill update");
    assert!(
        result.rows_affected() >= 1,
        "backfill should touch at least our seeded row"
    );

    // Verify the seeded row's completed_at now matches its updated_at.
    let row: (Option<DateTime<Utc>>, DateTime<Utc>) =
        sqlx::query_as("SELECT completed_at, updated_at FROM runs WHERE id = $1")
            .bind(9_000_001i64)
            .fetch_one(&pool)
            .await
            .expect("re-fetch seeded row");
    assert_eq!(
        row.0,
        Some(row.1),
        "completed_at should equal updated_at after backfill"
    );
}
