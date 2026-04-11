use super::*;

// core-domain.AC1.1 (job fields): Construct a `Job` with all fields populated
// including `steps: vec![Step { ... }]` and `runner: Some(RunnerInfo { ... })`,
// verify each field is accessible.
#[test]
fn test_job_with_all_fields() {
    let now = Utc::now();
    let runner = RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_id: Some(10),
        group_name: Some("default".to_string()),
    };
    let step = Step {
        number: 1,
        name: "Run build".to_string(),
        status: StepStatus::Completed,
        conclusion: Some(JobConclusion::Success),
        started_at: Some(now),
        completed_at: Some(now),
    };
    let job = Job {
        id: JobId(42),
        name: "Test Job".to_string(),
        run_id: RunId(100),
        status: JobStatus::Completed,
        conclusion: Some(JobConclusion::Success),
        runner: Some(runner.clone()),
        labels: vec!["linux".to_string(), "self-hosted".to_string()],
        steps: vec![step.clone()],
        created_at: now,
        started_at: Some(now),
        completed_at: Some(now),
    };

    // Verify each field is accessible
    assert_eq!(job.id, JobId(42));
    assert_eq!(job.name, "Test Job");
    assert_eq!(job.run_id, RunId(100));
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.conclusion, Some(JobConclusion::Success));
    assert_eq!(job.runner, Some(runner));
    assert_eq!(job.labels.len(), 2);
    assert_eq!(job.steps.len(), 1);
    assert_eq!(job.steps[0], step);
    assert_eq!(job.created_at, now);
    assert_eq!(job.started_at, Some(now));
    assert_eq!(job.completed_at, Some(now));
}

// core-domain.AC1.1 (step fields): Construct a `Step` with all fields,
// verify accessible.
#[test]
fn test_step_with_all_fields() {
    let now = Utc::now();
    let step = Step {
        number: 1,
        name: "Build".to_string(),
        status: StepStatus::InProgress,
        conclusion: None,
        started_at: Some(now),
        completed_at: None,
    };

    assert_eq!(step.number, 1);
    assert_eq!(step.name, "Build");
    assert_eq!(step.status, StepStatus::InProgress);
    assert_eq!(step.conclusion, None);
    assert_eq!(step.started_at, Some(now));
    assert_eq!(step.completed_at, None);
}

// core-domain.AC1.4 (job serde): Serialize a `Job` (with populated steps
// and runner) to JSON, deserialize back, assert round-trip equality.
// Test `JobStatus` and `StepStatus` enum serialization.
#[test]
fn test_job_serde_round_trip() {
    let now = Utc::now();
    let runner = RunnerInfo {
        id: 5,
        name: "runner-5".to_string(),
        group_id: Some(20),
        group_name: Some("ci-group".to_string()),
    };
    let step = Step {
        number: 1,
        name: "Test step".to_string(),
        status: StepStatus::Completed,
        conclusion: Some(JobConclusion::Success),
        started_at: Some(now),
        completed_at: Some(now),
    };
    let job = Job {
        id: JobId(999),
        name: "Complex Job".to_string(),
        run_id: RunId(888),
        status: JobStatus::Completed,
        conclusion: Some(JobConclusion::Success),
        runner: Some(runner.clone()),
        labels: vec!["ubuntu".to_string(), "x64".to_string()],
        steps: vec![step.clone()],
        created_at: now,
        started_at: Some(now),
        completed_at: Some(now),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&job).expect("failed to serialize job");

    // Deserialize back
    let deserialized: Job = serde_json::from_str(&json).expect("failed to deserialize job");

    // Verify round-trip equality
    assert_eq!(deserialized, job);
}

// Test JobStatus enum serialization
#[test]
fn test_job_status_serialization() {
    let job_statuses = vec![
        JobStatus::Queued,
        JobStatus::Waiting,
        JobStatus::InProgress,
        JobStatus::Completed,
    ];

    for status in job_statuses {
        let json = serde_json::to_string(&status).expect("failed to serialize status");
        let deserialized: JobStatus =
            serde_json::from_str(&json).expect("failed to deserialize status");
        assert_eq!(status, deserialized);
    }
}

// Test StepStatus enum serialization
#[test]
fn test_step_status_serialization() {
    let step_statuses = vec![
        StepStatus::Queued,
        StepStatus::InProgress,
        StepStatus::Completed,
    ];

    for status in step_statuses {
        let json = serde_json::to_string(&status).expect("failed to serialize status");
        let deserialized: StepStatus =
            serde_json::from_str(&json).expect("failed to deserialize status");
        assert_eq!(status, deserialized);
    }
}

