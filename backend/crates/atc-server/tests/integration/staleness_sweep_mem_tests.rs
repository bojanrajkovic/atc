//! Integration tests for `InMemoryStore`'s staleness sweep (issue #439 /
//! ADR-0013).
//!
//! Covers the atomic recheck-then-write guarantee added in response to
//! Codex review on PR #445: `sweep_one_job_if_stale`/`sweep_one_run_if_stale`
//! recheck staleness and apply the synthetic `Stale` completion inside one
//! `state` write-lock acquisition, so a real completion can never be
//! silently clobbered — `Completed -> Completed` is an admitted idempotent
//! replay by design (needed for real webhook redelivery), not a rejected
//! transition, so a naive "recheck, then separately re-lock to apply" gap
//! would let a real `Success`/`Failure` conclusion lose a race against the
//! sweep's synthetic `Stale` one, with no self-heal.

use std::sync::Arc;
use std::time::Duration;

use atc_core::clock::TestClock;
use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use atc_core::test_support::{make_job_event, make_run_event};
use atc_core::types::{JobId, RunId};
use atc_core::{JobConclusion, fixed_test_timestamp};
use atc_persist::PersistentStore;
use atc_store_mem::InMemoryStore;

const THRESHOLD: Duration = Duration::from_secs(48 * 60 * 60);

/// A `Queued` job past the threshold is force-completed with conclusion
/// `Stale`.
#[tokio::test]
async fn stale_job_is_swept() {
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let store = InMemoryStore::new_for_test(clock.clone(), Duration::from_hours(1), 256);

    let run_id = RunId(9_600_001);
    let job_id = JobId(9_600_002);
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .expect("seed run");
    store
        .apply_job_event(JobEventEnvelope {
            created_at: now - chrono::Duration::hours(72),
            started_at: None,
            ..make_job_event(
                job_id,
                run_id,
                "test-org",
                "test-repo",
                JobEvent::Queued {
                    labels: vec!["ubuntu-latest".to_string()],
                    steps: vec![],
                },
            )
        })
        .await
        .expect("seed stale job");

    let mut rx = store.subscribe();
    store.sweep_stale(THRESHOLD).await;

    let job = store
        .get_job(&job_id)
        .await
        .expect("job should still exist");
    assert_eq!(job.status, atc_core::JobStatus::Completed);
    assert_eq!(job.conclusion, Some(JobConclusion::Stale));

    // Issue #475: `Job` carries no `repo_id` of its own (it's resolved
    // through the parent run), so the read-state assertions above can't see
    // this bug — only the broadcast envelope `WebhookEvent::repo_id()` reads
    // and the WS per-connection filter checks directly can.
    let event = rx.try_recv().expect("sweep should have broadcast an event");
    assert_eq!(
        event.event.repo_id(),
        Some(atc_core::types::RepoId(1_296_269))
    );
}

/// A job that already reached a real terminal conclusion *before*
/// `sweep_stale` is called is left untouched — the recheck (done under the
/// same lock as the write) observes `Completed` and skips it.
#[tokio::test]
async fn already_completed_job_is_not_reswept() {
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let store = InMemoryStore::new_for_test(clock.clone(), Duration::from_hours(1), 256);

    let run_id = RunId(9_600_101);
    let job_id = JobId(9_600_102);
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .expect("seed run");
    store
        .apply_job_event(JobEventEnvelope {
            created_at: now - chrono::Duration::hours(72),
            started_at: None,
            ..make_job_event(
                job_id,
                run_id,
                "test-org",
                "test-repo",
                JobEvent::Queued {
                    labels: vec!["ubuntu-latest".to_string()],
                    steps: vec![],
                },
            )
        })
        .await
        .expect("seed job");

    // A real completion lands before the sweep runs — this is what the
    // recheck must catch, whether it landed a millisecond ago or an hour
    // ago; the sweep has no way to distinguish the two, which is exactly
    // why the recheck must be atomic with the write rather than trusting a
    // stale candidate-list snapshot.
    store
        .apply_job_event(JobEventEnvelope {
            run_attempt: 1,
            ..make_job_event(
                job_id,
                run_id,
                "test-org",
                "test-repo",
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

    store.sweep_stale(THRESHOLD).await;

    let job = store
        .get_job(&job_id)
        .await
        .expect("job should still exist");
    assert_eq!(job.status, atc_core::JobStatus::Completed);
    assert_eq!(
        job.conclusion,
        Some(JobConclusion::Success),
        "the real conclusion must survive untouched — this is the exact case Codex \
         flagged: Completed -> Completed is an admitted idempotent replay, not a \
         rejected transition, so only an atomic recheck-and-write closes this race"
    );
}

/// A run with no jobs, past the threshold, is force-completed with
/// conclusion `Stale`.
#[tokio::test]
async fn stale_run_is_swept() {
    let now = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(now));
    let store = InMemoryStore::new_for_test(clock.clone(), Duration::from_hours(1), 256);

    let run_id = RunId(9_600_201);
    store
        .apply_run_event(RunEventEnvelope {
            updated_at: now - chrono::Duration::hours(72),
            ..make_run_event(run_id, RunEvent::Requested)
        })
        .await
        .expect("seed stale run");

    let mut rx = store.subscribe();
    store.sweep_stale(THRESHOLD).await;

    let run = store
        .get_run(&run_id)
        .await
        .expect("run should still exist");
    assert_eq!(run.status, atc_core::RunStatus::Completed);
    assert_eq!(run.conclusion, Some(atc_core::RunConclusion::Stale));

    // Issue #475 — same rationale as the job-sweep broadcast assertion
    // above.
    let event = rx.try_recv().expect("sweep should have broadcast an event");
    assert_eq!(
        event.event.repo_id(),
        Some(atc_core::types::RepoId(1_296_269))
    );
}
