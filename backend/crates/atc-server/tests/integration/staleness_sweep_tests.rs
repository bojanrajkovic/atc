//! Integration tests for the PG staleness sweep (issue #439 / ADR-0013).
//!
//! Covers force-completion of stale jobs and runs, the non-terminal-jobs
//! shield on runs, the row-lock re-check against a real completion, and
//! multi-replica `SKIP LOCKED` dedup.
//!
//! Docker/OrbStack required (each test starts an ephemeral Postgres DB
//! through the shared test container).

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::clock::TestClock;
use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use atc_core::types::{JobId, RunId};
use atc_core::{JobConclusion, fixed_test_timestamp};
use atc_persist::PersistentStore;
use serial_test::serial;
use sqlx::Row;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

const THRESHOLD: Duration = Duration::from_secs(48 * 60 * 60);

fn job_envelope(
    job_id: i64,
    run_id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
) -> JobEventEnvelope {
    JobEventEnvelope {
        created_at,
        started_at: None,
        completed_at: None,
        ..common::make_job_envelope(
            JobId(job_id),
            RunId(run_id),
            JobEvent::Queued {
                labels: vec!["ubuntu-latest".to_string()],
                steps: vec![],
            },
        )
    }
}

fn run_envelope(run_id: i64, updated_at: chrono::DateTime<chrono::Utc>) -> RunEventEnvelope {
    RunEventEnvelope {
        updated_at,
        completed_at: None,
        ..common::make_run_envelope(RunId(run_id), RunEvent::Requested)
    }
}