// core-domain.AC1.5: Construct a `RunnerInfo` with `id`, `name`, `group_id: Some(1)`,
// `group_name: Some("default")`. Embed it in a `Job`. Verify `RunnerInfo` is a
// separate struct accessible via `job.runner`. Serialize/deserialize `RunnerInfo`
// independently.
#[test]
fn test_runner_info_as_separate_struct() {
    let runner = RunnerInfo {
        id: 123,
        name: "my-runner".to_string(),
        group_id: Some(1),
        group_name: Some("default".to_string()),
    };

    // Verify RunnerInfo is accessible as separate struct
    assert_eq!(runner.id, 123);
    assert_eq!(runner.name, "my-runner");
    assert_eq!(runner.group_id, Some(1));
    assert_eq!(runner.group_name, Some("default".to_string()));

    // Embed in a Job
    let job = Job {
        id: JobId(1),
        name: "test".to_string(),
        run_id: RunId(1),
        status: JobStatus::InProgress,
        conclusion: None,
        runner: Some(runner.clone()),
        labels: vec![],
        steps: vec![],
        created_at: Utc::now(),
        started_at: None,
        completed_at: None,
    };

    // Verify accessible via job.runner
    assert_eq!(job.runner, Some(runner.clone()));

    // Serialize/deserialize RunnerInfo independently
    let json = serde_json::to_string(&runner).expect("failed to serialize runner");
    let deserialized: RunnerInfo =
        serde_json::from_str(&json).expect("failed to deserialize runner");
    assert_eq!(deserialized, runner);
}

// Test Step serde round-trip
#[test]
fn test_step_serde_round_trip() {
    let now = Utc::now();
    let step = Step {
        number: 5,
        name: "Integration tests".to_string(),
        status: StepStatus::Completed,
        conclusion: Some(JobConclusion::Failure),
        started_at: Some(now),
        completed_at: Some(now),
    };

    let json = serde_json::to_string(&step).expect("failed to serialize step");
    let deserialized: Step = serde_json::from_str(&json).expect("failed to deserialize step");

    assert_eq!(deserialized, step);
}

// Test JobConclusion enum serialization
#[test]
fn test_job_conclusion_serialization() {
    let conclusions = vec![
        JobConclusion::Success,
        JobConclusion::Failure,
        JobConclusion::Cancelled,
        JobConclusion::TimedOut,
        JobConclusion::ActionRequired,
        JobConclusion::Stale,
        JobConclusion::Neutral,
        JobConclusion::Skipped,
    ];

    for conclusion in conclusions {
        let json = serde_json::to_string(&conclusion).expect("failed to serialize conclusion");
        let deserialized: JobConclusion =
            serde_json::from_str(&json).expect("failed to deserialize conclusion");
        assert_eq!(conclusion, deserialized);
    }
}

// core-domain.AC2.1: Valid job transitions succeed
#[test]
fn test_job_transition_queued_to_in_progress() {
    let result = JobStatus::Queued.transition_to(JobStatus::InProgress);
    assert_eq!(result, Ok(JobStatus::InProgress));
}

#[test]
fn test_job_transition_queued_to_waiting() {
    let result = JobStatus::Queued.transition_to(JobStatus::Waiting);
    assert_eq!(result, Ok(JobStatus::Waiting));
}

#[test]
fn test_job_transition_waiting_to_in_progress() {
    let result = JobStatus::Waiting.transition_to(JobStatus::InProgress);
    assert_eq!(result, Ok(JobStatus::InProgress));
}

#[test]
fn test_job_transition_in_progress_to_completed() {
    let result = JobStatus::InProgress.transition_to(JobStatus::Completed);
    assert_eq!(result, Ok(JobStatus::Completed));
}

// core-domain.AC2.3: Invalid transitions return Err(InvalidJobTransition)
#[test]
fn test_job_transition_completed_to_in_progress_fails() {
    let result = JobStatus::Completed.transition_to(JobStatus::InProgress);
    assert_eq!(
        result,
        Err(InvalidJobTransition {
            from: JobStatus::Completed,
            to: JobStatus::InProgress,
        })
    );
}

#[test]
fn test_job_transition_queued_to_completed_fails() {
    let result = JobStatus::Queued.transition_to(JobStatus::Completed);
    assert_eq!(
        result,
        Err(InvalidJobTransition {
            from: JobStatus::Queued,
            to: JobStatus::Completed,
        })
    );
}

#[test]
fn test_job_transition_in_progress_to_queued_fails() {
    let result = JobStatus::InProgress.transition_to(JobStatus::Queued);
    assert_eq!(
        result,
        Err(InvalidJobTransition {
            from: JobStatus::InProgress,
            to: JobStatus::Queued,
        })
    );
}

#[test]
fn test_job_transition_completed_to_waiting_fails() {
    let result = JobStatus::Completed.transition_to(JobStatus::Waiting);
    assert_eq!(
        result,
        Err(InvalidJobTransition {
            from: JobStatus::Completed,
            to: JobStatus::Waiting,
        })
    );
}

// core-domain.AC2.4: Idempotent re-application of same status always succeeds
#[test]
fn test_job_transition_queued_to_queued_idempotent() {
    let result = JobStatus::Queued.transition_to(JobStatus::Queued);
    assert_eq!(result, Ok(JobStatus::Queued));
}

#[test]
fn test_job_transition_waiting_to_waiting_idempotent() {
    let result = JobStatus::Waiting.transition_to(JobStatus::Waiting);
    assert_eq!(result, Ok(JobStatus::Waiting));
}

#[test]
fn test_job_transition_in_progress_to_in_progress_idempotent() {
    let result = JobStatus::InProgress.transition_to(JobStatus::InProgress);
    assert_eq!(result, Ok(JobStatus::InProgress));
}

#[test]
fn test_job_transition_completed_to_completed_idempotent() {
    let result = JobStatus::Completed.transition_to(JobStatus::Completed);
    assert_eq!(result, Ok(JobStatus::Completed));
}
