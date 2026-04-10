
use super::*;
use crate::clock::TestClock;
use crate::job::JobConclusion;
use crate::run::RunConclusion;
use chrono::{DateTime, Utc};

/// Helper to build a RunEventEnvelope with sensible defaults.
fn make_run_event(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = Utc::now();
    RunEventEnvelope {
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: "CI".to_string(),
        workflow_path: ".github/workflows/ci.yml".to_string(),
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

/// Helper to build a JobEventEnvelope with sensible defaults.
fn make_job_event(
    job_id: JobId,
    run_id: RunId,
    org: &str,
    repo: &str,
    action: JobEvent,
) -> JobEventEnvelope {
    let now = Utc::now();
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

/// Helper to build a JobEventEnvelope with custom completed_at timestamp.
fn make_job_event_with_completed_at(
    job_id: JobId,
    run_id: RunId,
    org: &str,
    repo: &str,
    action: JobEvent,
    completed_at: Option<DateTime<Utc>>,
) -> JobEventEnvelope {
    let now = Utc::now();
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

#[tokio::test]
async fn test_ac3_1_create_run_from_requested() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(1);

    let envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(envelope).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.id, run_id);
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.org, "octocat");
    assert_eq!(run.repo, "Hello-World");
    assert_eq!(run.workflow_name, "CI");
    assert_eq!(run.branch, Some("main".to_string()));
    assert_eq!(run.head_sha, "abc123def456");
    assert_eq!(run.commit_message, Some("Fix bug".to_string()));
    assert_eq!(run.event, "push");
    assert_eq!(run.display_title, "CI Run");
    assert_eq!(run.conclusion, None);
}

#[tokio::test]
async fn test_ac3_1_update_run_to_in_progress() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(2);

    // Create with Requested
    let envelope1 = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(envelope1).await.unwrap();

    // Update to InProgress
    let envelope2 = make_run_event(run_id, RunEvent::InProgress);
    store.apply_run_event(envelope2).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::InProgress);
    assert_eq!(run.conclusion, None);
}

#[tokio::test]
async fn test_ac3_1_complete_run_with_conclusion() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(3);

    // Requested
    let envelope1 = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(envelope1).await.unwrap();

    // InProgress
    let envelope2 = make_run_event(run_id, RunEvent::InProgress);
    store.apply_run_event(envelope2).await.unwrap();

    // Completed
    let envelope3 = make_run_event(
        run_id,
        RunEvent::Completed {
            conclusion: RunConclusion::Success,
        },
    );
    store.apply_run_event(envelope3).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.conclusion, Some(RunConclusion::Success));
}

#[tokio::test]
async fn test_ac3_6_idempotent_requested_twice() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(4);

    let envelope = make_run_event(run_id, RunEvent::Requested);

    // Send Requested twice
    store.apply_run_event(envelope.clone()).await.unwrap();
    store.apply_run_event(envelope).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.id, run_id);
}

// ===== Job Event Tests (Task 3) =====

#[tokio::test]
async fn test_ac3_2_create_job_from_queued() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let job_id = JobId(100);
    let run_id = RunId(10);

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
    store.apply_job_event(envelope).await.unwrap();

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
async fn test_ac3_2_update_job_to_in_progress() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let job_id = JobId(101);
    let run_id = RunId(11);

    // Create with Queued
    let envelope1 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope1).await.unwrap();

    // Update to InProgress with runner
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let envelope2 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::InProgress {
            runner: runner.clone(),
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope2).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::InProgress);
    assert_eq!(job.runner, Some(runner));
    assert_eq!(job.conclusion, None);
}