/// A `Queued` job whose last activity predates the threshold is
/// force-completed with conclusion `Stale`, through the normal outbox/NOTIFY
/// path — a subscriber observes the broadcast.
#[tokio::test]
#[serial]
async fn stale_job_is_force_completed_and_broadcast() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_001;
    let job_id = 9_500_002;
    store
        .apply_run_event(run_envelope(run_id, now))
        .await
        .expect("seed run");
    store
        .apply_job_event(job_envelope(
            job_id,
            run_id,
            now - chrono::Duration::hours(72),
        ))
        .await
        .expect("seed stale job");

    // No clock advance needed: the job's `created_at` is already 72h before
    // `now`, past the 48h threshold. The run's `updated_at = now` is fresh
    // at the moment the sweep runs, so it stays out of the run pass.
    let mut rx = store.subscribe();
    let (jobs_swept, runs_swept) = store
        .staleness_sweep_once()
        .await
        .expect("staleness_sweep_once");
    assert_eq!(jobs_swept, 1, "the one stale job should be swept");
    assert_eq!(runs_swept, 0, "the run's own updated_at is fresh");

    let row = sqlx::query("SELECT status, conclusion FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .expect("query job");
    let status: String = row.get("status");
    let conclusion: Option<String> = row.get("conclusion");
    assert_eq!(status, "Completed");
    assert_eq!(conclusion.as_deref(), Some("Stale"));

    let event = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("broadcast did not arrive within 5s")
        .expect("broadcast channel closed");
    assert!(event.seq >= 1);

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// A non-terminal run with zero jobs and `updated_at` past the threshold is
/// force-completed with conclusion `Stale`.
#[tokio::test]
#[serial]
async fn stale_run_with_no_jobs_is_force_completed() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_101;
    store
        .apply_run_event(run_envelope(run_id, now - chrono::Duration::hours(72)))
        .await
        .expect("seed stale run");

    clock.advance(chrono::Duration::hours(49));
    let (jobs_swept, runs_swept) = store
        .staleness_sweep_once()
        .await
        .expect("staleness_sweep_once");
    assert_eq!(jobs_swept, 0);
    assert_eq!(runs_swept, 1, "the run with no jobs should be swept");

    let row = sqlx::query("SELECT status, conclusion FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("query run");
    let status: String = row.get("status");
    let conclusion: Option<String> = row.get("conclusion");
    assert_eq!(status, "Completed");
    assert_eq!(conclusion.as_deref(), Some("Stale"));

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// A run whose only job is still non-terminal (fresh activity) is shielded
/// from the sweep even though the run's own `updated_at` predates the
/// threshold — the `NOT EXISTS` non-terminal-jobs guard in the candidate
/// query, and the same re-check under the row lock, both hold.
#[tokio::test]
#[serial]
async fn run_with_live_job_is_shielded() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_201;
    let job_id = 9_500_202;
    store
        .apply_run_event(run_envelope(run_id, now - chrono::Duration::hours(72)))
        .await
        .expect("seed run");
    // Job's own activity stays within the threshold even after the clock
    // advances below (25h old at sweep time vs. the 48h threshold) — it
    // should not itself be swept, and it shields the run.
    store
        .apply_job_event(job_envelope(
            job_id,
            run_id,
            now + chrono::Duration::hours(24),
        ))
        .await
        .expect("seed live job");

    clock.advance(chrono::Duration::hours(49));
    let (jobs_swept, runs_swept) = store
        .staleness_sweep_once()
        .await
        .expect("staleness_sweep_once");
    assert_eq!(jobs_swept, 0, "the job's own activity is recent");
    assert_eq!(runs_swept, 0, "the run is shielded by its live job");

    let row = sqlx::query("SELECT status FROM runs WHERE id = $1")
        .bind(run_id)
        .fetch_one(&pool)
        .await
        .expect("query run");
    let status: String = row.get("status");
    assert_eq!(status, "Queued", "run should be untouched");

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// A job that already reached a real terminal conclusion before the sweep
/// runs is left untouched — the row-lock re-check observes `Completed` and
/// skips it, so a genuine `Success` is never clobbered by `Stale`.
#[tokio::test]
#[serial]
async fn already_completed_job_is_not_reswept() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_301;
    let job_id = 9_500_302;
    store
        .apply_run_event(run_envelope(run_id, now))
        .await
        .expect("seed run");
    store
        .apply_job_event(job_envelope(
            job_id,
            run_id,
            now - chrono::Duration::hours(72),
        ))
        .await
        .expect("seed job");
    // A real completion webhook lands before the sweep runs.
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 1,
            ..common::make_job_envelope(
                JobId(job_id),
                RunId(run_id),
                JobEvent::Completed {
                    conclusion: JobConclusion::Success,
                    runner: None,
                    labels: vec!["ubuntu-latest".to_string()],
                    steps: vec![],
                },
            )
        })
        .await
        .expect("real completion");

    clock.advance(chrono::Duration::hours(49));
    let (jobs_swept, _runs_swept) = store
        .staleness_sweep_once()
        .await
        .expect("staleness_sweep_once");
    assert_eq!(jobs_swept, 0, "an already-completed job is not a candidate");

    let row = sqlx::query("SELECT conclusion FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .expect("query job");
    let conclusion: Option<String> = row.get("conclusion");
    assert_eq!(
        conclusion.as_deref(),
        Some("Success"),
        "the real conclusion must survive untouched"
    );

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Self-heal: once the sweep has force-completed a job as `Stale`, a
/// subsequent real completion webhook overwrites the conclusion — the
/// `Completed -> Completed` replay is admitted and the incoming `Some`
/// conclusion always wins over the synthetic one.
#[tokio::test]
#[serial]
async fn real_completion_after_sweep_overwrites_stale_conclusion() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_401;
    let job_id = 9_500_402;
    store
        .apply_run_event(run_envelope(run_id, now))
        .await
        .expect("seed run");
    store
        .apply_job_event(job_envelope(
            job_id,
            run_id,
            now - chrono::Duration::hours(72),
        ))
        .await
        .expect("seed stale job");

    clock.advance(chrono::Duration::hours(49));
    let (jobs_swept, _) = store
        .staleness_sweep_once()
        .await
        .expect("staleness_sweep_once");
    assert_eq!(jobs_swept, 1);

    // The delayed real webhook finally lands.
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 1,
            ..common::make_job_envelope(
                JobId(job_id),
                RunId(run_id),
                JobEvent::Completed {
                    conclusion: JobConclusion::Success,
                    runner: None,
                    labels: vec!["ubuntu-latest".to_string()],
                    steps: vec![],
                },
            )
        })
        .await
        .expect("late real completion should be admitted (Completed -> Completed replay)");

    let row = sqlx::query("SELECT status, conclusion FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .expect("query job");
    let status: String = row.get("status");
    let conclusion: Option<String> = row.get("conclusion");
    assert_eq!(status, "Completed");
    assert_eq!(
        conclusion.as_deref(),
        Some("Success"),
        "the real conclusion overwrites the synthetic Stale one"
    );

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Two concurrent sweep ticks against the same stale job (simulating two
/// replicas) never both count it: `SELECT ... FOR UPDATE SKIP LOCKED` means
/// whichever transaction acquires the row lock first proceeds, and the
/// other's `fetch_optional` returns `None` immediately rather than blocking,
/// so exactly one call reports the sweep — no double-write, no double-count.
#[tokio::test]
#[serial]
async fn concurrent_sweeps_never_double_count_the_same_job() {
    let (pool, _container, db_url) = common::start_pg().await;
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test_with_clock_and_staleness(
        clock.clone(),
        pool.clone(),
        &db_url,
        shutdown.clone(),
        THRESHOLD,
    )
    .await;

    let run_id = 9_500_501;
    let job_id = 9_500_502;
    store
        .apply_run_event(run_envelope(run_id, now))
        .await
        .expect("seed run");
    store
        .apply_job_event(job_envelope(
            job_id,
            run_id,
            now - chrono::Duration::hours(72),
        ))
        .await
        .expect("seed stale job");

    clock.advance(chrono::Duration::hours(49));

    let (a, b) = tokio::join!(store.staleness_sweep_once(), store.staleness_sweep_once());
    let (jobs_a, _) = a.expect("sweep A");
    let (jobs_b, _) = b.expect("sweep B");
    assert_eq!(
        jobs_a + jobs_b,
        1,
        "exactly one of the two concurrent sweeps should have counted the job, got {jobs_a} + {jobs_b}"
    );

    let row = sqlx::query("SELECT status, conclusion FROM jobs WHERE id = $1")
        .bind(job_id)
        .fetch_one(&pool)
        .await
        .expect("query job");
    let status: String = row.get("status");
    let conclusion: Option<String> = row.get("conclusion");
    assert_eq!(status, "Completed");
    assert_eq!(conclusion.as_deref(), Some("Stale"));

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}
