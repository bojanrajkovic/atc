//! Behavioral tests for `InMemoryStore`.
//!
//! These tests mirror the coverage previously provided by
//! `atc-core/src/state_machine/tests/` (event_ingestion, edge_cases,
//! eviction, queries, webhook_domain_updates) and the property tests,
//! now expressed against the public `PersistentStore` trait plus the
//! `#[cfg(test)]` inspection helpers (`get_run`, `get_job`, `jobs_for_run`,
//! `jobs_for_repo`, `assert_invariants`).
//!
//! Tests are grouped by concern so coverage gaps are easy to spot.

use std::sync::Arc;
use std::time::Duration;

use atc_core::{
    JobConclusion, JobId, JobStatus, RunConclusion, RunId, RunStatus, RunnerInfo, SystemClock,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
    fixed_test_timestamp,
    job::{Step, StepStatus},
    types::RepoKey,
};
use atc_persist::PersistentStore;
use atc_store_mem::InMemoryStore;
use chrono::{DateTime, Utc};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_store() -> Arc<InMemoryStore> {
    InMemoryStore::new_for_test(Arc::new(SystemClock), Duration::from_hours(1), 256)
}

fn make_store_with_clock(clock: Arc<dyn atc_core::Clock>) -> Arc<InMemoryStore> {
    InMemoryStore::new_for_test(clock, Duration::from_hours(1), 256)
}

fn make_store_with_clock_and_ttl(
    clock: Arc<dyn atc_core::Clock>,
    ttl: Duration,
) -> Arc<InMemoryStore> {
    InMemoryStore::new_for_test(clock, ttl, 256)
}

fn make_run_event(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = fixed_test_timestamp();
    RunEventEnvelope {
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: Some("CI".to_string()),
        workflow_path: Some(".github/workflows/ci.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123def456".to_string(),
        commit_message: Some("Fix bug".to_string()),
        trigger_event: "push".to_string(),
        display_title: "CI Run".to_string(),
        html_url: "https://github.com/octocat/Hello-World/actions/runs/123".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        action,
    }
}

fn make_job_event(
    job_id: JobId,
    run_id: RunId,
    org: &str,
    repo: &str,
    action: JobEvent,
) -> JobEventEnvelope {
    let now = fixed_test_timestamp();
    JobEventEnvelope {
        job_id,
        run_id,
        org: org.to_string(),
        repo: repo.to_string(),
        name: "Test Job".to_string(),
        created_at: now,
        started_at: None,
        completed_at: None,
        action,
    }
}

fn make_job_event_with_completed_at(
    job_id: JobId,
    run_id: RunId,
    org: &str,
    repo: &str,
    action: JobEvent,
    completed_at: Option<DateTime<Utc>>,
) -> JobEventEnvelope {
    let now = fixed_test_timestamp();
    JobEventEnvelope {
        job_id,
        run_id,
        org: org.to_string(),
        repo: repo.to_string(),
        name: "Test Job".to_string(),
        created_at: now,
        started_at: None,
        completed_at,
        action,
    }
}

// ---------------------------------------------------------------------------
// Event ingestion — run events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_run_from_requested() {
    let store = make_store();
    let run_id = RunId(1);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.id, run_id);
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.org, "octocat");
    assert_eq!(run.repo, "Hello-World");
    assert_eq!(run.workflow_name, Some("CI".to_string()));
    assert_eq!(run.branch, Some("main".to_string()));
    assert_eq!(run.head_sha, "abc123def456");
    assert_eq!(run.commit_message, Some("Fix bug".to_string()));
    assert_eq!(run.event, "push");
    assert_eq!(run.display_title, "CI Run");
    assert_eq!(run.conclusion, None);
}

#[tokio::test]
async fn update_run_to_in_progress() {
    let store = make_store();
    let run_id = RunId(2);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(run_id, RunEvent::InProgress))
        .await
        .unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::InProgress);
    assert_eq!(run.conclusion, None);
}

#[tokio::test]
async fn complete_run_with_conclusion() {
    let store = make_store();
    let run_id = RunId(3);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(run_id, RunEvent::InProgress))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(
            run_id,
            RunEvent::Completed {
                conclusion: RunConclusion::Success,
            },
        ))
        .await
        .unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.conclusion, Some(RunConclusion::Success));
}

#[tokio::test]
async fn idempotent_run_requested_twice() {
    let store = make_store();
    let run_id = RunId(4);

    let envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(envelope.clone()).await.unwrap();
    store.apply_run_event(envelope).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Queued);
}