#[tokio::test]
async fn test_ac3_3_jobs_by_run() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(20);
    let job_id_1 = JobId(201);
    let job_id_2 = JobId(202);

    // Create two jobs for the same run
    let envelope1 = make_job_event(
        job_id_1,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope1).await.unwrap();

    let envelope2 = make_job_event(
        job_id_2,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope2).await.unwrap();

    let jobs = store.jobs_for_run(&run_id).await;
    assert_eq!(jobs.len(), 2);
    assert!(jobs.contains(&job_id_1));
    assert!(jobs.contains(&job_id_2));

    // Create a job for a different run
    let run_id_2 = RunId(21);
    let job_id_3 = JobId(203);
    let envelope3 = make_job_event(
        job_id_3,
        run_id_2,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope3).await.unwrap();

    // Verify first run still only has 2 jobs
    let jobs_run1 = store.jobs_for_run(&run_id).await;
    assert_eq!(jobs_run1.len(), 2);

    // Verify second run has 1 job
    let jobs_run2 = store.jobs_for_run(&run_id_2).await;
    assert_eq!(jobs_run2.len(), 1);
    assert!(jobs_run2.contains(&job_id_3));
}

#[tokio::test]
async fn test_ac3_3_jobs_by_repo() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let run_id = RunId(30);

    // Create jobs for different repos
    let job_id_1 = JobId(301);
    let repo_a_key = RepoKey::new("octocat", "repo-a");
    let envelope1 = make_job_event(
        job_id_1,
        run_id,
        "octocat",
        "repo-a",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope1).await.unwrap();

    let job_id_2 = JobId(302);
    let repo_b_key = RepoKey::new("octocat", "repo-b");
    let envelope2 = make_job_event(
        job_id_2,
        run_id,
        "octocat",
        "repo-b",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope2).await.unwrap();

    // Verify jobs are separated by repo
    let jobs_a = store.jobs_for_repo(&repo_a_key).await;
    let jobs_b = store.jobs_for_repo(&repo_b_key).await;

    assert_eq!(jobs_a.len(), 1);
    assert!(jobs_a.contains(&job_id_1));

    assert_eq!(jobs_b.len(), 1);
    assert!(jobs_b.contains(&job_id_2));
}

#[tokio::test]
async fn test_ac3_4_steps_snapshot_replacement() {
    use crate::job::{Step, StepStatus};

    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let job_id = JobId(400);
    let run_id = RunId(40);

    // Create with 2 steps
    let steps_1 = vec![
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

    let envelope1 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: steps_1,
        },
    );
    store.apply_job_event(envelope1).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.steps.len(), 2);

    // Update to InProgress with 3 steps (should replace, not append)
    let steps_2 = vec![
        Step {
            number: 1,
            name: "Step A".to_string(),
            status: StepStatus::InProgress,
            conclusion: None,
            started_at: Some(start_time),
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

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    let envelope2 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::InProgress {
            runner,
            labels: vec![],
            steps: steps_2,
        },
    );
    store.apply_job_event(envelope2).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.steps.len(), 3); // Should be 3, not 5 (replaced, not appended)
    assert_eq!(job.steps[0].name, "Step A");
    assert_eq!(job.steps[1].name, "Step B");
    assert_eq!(job.steps[2].name, "Step C");
}

#[tokio::test]
async fn test_ac3_5_first_sight_completed_job() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let job_id = JobId(500);
    let run_id = RunId(50);

    // Send a Completed event for an unknown job (out-of-order delivery)
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    let envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.conclusion, Some(JobConclusion::Success));
}

#[tokio::test]
async fn test_ac3_6_idempotent_queued_twice() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
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

    // Send Queued twice
    store.apply_job_event(envelope.clone()).await.unwrap();
    store.apply_job_event(envelope).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Queued);
    assert_eq!(job.id, job_id);
}

// ===== Task 1: Repository-Scoped Queries =====

