//! AC3: State store event ingestion tests.

use super::*;

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

// ===== Job Event Tests =====

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
            runner: Some(runner.clone()),
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
    let repo_alpha_key = RepoKey::new("octocat", "repo-a");
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
    let repo_beta_key = RepoKey::new("octocat", "repo-b");
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
    let jobs_a = store.jobs_for_repo(&repo_alpha_key).await;
    let jobs_b = store.jobs_for_repo(&repo_beta_key).await;

    assert_eq!(jobs_a.len(), 1);
    assert!(jobs_a.contains(&job_id_1));

    assert_eq!(jobs_b.len(), 1);
    assert!(jobs_b.contains(&job_id_2));
}

#[tokio::test]
async fn test_ac3_4_steps_snapshot_replacement() {
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
            runner: Some(runner),
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

// ===== Waiting Event Tests (Task 1 + Task 2) =====

#[tokio::test]
async fn test_ac3_1_waiting_variant_exists() {
    // AC3.1: JobEvent::Waiting variant exists and carries labels and steps
    let step = Step {
        number: 1,
        name: "Checkout".to_string(),
        status: StepStatus::Queued,
        conclusion: None,
        started_at: None,
        completed_at: None,
    };

    let envelope = make_job_event(
        JobId(701),
        RunId(70),
        "octocat",
        "Hello-World",
        JobEvent::Waiting {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![step.clone()],
        },
    );

    // Verify the envelope was created successfully with Waiting variant
    assert_eq!(envelope.job_id, JobId(701));
    assert_eq!(envelope.run_id, RunId(70));
}

#[tokio::test]
async fn test_ac3_2_create_job_from_waiting() {
    // AC3.2: StateStore::apply_job_event handles Waiting events, creating jobs in JobStatus::Waiting
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
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

    let envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Waiting {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![step.clone()],
        },
    );
    store.apply_job_event(envelope).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Waiting);
    assert_eq!(job.labels, vec!["ubuntu-latest".to_string()]);
    assert_eq!(job.steps, vec![step]);
}

#[tokio::test]
async fn test_ac3_3_queued_to_waiting_to_inprogress() {
    // AC3.3: Transition Queued → Waiting → InProgress succeeds
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = StateStore::new(clock, Duration::from_secs(3600));
    let job_id = JobId(703);
    let run_id = RunId(72);
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: None,
        group_name: None,
    };

    // Start with Queued
    let envelope1 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope1).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Queued);

    // Transition to Waiting
    let envelope2 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Waiting {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope2).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::Waiting);

    // Transition to InProgress
    let envelope3 = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::InProgress {
            runner: Some(runner),
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(envelope3).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::InProgress);
}

#[tokio::test]
async fn test_ac3_4_in_progress_with_no_runner() {
    let clock = TestClock::new(Utc::now());
    let store = StateStore::new(Arc::new(clock), Duration::from_secs(3600));

    let run_id = RunId(100);
    let job_id = JobId(1);

    // Create run
    let run_envelope = make_run_event(run_id, RunEvent::Requested);
    store.apply_run_event(run_envelope).await.unwrap();

    // Queue job
    let queued_envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(queued_envelope).await.unwrap();

    // Transition directly to InProgress with None runner
    let in_progress_envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::InProgress {
            runner: None,
            labels: vec!["ubuntu-latest".to_string()],
            steps: vec![],
        },
    );
    store.apply_job_event(in_progress_envelope).await.unwrap();

    let job = store.get_job(&job_id).await.expect("job should exist");
    assert_eq!(job.status, JobStatus::InProgress);
    assert_eq!(job.runner, None);
}
