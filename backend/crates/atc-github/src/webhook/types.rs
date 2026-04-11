//! GitHub webhook payload types for deserialization.
//!
//! These types model the subset of GitHub's webhook JSON that ATC needs.
//! They are `pub(crate)` — consumers of `atc-github` never see them;
//! they only see domain events from `atc-core`.

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Top-level `workflow_run` webhook payload.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowRunWebhook {
    /// Action that triggered the webhook: `"requested"`, `"in_progress"`, or `"completed"`.
    pub action: String,
    /// The workflow run object with run-level metadata.
    pub workflow_run: WorkflowRunData,
    /// The workflow definition. `None` when GitHub sends `workflow: null`
    /// (observed on some `in_progress` and `completed` events).
    pub workflow: Option<WorkflowData>,
    /// Repository where the workflow run occurred.
    pub repository: RepositoryData,
}

/// Top-level `workflow_job` webhook payload.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowJobWebhook {
    /// Action that triggered the webhook: `"queued"`, `"waiting"`,
    /// `"in_progress"`, or `"completed"`.
    pub action: String,
    /// The workflow job object with job-level metadata.
    pub workflow_job: WorkflowJobData,
    /// Repository where the job ran.
    pub repository: RepositoryData,
}

/// Fields from the `workflow_run` nested object that ATC uses.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowRunData {
    /// Unique identifier for this workflow run.
    pub id: i64,
    /// Run status string (e.g., `"queued"`, `"in_progress"`, `"completed"`).
    pub status: String,
    /// Conclusion string (e.g., `"success"`, `"failure"`). `None` until completed.
    pub conclusion: Option<String>,
    /// Branch name where the run was triggered. `None` for tag-triggered runs.
    pub head_branch: Option<String>,
    /// Commit SHA at the head of the branch.
    pub head_sha: String,
    /// Commit metadata. `None` on some payloads (e.g., deleted forks).
    pub head_commit: Option<HeadCommit>,
    /// Event type that triggered the run (e.g., `"push"`, `"pull_request"`).
    pub event: String,
    /// Human-readable run title shown in the GitHub UI.
    pub display_title: String,
    /// Full URL to the run on GitHub.
    pub html_url: String,
    /// When the run was created.
    pub created_at: DateTime<Utc>,
    /// When the run started executing. `None` if not yet started.
    pub run_started_at: Option<DateTime<Utc>>,
    /// When the run was last updated.
    pub updated_at: DateTime<Utc>,
}

/// Fields from the `workflow` top-level object.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowData {
    /// Workflow name as defined in the YAML file.
    pub name: String,
    /// Path to the workflow YAML file (e.g., `".github/workflows/ci.yml"`).
    pub path: String,
}

/// Fields from the `workflow_job` nested object that ATC uses.
#[derive(Debug, Deserialize)]
pub(crate) struct WorkflowJobData {
    /// Unique identifier for this job.
    pub id: i64,
    /// Parent workflow run ID.
    pub run_id: i64,
    /// Job name as defined in the workflow YAML.
    pub name: String,
    /// Job status string (e.g., `"queued"`, `"waiting"`, `"in_progress"`, `"completed"`).
    pub status: String,
    /// Conclusion string (e.g., `"success"`, `"failure"`). `None` until completed.
    pub conclusion: Option<String>,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job started executing. `None` if not yet started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job finished. `None` until completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Steps defined in the job. Defaults to empty if omitted from the payload
    /// (GitHub omits `steps` when jobs are triggered by check_run events).
    #[serde(default)]
    pub steps: Vec<StepData>,
    /// Runner labels requested by the job (e.g., `["ubuntu-latest"]`).
    pub labels: Vec<String>,
    /// Runner ID assigned to the job. `None` until runner assignment.
    pub runner_id: Option<i64>,
    /// Runner name. `None` until runner assignment.
    pub runner_name: Option<String>,
    /// Runner group ID (for self-hosted runners). `None` for GitHub-hosted.
    pub runner_group_id: Option<i64>,
    /// Runner group name. `None` for GitHub-hosted.
    pub runner_group_name: Option<String>,
}

/// A single step within a workflow job.
#[derive(Debug, Deserialize)]
pub(crate) struct StepData {
    /// Step sequence number (1-indexed).
    pub number: i32,
    /// Step name from the workflow YAML or auto-generated.
    pub name: String,
    /// Step status string (e.g., `"queued"`, `"in_progress"`, `"completed"`).
    pub status: String,
    /// Step conclusion string. `None` until the step completes.
    pub conclusion: Option<String>,
    /// When the step started. `None` if not yet started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the step finished. `None` until completed.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Repository metadata from the webhook payload.
#[derive(Debug, Deserialize)]
pub(crate) struct RepositoryData {
    /// Repository owner (organization or user).
    pub owner: OwnerData,
    /// Repository name (without owner prefix).
    pub name: String,
}

/// Repository owner metadata.
#[derive(Debug, Deserialize)]
pub(crate) struct OwnerData {
    /// Owner login name (e.g., `"octocat"` or `"my-org"`).
    pub login: String,
}

/// Commit metadata from `head_commit`.
#[derive(Debug, Deserialize)]
pub(crate) struct HeadCommit {
    /// Commit message text.
    pub message: String,
}
