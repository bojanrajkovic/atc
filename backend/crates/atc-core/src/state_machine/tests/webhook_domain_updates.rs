// Tests for gh-webhooks.AC3: domain model updates required by webhook parsing.
// Covers JobEvent::Waiting, optional InProgress runner, and workflow field .or() preservation.

use super::*;

// ===== Waiting Event Tests =====

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
    // AC3.2: RunStateMachine::apply_job_event handles Waiting events, creating jobs in JobStatus::Waiting
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));
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
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));
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
    let store = RunStateMachine::new(Arc::new(clock), Duration::from_secs(3600));

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

// ===== Workflow Field Preservation Tests =====

#[tokio::test]
async fn test_ac3_7_workflow_name_preservation_with_or() {
    // AC3.7: RunEventEnvelope.workflow_name and workflow_path are Option<String>
    // and preserved via .or() when a later event arrives with None
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));
    let run_id = RunId(800);

    // Create run with Requested event carrying workflow_name: Some("CI")
    let mut envelope1 = make_run_event(run_id, RunEvent::Requested);
    envelope1.workflow_name = Some("CI".to_string());
    envelope1.workflow_path = Some(".github/workflows/ci.yml".to_string());
    store.apply_run_event(envelope1).await.unwrap();

    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.workflow_name, Some("CI".to_string()));
    assert_eq!(
        run.workflow_path,
        Some(".github/workflows/ci.yml".to_string())
    );

    // Update with InProgress event carrying workflow_name: None
    let mut envelope2 = make_run_event(run_id, RunEvent::InProgress);
    envelope2.workflow_name = None;
    envelope2.workflow_path = None;
    store.apply_run_event(envelope2).await.unwrap();

    // Stored run should still have the workflow_name and workflow_path from envelope1
    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.workflow_name, Some("CI".to_string()));
    assert_eq!(
        run.workflow_path,
        Some(".github/workflows/ci.yml".to_string())
    );
}

#[tokio::test]
async fn test_ac3_8_workflow_name_preservation_failure_mode() {
    // AC3.8: Specific failure test - requested with Some("CI") then in_progress with None
    // should preserve "CI", not overwrite with None
    let start_time = Utc::now();
    let clock = Arc::new(TestClock::new(start_time));
    let store = RunStateMachine::new(clock, Duration::from_secs(3600));
    let run_id = RunId(801);

    // Requested with workflow_name: Some("CI")
    let mut envelope_requested = make_run_event(run_id, RunEvent::Requested);
    envelope_requested.workflow_name = Some("CI".to_string());
    envelope_requested.workflow_path = Some(".github/workflows/ci.yml".to_string());
    store.apply_run_event(envelope_requested).await.unwrap();

    // Verify it was stored
    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(run.workflow_name, Some("CI".to_string()));

    // InProgress with workflow_name: None
    let mut envelope_in_progress = make_run_event(run_id, RunEvent::InProgress);
    envelope_in_progress.workflow_name = None;
    envelope_in_progress.workflow_path = None;
    store.apply_run_event(envelope_in_progress).await.unwrap();

    // Should still show CI, not None
    let run = store.get_run(&run_id).await.expect("run should exist");
    assert_eq!(
        run.workflow_name,
        Some("CI".to_string()),
        "workflow_name should be preserved as Some(\"CI\"), not overwritten with None"
    );
}