#[tokio::test]
async fn invalid_run_transition_completed_to_in_progress() {
    let store = make_store();
    let run_id = RunId(5);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(
            run_id,
            RunEvent::Completed {
                conclusion: RunConclusion::Failure,
            },
        ))
        .await
        .unwrap();

    // Completed → InProgress is invalid
    let result = store
        .apply_run_event(make_run_event(run_id, RunEvent::InProgress))
        .await;
    assert!(
        result.is_err(),
        "Completed→InProgress should return Err, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Event ingestion — job events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_job_from_queued() {
    let store = make_store();
    let job_id = JobId(100);
    let run_id = RunId(10);

    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.id, job_id);
    assert_eq!(job.run_id, run_id);
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.name, "Test Job");
    assert_eq!(job.conclusion, None);
    assert_eq!(job.runner, None);
    assert_eq!(job.labels, vec!["linux".to_string()]);
}

#[tokio::test]
async fn update_job_to_in_progress_with_runner() {
    let store = make_store();
    let job_id = JobId(101);
    let run_id = RunId(11);

    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::InProgress {
                runner: Some(runner.clone()),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::InProgress);
    assert_eq!(job.runner, Some(runner));
    assert_eq!(job.conclusion, None);
}

#[tokio::test]
async fn idempotent_job_queued_twice() {
    let store = make_store();
    let job_id = JobId(600);
    let run_id = RunId(60);

    let envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope.clone()).await.unwrap();
    store.apply_job_event(envelope).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Queued);
}

#[tokio::test]
async fn first_sight_completed_job() {
    let store = make_store();
    let job_id = JobId(500);
    let run_id = RunId(50);

    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.conclusion, Some(JobConclusion::Success));
}

#[tokio::test]
async fn steps_snapshot_replacement_not_append() {
    let store = make_store();
    let job_id = JobId(400);
    let run_id = RunId(40);

    let two_steps = vec![
        Step {
            number: 1,
            name: "Step A".to_string(),
            status: StepStatus::Queued,
            conclusion: None,
            started_at: None,
            completed_at: None,
        },
        Step {
            number: 2,
            name: "Step B".to_string(),
            status: StepStatus::Queued,
            conclusion: None,
            started_at: None,
            completed_at: None,
        },
    ];
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: two_steps,
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.steps.len(), 2);

    // Update with 3 steps — should replace, not append
    let three_steps = vec![
        Step {
            number: 1,
            name: "Step A".to_string(),
            status: StepStatus::InProgress,
            conclusion: None,
            started_at: Some(fixed_test_timestamp()),
            completed_at: None,
        },
        Step {
            number: 2,
            name: "Step B".to_string(),
            status: StepStatus::Queued,
            conclusion: None,
            started_at: None,
            completed_at: None,
        },
        Step {
            number: 3,
            name: "Step C".to_string(),
            status: StepStatus::Queued,
            conclusion: None,
            started_at: None,
            completed_at: None,
        },
    ];
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::InProgress {
                runner: None,
                labels: vec![],
                steps: three_steps,
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.steps.len(), 3, "steps should be replaced, not appended");
    assert_eq!(job.steps[0].name, "Step A");
    assert_eq!(job.steps[2].name, "Step C");
}

// ---------------------------------------------------------------------------
// Secondary indexes — jobs_by_run and jobs_by_repo
// ---------------------------------------------------------------------------

