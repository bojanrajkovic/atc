//! Job, step, and runner types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::types::{JobId, RunId};

/// Status of a job in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
pub enum StepStatus {
    /// Step is waiting to execute.
    Queued,
    /// Step is currently executing.
    InProgress,
    /// Step has finished executing.
    Completed,
}

/// A step within a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
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

use std::fmt;

/// Error returned when an invalid job status transition is attempted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidJobTransition {
    /// The current status.
    pub from: JobStatus,
    /// The attempted target status.
    pub to: JobStatus,
}

impl fmt::Display for InvalidJobTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid job transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for InvalidJobTransition {}

impl JobStatus {
    /// Attempt to transition to the target status.
    ///
    /// Returns the new status on success, or `InvalidJobTransition` if
    /// the transition is not allowed. Same-status transitions are
    /// idempotent and always succeed.
    ///
    /// # Valid transitions
    ///
    /// - `Queued` -> `Waiting` | `InProgress`
    /// - `Waiting` -> `InProgress`
    /// - `InProgress` -> `Completed`
    ///
    /// Note: `Waiting` transitions are not in design AC2.1 but are
    /// included to match GitHub's `workflow_job` model where jobs
    /// can enter a `waiting` state for dependency resolution.
    ///
    /// # Errors
    ///
    /// Returns `InvalidJobTransition` for any transition not listed above
    /// (excluding idempotent same-status).
    pub fn transition_to(self, target: Self) -> Result<Self, InvalidJobTransition> {
        if self == target {
            return Ok(self);
        }
        match (self, target) {
            (Self::Queued, Self::Waiting | Self::InProgress)
            | (Self::Waiting, Self::InProgress)
            | (Self::InProgress, Self::Completed) => Ok(target),
            _ => Err(InvalidJobTransition {
                from: self,
                to: target,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
