//! Domain event types for state store ingestion.
//!
//! These types are source-agnostic — they carry domain data, not raw
//! webhook payloads. The `atc-github` crate maps webhook JSON into these types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::job::{JobConclusion, RunnerInfo, Step};
use crate::run::RunConclusion;
use crate::types::{JobId, RunId};

/// Action that occurred on a workflow run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum RunEvent {
    /// New run appeared (maps to `workflow_run` `requested` action).
    Requested,
    /// Run started executing.
    InProgress,
    /// Run finished.
    Completed {
        /// The conclusion of the run.
        conclusion: RunConclusion,
    },
}

/// Full run event data for state store ingestion.
///
/// Carries all fields needed to create or update a `WorkflowRun`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunEventEnvelope {
    /// Unique identifier for the run.
    pub run_id: RunId,
    /// Organization name.
    pub org: String,
    /// Repository name.
    pub repo: String,
    /// Workflow name from the `workflow` object. `None` when GitHub sends
    /// `workflow: null` (common on `in_progress` and `completed` events).
    pub workflow_name: Option<String>,
    /// Workflow file path. `None` when GitHub sends `workflow: null`.
    pub workflow_path: Option<String>,
    /// Branch name, if applicable.
    pub branch: Option<String>,
    /// Head commit SHA.
    pub head_sha: String,
    /// Head commit message.
    pub commit_message: Option<String>,
    /// Event that triggered the run (e.g., `push`, `pull_request`).
    pub trigger_event: String,
    /// Display title for the run.
    pub display_title: String,
    /// URL to the run on GitHub.
    pub html_url: String,
    /// When the run was created.
    pub created_at: DateTime<Utc>,
    /// When the run started executing.
    pub run_started_at: Option<DateTime<Utc>>,
    /// When the run was last updated.
    pub updated_at: DateTime<Utc>,
    /// The action that occurred.
    pub action: RunEvent,
}

/// Action that occurred on a job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum JobEvent {
    /// Job entered the queue.
    Queued {
        /// Runner labels requested by this job.
        labels: Vec<String>,
        /// Current step snapshot.
        steps: Vec<Step>,
    },
    /// A job is waiting for approval (environment protection rule, required reviewer).
    Waiting {
        /// Runner labels requested by the job.
        labels: Vec<String>,
        /// Steps defined in the job at the time of the event.
        steps: Vec<Step>,
    },
    /// Job started executing on a runner.
    InProgress {
        /// Runner assigned to the job. `None` when GitHub fires `in_progress`
        /// before runner assignment is complete.
        runner: Option<RunnerInfo>,
        /// Runner labels.
        labels: Vec<String>,
        /// Current step snapshot.
        steps: Vec<Step>,
    },
    /// Job finished executing.
    Completed {
        /// The conclusion of the job.
        conclusion: JobConclusion,
        /// Runner that executed the job, if known.
        runner: Option<RunnerInfo>,
        /// Runner labels.
        labels: Vec<String>,
        /// Final step snapshot.
        steps: Vec<Step>,
    },
}

/// Full job event data for state store ingestion.
///
/// Carries all fields needed to create or update a `Job`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JobEventEnvelope {
    /// Unique identifier for the job.
    pub job_id: JobId,
    /// Back-reference to the parent run.
    pub run_id: RunId,
    /// Organization name.
    pub org: String,
    /// Repository name.
    pub repo: String,
    /// Job name.
    pub name: String,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job started executing.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job finished executing.
    pub completed_at: Option<DateTime<Utc>>,
    /// The action that occurred.
    pub action: JobEvent,
}