#[tokio::test]
async fn jobs_by_run_index_tracks_multiple_jobs() {
    let store = make_store();
    let run_id = RunId(20);
    let job_id_1 = JobId(201);
    let job_id_2 = JobId(202);

    store
        .apply_job_event(make_job_event(
            job_id_1,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            job_id_2,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let jobs = store.jobs_for_run(&run_id).await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.contains(&job_id_1));
    assert!(jobs.contains(&job_id_2));

    // Different run — separate index
    let run_id_2 = RunId(21);
    let job_id_3 = JobId(203);
    store
        .apply_job_event(make_job_event(
            job_id_3,
            run_id_2,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let jobs_run1 = store.jobs_for_run(&run_id).await;
    assert_eq!(jobs_run1.len(), 2);

    let jobs_run2 = store.jobs_for_run(&run_id_2).await;
    assert_eq!(jobs_run2.len(), 1);
    assert!(jobs_run2.contains(&job_id_3));
}

#[tokio::test]
async fn jobs_by_repo_index_separates_repos() {
    let store = make_store();
    let run_id = RunId(30);
    let job_id_alpha = JobId(301);
    let job_id_beta = JobId(302);

    store
        .apply_job_event(make_job_event(
            job_id_alpha,
            run_id,
            "octocat",
            "repo-a",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            job_id_beta,
            run_id,
            "octocat",
            "repo-b",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let repo_a = RepoKey::new("octocat", "repo-a");
    let repo_b = RepoKey::new("octocat", "repo-b");

    let jobs_a = store.jobs_for_repo(&repo_a).await;
    let jobs_b = store.jobs_for_repo(&repo_b).await;

    assert_eq!(jobs_a.len(), 1);
    assert!(jobs_a.contains(&job_id_alpha));
    assert_eq!(jobs_b.len(), 1);
    assert!(jobs_b.contains(&job_id_beta));
}

#[tokio::test]
async fn index_is_not_updated_on_duplicate_job_event() {
    // Sending the same job event twice must not double-insert in secondary indexes
    let store = make_store();
    let run_id = RunId(35);
    let job_id = JobId(351);

    let envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope.clone()).await.unwrap();
    store.apply_job_event(envelope).await.unwrap();

    // jobs_by_run should still only contain the job once
    let jobs = store.jobs_for_run(&run_id).await;
    assert_eq!(jobs.len(), 1, "duplicate insert must not double-count");

    store.assert_invariants().await;
}

// ---------------------------------------------------------------------------
// Out-of-order and edge-case delivery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn out_of_order_job_before_run() {
    let store = make_store();
    let run_id = RunId(1);
    let job_id = JobId(1);

    // Job arrives before its run
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    // Run arrives late
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Queued);

    // Indexes must be internally consistent
    store.assert_invariants().await;
}

#[tokio::test]
async fn out_of_order_completed_before_queued() {
    let store = make_store();
    let job_id = JobId(10);
    let run_id = RunId(2);

    // Completed arrives before Queued
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    // Queued would be a backward transition — expect error
    let result = store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await;
    assert!(
        result.is_err(),
        "Completed→Queued should be rejected, got {result:?}"
    );

    store.assert_invariants().await;
}

// ---------------------------------------------------------------------------
// Waiting variant
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_job_from_waiting() {
    let store = make_store();
    let job_id = JobId(702);
    let run_id = RunId(71);

    let step = Step {
        number: 1,
        name: "Checkout".to_string(),
        status: StepStatus::Queued,
        conclusion: None,
        started_at: None,
        completed_at: None,
    };

    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Waiting {
                labels: vec!["ubuntu-latest".to_string()],
                steps: vec![step],
            },
        ))
        .await
        .unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Waiting);
    assert_eq!(job.steps.len(), 1);

    store.assert_invariants().await;
}

#[tokio::test]
async fn workflow_name_preserved_on_in_progress_without_name() {
    // The `.or()` merge in `apply_run_event` must preserve workflow_name
    // when a later envelope arrives without it.
    let store = make_store();
    let run_id = RunId(1004);

    // First event sets the workflow_name
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    // InProgress with no workflow_name (common for this event type)
    let now = fixed_test_timestamp();
    let env = RunEventEnvelope {
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: None,
        workflow_path: None,
        branch: Some("main".to_string()),
        head_sha: "abc123def456".to_string(),
        commit_message: None,
        trigger_event: "push".to_string(),
        display_title: "CI Run".to_string(),
        html_url: "https://github.com/octocat/Hello-World/actions/runs/123".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        action: RunEvent::InProgress,
    };
    store.apply_run_event(env).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::InProgress);
    assert_eq!(
        run.workflow_name,
        Some("CI".to_string()),
        "workflow_name should be preserved from the Requested envelope"
    );
}