#[tokio::test]
async fn test_ac4_1_query_returns_only_queried_repos() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(700);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create jobs in two different repos
    let job_id_alpha = JobId(701);
    let job_id_beta = JobId(702);

    let alpha_envelope = make_job_event(
        job_id_alpha,
        run_id,
        "org",
        "alpha",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(alpha_envelope).await.unwrap();

    let beta_envelope = make_job_event(
        job_id_beta,
        run_id,
        "org",
        "beta",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(beta_envelope).await.unwrap();

    // Query only alpha repo
    let alpha_repo = RepoKey::new("org", "alpha");
    let result = store.query_by_repos(&[alpha_repo]).await;

    // Verify only alpha's job is returned
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.jobs[0].id, job_id_alpha);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_ac4_2_query_returns_owned_snapshots() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    // Create a run and job
    let run_id = RunId(800);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let job_id = JobId(801);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query and hold the result
    let repo = RepoKey::new("org", "repo");
    let result = store.query_by_repos(&[repo]).await;

    // Verify we can use the result without holding the store lock
    // (This is implicitly tested by the API returning owned types,
    // but we verify the data is present and usable)
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.jobs[0].id, job_id);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_ac4_5_empty_repos_returns_empty_result() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    // Create a run and job
    let run_id = RunId(900);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let job_id = JobId(901);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query with empty repos slice
    let result = store.query_by_repos(&[]).await;

    // Verify empty result
    assert_eq!(result.jobs.len(), 0);
    assert_eq!(result.runs.len(), 0);
}

#[tokio::test]
async fn test_ac4_6_multi_repo_query_isolation() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(1000);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create jobs in repo A
    let job_a1 = JobId(1001);
    let job_a2 = JobId(1002);

    let envelope_a1 = make_job_event(
        job_a1,
        run_id,
        "org",
        "repoA",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_a1).await.unwrap();

    let envelope_a2 = make_job_event(
        job_a2,
        run_id,
        "org",
        "repoA",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_a2).await.unwrap();

    // Create jobs in repo B
    let job_b1 = JobId(1003);
    let job_b2 = JobId(1004);

    let envelope_b1 = make_job_event(
        job_b1,
        run_id,
        "org",
        "repoB",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_b1).await.unwrap();

    let envelope_b2 = make_job_event(
        job_b2,
        run_id,
        "org",
        "repoB",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_b2).await.unwrap();

    // Query both repos
    let repo_a = RepoKey::new("org", "repoA");
    let repo_b = RepoKey::new("org", "repoB");
    let result = store
        .query_by_repos(&[repo_a.clone(), repo_b.clone()])
        .await;

    // Verify both repos' jobs are returned (4 jobs total)
    assert_eq!(result.jobs.len(), 4);

    // Verify all expected job IDs are present
    let job_ids: HashSet<JobId> = result.jobs.iter().map(|job| job.id).collect();
    assert!(job_ids.contains(&job_a1), "Job A1 should be in result");
    assert!(job_ids.contains(&job_a2), "Job A2 should be in result");
    assert!(job_ids.contains(&job_b1), "Job B1 should be in result");
    assert!(job_ids.contains(&job_b2), "Job B2 should be in result");

    // Verify repo A query alone returns only repo A's jobs
    let result_a = store.query_by_repos(&[repo_a]).await;
    assert_eq!(result_a.jobs.len(), 2);
    let job_ids_a: HashSet<JobId> = result_a.jobs.iter().map(|job| job.id).collect();
    assert!(job_ids_a.contains(&job_a1));
    assert!(job_ids_a.contains(&job_a2));
    assert!(!job_ids_a.contains(&job_b1));
    assert!(!job_ids_a.contains(&job_b2));

    // Verify repo B query alone returns only repo B's jobs
    let result_b = store.query_by_repos(&[repo_b]).await;
    assert_eq!(result_b.jobs.len(), 2);
    let job_ids_b: HashSet<JobId> = result_b.jobs.iter().map(|job| job.id).collect();
    assert!(!job_ids_b.contains(&job_a1));
    assert!(!job_ids_b.contains(&job_a2));
    assert!(job_ids_b.contains(&job_b1));
    assert!(job_ids_b.contains(&job_b2));

    // Verify run is included
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.runs[0].id, run_id);
}

