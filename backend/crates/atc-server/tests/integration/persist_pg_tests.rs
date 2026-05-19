//! Integration tests for PgStore persistence against a live PostgreSQL container.
//!
//! Covers run-event durable write, job-event durable write including
//! job-before-run, and field-merge parity with in-memory store.
//!
//! Requires Docker (or OrbStack) to be running.

use crate::common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use atc_core::{
    JobStatus, PersistError,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
    fixed_test_timestamp,
    types::{JobId, RepoKey, RunId},
};
use atc_persist::PersistentStore;
use atc_store_pg::PgStore;
use chrono::{DateTime, Utc};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

fn ts() -> DateTime<Utc> {
    fixed_test_timestamp()
}

/// Minimal RunEventEnvelope for a Requested (Queued) event.
fn run_requested(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        workflow_name: Some("CI".to_string()),
        workflow_path: Some(".github/workflows/ci.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("Initial commit".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Test run".to_string(),
        html_url: format!("https://github.com/test-org/test-repo/actions/runs/{run_id}"),
        created_at: ts(),
        run_started_at: None,
        updated_at: ts(),
        action: RunEvent::Requested,
    }
}

/// InProgress run event.
fn run_in_progress(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        workflow_name: None, // deliberately omitted — should be preserved via COALESCE
        workflow_path: None,
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("Initial commit".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Test run".to_string(),
        html_url: format!("https://github.com/test-org/test-repo/actions/runs/{run_id}"),
        created_at: ts(),
        run_started_at: Some(ts()),
        updated_at: ts(),
        action: RunEvent::InProgress,
    }
}

/// Completed run event.
fn run_completed(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        workflow_name: None,
        workflow_path: None,
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("Initial commit".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Test run".to_string(),
        html_url: format!("https://github.com/test-org/test-repo/actions/runs/{run_id}"),
        created_at: ts(),
        run_started_at: Some(ts()),
        updated_at: ts(),
        action: RunEvent::Completed {
            conclusion: atc_core::RunConclusion::Success,
        },
    }
}