// ---------------------------------------------------------------------------
// Seq counter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn seq_increments_on_each_apply() {
    let store = make_store();
    assert_eq!(store.current_seq().await, 0);

    let seq1 = store
        .apply_run_event(make_run_event(RunId(1), RunEvent::Requested))
        .await
        .unwrap();
    assert_eq!(seq1, 1);
    assert_eq!(store.current_seq().await, 1);

    let seq2 = store
        .apply_job_event(make_job_event(
            JobId(1),
            RunId(1),
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    assert_eq!(seq2, 2);
    assert_eq!(store.current_seq().await, 2);
}

#[tokio::test]
async fn seq_not_incremented_on_invalid_transition() {
    let store = make_store();

    store
        .apply_run_event(make_run_event(RunId(99), RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(
            RunId(99),
            RunEvent::Completed {
                conclusion: RunConclusion::Failure,
            },
        ))
        .await
        .unwrap();
    let seq_before = store.current_seq().await;

    // This should fail (Completed → InProgress)
    let _ = store
        .apply_run_event(make_run_event(RunId(99), RunEvent::InProgress))
        .await;
    assert_eq!(
        store.current_seq().await,
        seq_before,
        "seq must not advance on rejected transition"
    );
}

// ---------------------------------------------------------------------------
// TTL eviction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn completed_job_within_ttl_retained() {
    use atc_core::clock::TestClock;
    use chrono::TimeDelta;

    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let store = make_store_with_clock(clock.clone());

    let run_id = RunId(1700);
    let job_id = JobId(1701);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let completed_at = start_time;
    store
        .apply_job_event(make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
            Some(completed_at),
        ))
        .await
        .unwrap();

    // Advance time by less than TTL (3600s), say 30 minutes
    clock.advance(TimeDelta::minutes(30));
    store.evict_expired().await;

    // Job is still within TTL — must be retained
    let job = store.get_job(&job_id).await;
    assert!(
        job.is_some(),
        "job completed 30m ago should be retained under 1h TTL"
    );
}

#[tokio::test]
async fn completed_job_past_ttl_evicted() {
    use atc_core::clock::TestClock;
    use chrono::TimeDelta;

    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let store = make_store_with_clock_and_ttl(clock.clone(), Duration::from_hours(1));

    let run_id = RunId(1800);
    let job_id = JobId(1801);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let completed_at = start_time;
    store
        .apply_job_event(make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
            Some(completed_at),
        ))
        .await
        .unwrap();

    // Advance past TTL
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    let job = store.get_job(&job_id).await;
    assert!(
        job.is_none(),
        "job completed 2h ago should be evicted under 1h TTL"
    );
}

#[tokio::test]
async fn run_evicted_when_all_jobs_evicted() {
    use atc_core::clock::TestClock;
    use chrono::TimeDelta;

    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let store = make_store_with_clock_and_ttl(clock.clone(), Duration::from_hours(1));

    let run_id = RunId(1900);
    let job_id = JobId(1901);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        ))
        .await
        .unwrap();

    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    assert!(
        store.get_run(&run_id).await.is_none(),
        "run should be evicted once all its jobs are gone"
    );
    store.assert_invariants().await;
}

#[tokio::test]
async fn active_job_not_evicted() {
    use atc_core::clock::TestClock;

    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let store = make_store_with_clock(clock.clone());

    let run_id = RunId(2000);
    let job_id = JobId(2001);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner: None,
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    clock.advance(chrono::TimeDelta::hours(48));
    store.evict_expired().await;

    assert!(
        store.get_job(&job_id).await.is_some(),
        "active job must never be evicted regardless of age"
    );
}

#[tokio::test]
async fn eviction_indexes_remain_consistent() {
    use atc_core::clock::TestClock;
    use chrono::TimeDelta;

    let start_time = fixed_test_timestamp();
    let clock = Arc::new(TestClock::new(start_time));
    let store = make_store_with_clock_and_ttl(clock.clone(), Duration::from_hours(1));

    let run_id = RunId(2100);
    let job_expired = JobId(2101);
    let job_active = JobId(2102);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    // One completed (will expire), one in-progress (must survive)
    store
        .apply_job_event(make_job_event_with_completed_at(
            job_expired,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        ))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            job_active,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner: None,
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    assert!(
        store.get_job(&job_expired).await.is_none(),
        "expired job must be evicted"
    );
    assert!(
        store.get_job(&job_active).await.is_some(),
        "active job must survive"
    );
    assert!(
        store.get_run(&run_id).await.is_some(),
        "run should survive because active job remains"
    );

    store.assert_invariants().await;
}

// ---------------------------------------------------------------------------
// Repository-scoped read_snapshot
// ---------------------------------------------------------------------------