#[tokio::test]
async fn test_ac4_query_includes_parent_runs() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    // Create a run
    let run_id = RunId(1100);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Add a job to the run
    let job_id = JobId(1101);
    let job_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Query by repo
    let repo = RepoKey::new("org", "repo");
    let result = store.query_by_repos(&[repo]).await;

    // Verify both job and run are present
    assert_eq!(result.jobs.len(), 1);
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.jobs[0].run_id, run_id);
    assert_eq!(result.runs[0].id, run_id);
}

// ===== Task 2: Runner Pool Stats Derivation =====

#[tokio::test]
async fn test_ac4_3_basic_pool_counts() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1200);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create 2 queued jobs with labels ["linux", "self-hosted"]
    let labels = vec!["linux".to_string(), "self-hosted".to_string()];
    let job_id_1 = JobId(1201);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    let job_id_2 = JobId(1202);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Create 1 running job with same labels
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_id_3 = JobId(1203);
    let envelope_3 = make_job_event(
        job_id_3,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner,
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_3).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify one entry with correct counts
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 2);
    assert_eq!(stats[0].running, 1);
}

#[tokio::test]
async fn test_ac4_3_multiple_pools() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1300);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create queued jobs with labels ["linux"]
    let job_id_1 = JobId(1301);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create queued jobs with labels ["macos"]
    let job_id_2 = JobId(1302);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["macos".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify two entries
    assert_eq!(stats.len(), 2);
    let mut counts: Vec<(usize, usize)> = stats.iter().map(|s| (s.queued, s.running)).collect();
    counts.sort();
    assert_eq!(counts, vec![(1, 0), (1, 0)]);
}

#[tokio::test]
async fn test_ac4_3_label_normalization() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1400);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create job with ["self-hosted", "linux"]
    let job_id_1 = JobId(1401);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["self-hosted".to_string(), "linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create job with ["linux", "self-hosted"] (different order)
    let job_id_2 = JobId(1402);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string(), "self-hosted".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify single entry (same label set regardless of order)
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 2);
    assert_eq!(stats[0].running, 0);
}

#[tokio::test]
async fn test_ac4_3_excludes_completed() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1500);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    let labels = vec!["linux".to_string()];

    // Create a queued job
    let job_id_1 = JobId(1501);
    let envelope_1 = make_job_event(
        job_id_1,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_1).await.unwrap();

    // Create a completed job
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_id_2 = JobId(1502);
    let envelope_2 = make_job_event(
        job_id_2,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: labels.clone(),
            steps: vec![],
        },
    );
    store.apply_job_event(envelope_2).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify completed job is not counted
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].queued, 1);
    assert_eq!(stats[0].running, 0);
}

#[tokio::test]
async fn test_ac4_4_group_name_from_runner_info() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1600);
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create a running job with group_name
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: Some(42),
        group_name: Some("default".to_string()),
    };
    let job_id = JobId(1601);
    let labels = vec!["linux".to_string()];
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner,
            labels,
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    // Get pool stats
    let stats = store.pool_stats().await;

    // Verify group_name is captured
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].group_name, Some("default".to_string()));
}

// ===== Task 1: TTL Eviction =====

#[tokio::test]
async fn test_ac5_1_completed_job_within_ttl_retained() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1700);
    let job_id = JobId(1701);

    // Create and complete a job at t0
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock to t0 + 30 minutes (within 1-hour TTL)
    clock.advance(TimeDelta::minutes(30));

    // Call evict_expired()
    store.evict_expired().await;

    // Verify job still exists
    let job = store.get_job(&job_id).await;
    assert!(job.is_some(), "Job should be retained within TTL");
}