/// Minimal queued job envelope.
fn job_queued(job_id: i64, run_id: i64) -> JobEventEnvelope {
    JobEventEnvelope {
        job_id: JobId(job_id),
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "test-job".to_string(),
        created_at: ts(),
        started_at: None,
        completed_at: None,
        action: JobEvent::Queued {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    }
}

/// InProgress job envelope with runner info.
fn job_in_progress(job_id: i64, run_id: i64) -> JobEventEnvelope {
    JobEventEnvelope {
        job_id: JobId(job_id),
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "test-job".to_string(),
        created_at: ts(),
        started_at: Some(ts()),
        completed_at: None,
        action: JobEvent::InProgress {
            runner: Some(atc_core::job::RunnerInfo {
                id: 42,
                name: "runner-1".to_string(),
                group_id: Some(1),
                group_name: Some("default".to_string()),
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    }
}

/// Completed job envelope.
fn job_completed(job_id: i64, run_id: i64) -> JobEventEnvelope {
    JobEventEnvelope {
        job_id: JobId(job_id),
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        name: "test-job".to_string(),
        created_at: ts(),
        started_at: Some(ts()),
        completed_at: Some(ts()),
        action: JobEvent::Completed {
            conclusion: atc_core::JobConclusion::Success,
            runner: Some(atc_core::job::RunnerInfo {
                id: 42,
                name: "runner-1".to_string(),
                group_id: Some(1),
                group_name: Some("default".to_string()),
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    }
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
    // First in_progress: runner with group_id=1, group_name="default"
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
        action: JobEvent::InProgress {
            runner: Some(atc_core::job::RunnerInfo {
                id: 99,
                name: "runner-2".to_string(),
                group_id: None,
                group_name: None,
            }),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    };
    store.apply_job_event(env_new_runner).await.unwrap();

    let row = sqlx::query!(
        "SELECT runner_id, runner_name, runner_group_id, runner_group_name FROM jobs WHERE id = 5010"
    )
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
        row.runner_group_id.is_none(),
        "runner_group_id must be cleared (was Some(1), new runner has None)"
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

// ---------------------------------------------------------------------------
// read_snapshot_for_repos — repository-scoped reads
// ---------------------------------------------------------------------------
//
// The scoped variant filters runs/jobs by (org, repo) but must surface the
// same broadcast-watermark cursor as `read_snapshot` so a caller whose
// accessible repos are quiet still reconciles against the live cursor.

/// Run-event envelope targeting a specific (org, repo).
fn run_requested_in(run_id: i64, org: &str, repo: &str) -> RunEventEnvelope {
    let mut env = run_requested(run_id);
    env.org = org.to_string();
    env.repo = repo.to_string();
    env.html_url = format!("https://github.com/{org}/{repo}/actions/runs/{run_id}");
    env
}

/// Job envelope targeting a specific (org, repo).
fn job_queued_in(job_id: i64, run_id: i64, org: &str, repo: &str) -> JobEventEnvelope {
    let mut env = job_queued(job_id, run_id);
    env.org = org.to_string();
    env.repo = repo.to_string();
    env
}

/// Block until the drain has advanced the broadcast watermark to `target`.
/// Mirrors the wait pattern in `state_pg_read.rs` so scoped-read tests can
/// assert against a known-current cursor.
async fn wait_for_watermark(store: &PgStore, target: i64) {
    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if store.broadcast_watermark().load(Ordering::Acquire) >= target {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("drain did not advance broadcast_watermark to {target} within 5s"));
}

/// Empty `repos` slice → empty snapshot; `last_seq` still surfaces the live
/// cursor advanced by the drain.
#[tokio::test]
#[serial_test::serial]
async fn pg_read_snapshot_for_repos_empty_input_returns_empty_snapshot() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store
        .apply_run_event(run_requested_in(7001, "octocat", "alpha"))
        .await
        .unwrap();
    store
        .apply_job_event(job_queued_in(8001, 7001, "octocat", "alpha"))
        .await
        .unwrap();
    wait_for_watermark(&store, 2).await;

    let snap = store
        .read_snapshot_for_repos(&[])
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert!(snap.runs.is_empty(), "runs must be empty");
    assert!(snap.jobs.is_empty(), "jobs must be empty");
    assert_eq!(
        snap.last_seq, 2,
        "last_seq must reflect the live cursor, not 0"
    );

    shutdown.cancel();
}

/// Subset of repos returns only those entities; the live cursor is preserved
/// even when it advanced past a non-matching repo's event.
#[tokio::test]
#[serial_test::serial]
async fn pg_read_snapshot_for_repos_subset_filters_to_listed_repos() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Two runs: one in octocat/alpha, one in octocat/beta.
    store
        .apply_run_event(run_requested_in(7100, "octocat", "alpha"))
        .await
        .unwrap();
    store
        .apply_run_event(run_requested_in(7101, "octocat", "beta"))
        .await
        .unwrap();
    store
        .apply_job_event(job_queued_in(8100, 7100, "octocat", "alpha"))
        .await
        .unwrap();
    store
        .apply_job_event(job_queued_in(8101, 7101, "octocat", "beta"))
        .await
        .unwrap();
    wait_for_watermark(&store, 4).await;

    let scope = vec![RepoKey::new("octocat", "alpha")];
    let snap = store
        .read_snapshot_for_repos(&scope)
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert_eq!(snap.runs.len(), 1, "only alpha's run should be visible");
    assert_eq!(snap.runs[0].id.0, 7100);
    assert_eq!(snap.runs[0].repo, "alpha");
    assert_eq!(snap.jobs.len(), 1, "only alpha's job should be visible");
    assert_eq!(snap.jobs[0].id.0, 8100);
    assert_eq!(snap.jobs[0].run_id.0, 7100);
    assert_eq!(
        snap.last_seq, 4,
        "last_seq must reflect the live cursor even when matched rows are quiet"
    );

    shutdown.cancel();
}

/// Scope referencing repos that do not exist in the store returns an empty
/// snapshot; the live cursor is preserved.
#[tokio::test]
#[serial_test::serial]
async fn pg_read_snapshot_for_repos_non_existent_returns_empty_snapshot() {
    let (pool, _c, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    store
        .apply_run_event(run_requested_in(7200, "octocat", "alpha"))
        .await
        .unwrap();
    wait_for_watermark(&store, 1).await;

    let scope = vec![RepoKey::new("ghost", "nowhere")];
    let snap = store
        .read_snapshot_for_repos(&scope)
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert!(snap.runs.is_empty(), "no run matches the requested scope");
    assert!(snap.jobs.is_empty(), "no job matches the requested scope");
    assert_eq!(snap.last_seq, 1, "last_seq must reflect the live cursor");

    shutdown.cancel();
}
