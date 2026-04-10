//! Job, step, and runner types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::{JobId, RunId};

/// Status of a job in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum JobStatus {
    /// Job is waiting in the queue.
    Queued,
    /// Job is waiting for a dependency.
    Waiting,
    /// Job is currently executing on a runner.
    InProgress,
    /// Job has finished executing.
    Completed,
}

/// Conclusion of a completed job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum JobConclusion {
    /// Job succeeded.
    Success,
    /// Job failed.
    Failure,
    /// Job was cancelled.
    Cancelled,
    /// Job exceeded time limit.
    TimedOut,
    /// Job requires manual intervention.
    ActionRequired,
    /// Job became stale.
    Stale,
    /// Job completed with neutral result.
    Neutral,
    /// Job was skipped.
    Skipped,
}

/// Status of a step within a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum StepStatus {
    /// Step is waiting to execute.
    Queued,
    /// Step is currently executing.
    InProgress,
    /// Step has finished executing.
    Completed,
}

/// A step within a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Position number within the job (1-based).
    pub number: i64,
    /// Display name of the step.
    pub name: String,
    /// Current lifecycle status.
    pub status: StepStatus,
    /// Final conclusion, populated when status is `Completed`.
    pub conclusion: Option<JobConclusion>,
    /// When the step started executing.
    pub started_at: Option<DateTime<Utc>>,
    /// When the step finished executing.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Information about the runner executing a job.
///
/// This is a composed struct (not flattened into `Job`) to enable
/// runner pool derivation and group-level reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerInfo {
    /// Unique identifier for the runner.
    pub id: i64,
    /// Display name of the runner.
    pub name: String,
    /// Runner group identifier, if grouped.
    pub group_id: Option<i64>,
    /// Runner group name, if grouped.
    pub group_name: Option<String>,
}

/// A job within a workflow run.
///
/// Created and updated by `JobEvent`s (Phase 2). Steps use snapshot
/// semantics — the entire `Vec<Step>` is replaced on each event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    /// Unique identifier for this job.
    pub id: JobId,
    /// Display name of the job.
    pub name: String,
    /// Back-reference to the parent workflow run.
    pub run_id: RunId,
    /// Current lifecycle status.
    pub status: JobStatus,
    /// Final conclusion, populated when status is `Completed`.
    pub conclusion: Option<JobConclusion>,
    /// Runner assigned to this job, populated when a runner picks it up.
    pub runner: Option<RunnerInfo>,
    /// Runner labels this job requires.
    pub labels: Vec<String>,
    /// Steps within this job, ordered by step number.
    pub steps: Vec<Step>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job started executing.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job finished executing.
    pub completed_at: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
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
        let step_statuses = vec![StepStatus::Queued, StepStatus::InProgress, StepStatus::Completed];

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
}