#[tokio::test]
async fn test_ac5_2_completed_job_past_ttl_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1800);
    let job_id = JobId(1801);

    // Create and complete a job at t0
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock to t0 + 2 hours (past 1-hour TTL)
    clock.advance(TimeDelta::hours(2));

    // Call evict_expired()
    store.evict_expired().await;

    // Verify job is removed from primary map
    let job = store.get_job(&job_id).await;
    assert!(
        job.is_none(),
        "Completed job past TTL should be evicted from primary map"
    );

    // Verify job is removed from jobs_for_run
    let jobs = store.jobs_for_run(&run_id).await;
    assert!(
        !jobs.contains(&job_id),
        "Evicted job should not be in jobs_for_run"
    );

    // Verify job is removed from jobs_for_repo
    let repo = RepoKey::new("org", "repo");
    let jobs = store.jobs_for_repo(&repo).await;
    assert!(
        !jobs.contains(&job_id),
        "Evicted job should not be in jobs_for_repo"
    );
}

#[tokio::test]
async fn test_ac5_3_run_with_no_jobs_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(1900);
    let job_id = JobId(1901);

    // Create a run with one job
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope = make_job_event_with_completed_at(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Advance clock past TTL and evict
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify both job and run are evicted
    let job = store.get_job(&job_id).await;
    assert!(job.is_none(), "Job should be evicted");
    let run = store.get_run(&run_id).await;
    assert!(
        run.is_none(),
        "Run with no remaining jobs should be evicted"
    );
}

#[tokio::test]
async fn test_ac5_3_run_with_active_job_retained() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(2000);
    let completed_job_id = JobId(2001);
    let active_job_id = JobId(2002);

    // Create a run with two jobs
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Complete one job
    let completed_envelope = make_job_event_with_completed_at(
        completed_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner.clone()),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store.apply_job_event(completed_envelope).await.unwrap();

    // Create an active (running) job
    let active_envelope = make_job_event(
        active_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner,
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(active_envelope).await.unwrap();

    // Advance clock past TTL and evict
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify completed job is evicted but run and active job remain
    let completed_job = store.get_job(&completed_job_id).await;
    assert!(
        completed_job.is_none(),
        "Completed job past TTL should be evicted"
    );

    let active_job = store.get_job(&active_job_id).await;
    assert!(active_job.is_some(), "Active job should be retained");

    let run = store.get_run(&run_id).await;
    assert!(
        run.is_some(),
        "Run should be retained because it still has an active job"
    );
}

#[tokio::test]
async fn test_ac5_4_active_jobs_never_evicted() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(2100);
    let queued_job_id = JobId(2101);
    let running_job_id = JobId(2102);
    let waiting_job_id = JobId(2103);

    // Create a run
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Create queued job
    let queued_envelope = make_job_event(
        queued_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(queued_envelope).await.unwrap();

    // Create running job
    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let running_envelope = make_job_event(
        running_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner,
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(running_envelope).await.unwrap();

    // Create waiting job
    let waiting_envelope = make_job_event(
        waiting_job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    );
    store.apply_job_event(waiting_envelope).await.unwrap();

    // Advance clock well past TTL and evict
    clock.advance(TimeDelta::days(10));
    store.evict_expired().await;

    // Verify all active jobs are retained
    let queued_job = store.get_job(&queued_job_id).await;
    assert!(queued_job.is_some(), "Queued job should never be evicted");

    let running_job = store.get_job(&running_job_id).await;
    assert!(running_job.is_some(), "Running job should never be evicted");

    let waiting_job = store.get_job(&waiting_job_id).await;
    assert!(waiting_job.is_some(), "Waiting job should never be evicted");
}

#[tokio::test]
async fn test_ac5_5_ttl_configurable() {
    let start_time = Utc::now();
    let clock_1h = Arc::new(TestClock::new(start_time));
    let clock_5m = Arc::new(TestClock::new(start_time));

    // Store with 1-hour TTL
    let store_1h = StateStore::new(clock_1h.clone(), Duration::from_secs(3600));
    // Store with 5-minute TTL
    let store_5m = StateStore::new(clock_5m.clone(), Duration::from_secs(300));

    let run_id_1h = RunId(2200);
    let job_id_1h = JobId(2201);
    let run_id_5m = RunId(2300);
    let job_id_5m = JobId(2301);

    // Setup store with 1-hour TTL
    let run_envelope_1h = make_run_event(run_id_1h, RunEvent::Requested);
    store_1h.apply_run_event(run_envelope_1h).await.unwrap();

    use crate::job::RunnerInfo;
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let job_envelope_1h = make_job_event_with_completed_at(
        job_id_1h,
        run_id_1h,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner.clone()),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store_1h.apply_job_event(job_envelope_1h).await.unwrap();

    // Setup store with 5-minute TTL
    let run_envelope_5m = make_run_event(run_id_5m, RunEvent::Requested);
    store_5m.apply_run_event(run_envelope_5m).await.unwrap();

    let job_envelope_5m = make_job_event_with_completed_at(
        job_id_5m,
        run_id_5m,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec![],
            steps: vec![],
        },
        Some(start_time),
    );
    store_5m.apply_job_event(job_envelope_5m).await.unwrap();

    // Advance both clocks to t0 + 30 minutes
    clock_1h.advance(TimeDelta::minutes(30));
    clock_5m.advance(TimeDelta::minutes(30));

    // Evict from both stores
    store_1h.evict_expired().await;
    store_5m.evict_expired().await;

    // Verify: 1-hour store retains job, 5-minute store evicts it
    let job_1h = store_1h.get_job(&job_id_1h).await;
    assert!(job_1h.is_some(), "Job in 1-hour store should be retained");

    let job_5m = store_5m.get_job(&job_id_5m).await;
    assert!(job_5m.is_none(), "Job in 5-minute store should be evicted");
}