#[tokio::test]
async fn read_snapshot_contains_all_runs_and_jobs() {
    let store = make_store();
    let run_id = RunId(700);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let job_id_alpha = JobId(701);
    let job_id_beta = JobId(702);

    store
        .apply_job_event(make_job_event(
            job_id_alpha,
            run_id,
            "org",
            "alpha",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            job_id_beta,
            run_id,
            "org",
            "beta",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let snapshot = store
        .read_snapshot()
        .await
        .expect("read_snapshot should succeed");

    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.jobs.len(), 2);
    assert_eq!(snapshot.last_seq, 3); // 1 run + 2 jobs
}

// ---------------------------------------------------------------------------
// read_snapshot_for_repos — repository-scoped filter
// ---------------------------------------------------------------------------
//
// `read_snapshot_for_repos` is the scoped variant used by the auth layer.
// The store filters by (org, repo) but must surface the same `last_seq`
// cursor as `read_snapshot` so clients with quiet accessible repos still
// reconcile against the live cursor rather than the max seq of matched rows.

/// An empty repo slice returns an empty snapshot with the current cursor.
#[tokio::test]
async fn read_snapshot_for_repos_empty_input_returns_empty_snapshot() {
    let store = make_store();
    let run_id = RunId(800);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            JobId(801),
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let snapshot = store
        .read_snapshot_for_repos(&[])
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert!(snapshot.runs.is_empty(), "runs must be empty");
    assert!(snapshot.jobs.is_empty(), "jobs must be empty");
    assert_eq!(
        snapshot.last_seq, 2,
        "last_seq must reflect the live cursor (1 run + 1 job), not the max seq of matched rows"
    );
}

/// A subset of repos returns only those entities; `last_seq` still reflects
/// the live cursor (which advanced past a non-matching repo's event).
#[tokio::test]
async fn read_snapshot_for_repos_subset_filters_to_listed_repos() {
    let store = make_store();
    let run_id_alpha = RunId(810);
    let run_id_beta = RunId(811);

    // Run that targets octocat/alpha
    let mut run_alpha = make_run_event(run_id_alpha, RunEvent::Requested);
    run_alpha.repo = "alpha".to_string();
    store.apply_run_event(run_alpha).await.unwrap();

    // Run that targets octocat/beta
    let mut run_beta = make_run_event(run_id_beta, RunEvent::Requested);
    run_beta.repo = "beta".to_string();
    store.apply_run_event(run_beta).await.unwrap();

    store
        .apply_job_event(make_job_event(
            JobId(820),
            run_id_alpha,
            "octocat",
            "alpha",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    store
        .apply_job_event(make_job_event(
            JobId(821),
            run_id_beta,
            "octocat",
            "beta",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let scope = vec![RepoKey::new("octocat", "alpha")];
    let snapshot = store
        .read_snapshot_for_repos(&scope)
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert_eq!(snapshot.runs.len(), 1, "only alpha's run should be visible");
    assert_eq!(snapshot.runs[0].id, run_id_alpha);
    assert_eq!(snapshot.jobs.len(), 1, "only alpha's job should be visible");
    assert_eq!(snapshot.jobs[0].id, JobId(820));
    // 2 runs + 2 jobs; cursor must reflect the live counter, including beta's
    // contribution.
    assert_eq!(
        snapshot.last_seq, 4,
        "last_seq must surface the live cursor even when matched rows are quiet"
    );
}

/// Repos that aren't in the store at all return an empty snapshot with the
/// current cursor.
#[tokio::test]
async fn read_snapshot_for_repos_non_existent_returns_empty_snapshot() {
    let store = make_store();
    let run_id = RunId(830);

    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();

    let scope = vec![RepoKey::new("ghost", "nowhere")];
    let snapshot = store
        .read_snapshot_for_repos(&scope)
        .await
        .expect("read_snapshot_for_repos should succeed");

    assert!(
        snapshot.runs.is_empty(),
        "no run matches the requested scope"
    );
    assert!(
        snapshot.jobs.is_empty(),
        "no job matches the requested scope"
    );
    assert_eq!(
        snapshot.last_seq, 1,
        "last_seq must reflect the live cursor"
    );
}

// ---------------------------------------------------------------------------
// Invariant checks across compound sequences
// ---------------------------------------------------------------------------

#[tokio::test]
async fn invariants_hold_after_mixed_event_sequence() {
    let store = make_store();

    // Multiple runs with multiple jobs, some out of order
    for run_id_n in 1i64..=3 {
        let run_id = RunId(run_id_n * 100);
        store
            .apply_run_event(make_run_event(run_id, RunEvent::Requested))
            .await
            .unwrap();

        for job_n in 1i64..=3 {
            let job_id = JobId(run_id_n * 100 + job_n);
            store
                .apply_job_event(make_job_event(
                    job_id,
                    run_id,
                    "org",
                    "repo",
                    JobEvent::Queued {
                        labels: vec![],
                        steps: vec![],
                    },
                ))
                .await
                .unwrap();
            store
                .apply_job_event(make_job_event(
                    job_id,
                    run_id,
                    "org",
                    "repo",
                    JobEvent::InProgress {
                        runner: None,
                        labels: vec![],
                        steps: vec![],
                    },
                ))
                .await
                .unwrap();
        }
    }

    store.assert_invariants().await;
}