// Edge case tests for AC6.5 — out-of-order, duplicate, and unknown-ID events
#[tokio::test]
async fn test_ac6_5_out_of_order_job_before_run() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(1);
    let job_id = JobId(1);

    // Send JobEvent::Completed before RunEvent::Requested
    let job_envelope = make_job_event(
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
    );
    store.apply_job_event(job_envelope).await.unwrap();

    // Then send RunEvent::Requested
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Verify job exists in completed state
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    // Verify run exists in queued state
    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.status, RunStatus::Queued);

    // Verify indexes are consistent
    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_out_of_order_completed_before_queued() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(2);
    let job_id = JobId(2);

    // First send JobEvent::Completed
    let completed_envelope = make_job_event(
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
    );
    store.apply_job_event(completed_envelope).await.unwrap();

    // Then try to send JobEvent::Queued for same job
    let queued_envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    let result = store.apply_job_event(queued_envelope).await;

    // Second event should return error (backward transition)
    assert!(result.is_err(), "Backward transition should be rejected");

    // Job should still be completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_duplicate_queued_events() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(3);
    let job_id = JobId(3);

    // Send JobEvent::Queued twice
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );

    store.apply_job_event(envelope.clone()).await.unwrap();
    let result = store.apply_job_event(envelope).await;

    // Second send should not error (idempotent)
    assert!(
        result.is_ok(),
        "Duplicate same-status event should be idempotent"
    );

    // Job should still be queued, and only appear once in indexes
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Queued);

    let jobs_for_run = store.jobs_for_run(&run_id).await;
    assert_eq!(
        jobs_for_run.len(),
        1,
        "Job should appear exactly once in jobs_by_run"
    );

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_duplicate_completed_events() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(4);
    let job_id = JobId(4);

    // Send JobEvent::Completed twice
    let envelope = make_job_event(
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
    );

    store.apply_job_event(envelope.clone()).await.unwrap();
    let result = store.apply_job_event(envelope).await;

    // Second send should not error (idempotent)
    assert!(
        result.is_ok(),
        "Duplicate completed event should be idempotent"
    );

    // Job should still be completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_unknown_run_id_on_job() {
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(99); // No run event for this ID
    let job_id = JobId(5);

    // Send JobEvent::Queued with run_id that has no corresponding RunEvent
    let envelope = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    // Job should be created successfully with the unknown run_id
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.run_id, run_id);
    assert_eq!(job.status, JobStatus::Queued);

    // Verify indexes are consistent (job is in jobs_by_run even though run doesn't exist)
    let jobs_for_run = store.jobs_for_run(&run_id).await;
    assert!(jobs_for_run.contains(&job_id));

    // Run itself should not exist
    let run = store.get_run(&run_id).await;
    assert!(
        run.is_none(),
        "Run should not exist if no RunEvent was sent"
    );

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_rapid_status_cycling() {
    use crate::job::RunnerInfo;

    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run_id = RunId(5);
    let job_id = JobId(6);

    // Send Queued
    let queued = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(queued).await.unwrap();

    // Send InProgress
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };
    let in_progress = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::InProgress {
            runner: runner.clone(),
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(in_progress).await.unwrap();

    // Send Completed
    let completed = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Completed {
            conclusion: JobConclusion::Success,
            runner: Some(runner),
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(completed).await.unwrap();

    // Try to send Queued again
    let queued_again = make_job_event(
        job_id,
        run_id,
        "org",
        "repo",
        JobEvent::Queued {
            labels: vec!["linux".to_string()],
            steps: vec![],
        },
    );
    let result = store.apply_job_event(queued_again).await;

    // Should be rejected
    assert!(result.is_err(), "Cannot go from Completed back to Queued");

    // Job should remain completed
    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Completed);

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_interleaved_multi_job() {
    use crate::job::RunnerInfo;

    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));

    let run1_id = RunId(10);
    let run2_id = RunId(11);

    // Create two runs
    store
        .apply_run_event(make_run_event(run1_id, RunEvent::Requested))
        .await
        .unwrap();
    store
        .apply_run_event(make_run_event(run2_id, RunEvent::Requested))
        .await
        .unwrap();
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Add jobs 1-3 to run 1
    for i in 1..=3 {
        let job_id = JobId(i * 100);
        let queued = make_job_event(
            job_id,
            run1_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued).await.unwrap();

        let in_progress = make_job_event(
            job_id,
            run1_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner: runner.clone(),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(in_progress).await.unwrap();
    }

    // Add jobs 4-5 to run 2
    for i in 4..=5 {
        let job_id = JobId(i * 100);
        let queued = make_job_event(
            job_id,
            run2_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued).await.unwrap();
    }

    // Verify both runs' indexes are correct and no cross-contamination
    let run1_jobs = store.jobs_for_run(&run1_id).await;
    assert_eq!(run1_jobs.len(), 3);
    for i in 1..=3 {
        assert!(run1_jobs.contains(&JobId(i * 100)));
    }

    let run2_jobs = store.jobs_for_run(&run2_id).await;
    assert_eq!(run2_jobs.len(), 2);
    for i in 4..=5 {
        assert!(run2_jobs.contains(&JobId(i * 100)));
    }

    store.assert_invariants().await;
}

#[tokio::test]
async fn test_ac6_5_eviction_with_mixed_state() {
    use crate::job::RunnerInfo;

    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

    let run_id = RunId(20);

    // Create run
    store
        .apply_run_event(make_run_event(run_id, RunEvent::Requested))
        .await
        .unwrap();
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Create some completed jobs
    for i in 1..=3 {
        let job_id = JobId(i);
        let completed_envelope = make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner.clone()),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(completed_envelope).await.unwrap();
    }

    // Create some active jobs
    for i in 4..=6 {
        let job_id = JobId(i);
        let queued_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued_envelope).await.unwrap();
    }

    // Advance time past TTL
    clock.advance(TimeDelta::hours(2));
    store.evict_expired().await;

    // Verify: completed jobs (1-3) evicted, active jobs (4-6) retained
    for i in 1..=3 {
        let job = store.get_job(&JobId(i)).await;
        assert!(job.is_none(), "Completed job {i} should be evicted");
    }

    for i in 4..=6 {
        let job = store.get_job(&JobId(i)).await;
        assert!(job.is_some(), "Active job {i} should be retained");
    }

    // Verify indexes are consistent
    store.assert_invariants().await;
}
