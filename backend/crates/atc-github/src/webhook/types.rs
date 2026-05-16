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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    /// (GitHub omits `steps` when jobs are triggered by `check_run` events).
    #[serde(default)]
    pub steps: Vec<StepData>,
    /// Runner labels requested by the job (e.g., `["ubuntu-latest"]`).
    pub labels: Vec<String>,
    /// Runner ID assigned to the job. `None` until runner assignment.
    pub runner_id: Option<i64>,
    /// Runner name. `None` until runner assignment.
    pub runner_name: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_workflow_run_requested_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_run_requested.json");
        let payload: WorkflowRunWebhook =
            serde_json::from_str(json).expect("Should deserialize workflow_run_requested fixture");

        assert_eq!(payload.action, "requested");
        assert!(!payload.workflow_run.head_sha.is_empty());
        assert!(!payload.repository.name.is_empty());
    }

    #[test]
    fn test_workflow_run_in_progress_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_run_in_progress.json");
        let payload: WorkflowRunWebhook = serde_json::from_str(json)
            .expect("Should deserialize workflow_run_in_progress fixture");

        assert_eq!(payload.action, "in_progress");
        assert!(!payload.workflow_run.head_sha.is_empty());
        assert!(!payload.repository.name.is_empty());
    }

    #[test]
    fn test_workflow_run_completed_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_run_completed.json");
        let payload: WorkflowRunWebhook =
            serde_json::from_str(json).expect("Should deserialize workflow_run_completed fixture");

        assert_eq!(payload.action, "completed");
        assert!(!payload.workflow_run.head_sha.is_empty());
        assert!(!payload.repository.name.is_empty());
    }

    #[test]
    fn test_workflow_job_queued_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_job_queued.json");
        let payload: WorkflowJobWebhook =
            serde_json::from_str(json).expect("Should deserialize workflow_job_queued fixture");

        assert_eq!(payload.action, "queued");
        assert!(payload.workflow_job.run_id > 0);
    }

    #[test]
    fn test_workflow_job_in_progress_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_job_in_progress.json");
        let payload: WorkflowJobWebhook = serde_json::from_str(json)
            .expect("Should deserialize workflow_job_in_progress fixture");

        assert_eq!(payload.action, "in_progress");
        assert!(payload.workflow_job.run_id > 0);
    }

    #[test]
    fn test_workflow_job_completed_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_job_completed.json");
        let payload: WorkflowJobWebhook =
            serde_json::from_str(json).expect("Should deserialize workflow_job_completed fixture");

        assert_eq!(payload.action, "completed");
        assert!(payload.workflow_job.run_id > 0);
    }

    #[test]
    fn test_workflow_job_waiting_fixture() {
        let json = include_str!("../../tests/fixtures/workflow_job_waiting.json");
        let payload: WorkflowJobWebhook =
            serde_json::from_str(json).expect("Should deserialize workflow_job_waiting fixture");

        assert_eq!(payload.action, "waiting");
        assert_eq!(payload.workflow_job.status, "waiting");
    }

    #[test]
    fn test_workflow_run_all_fields_populated() {
        let json = include_str!("../../tests/fixtures/workflow_run_requested.json");
        let payload: WorkflowRunWebhook = serde_json::from_str(json).expect("Should deserialize");

        assert!(payload.workflow_run.id > 0);
        assert!(!payload.workflow_run.status.is_empty());
        assert!(!payload.workflow_run.head_sha.is_empty());
        assert!(!payload.workflow_run.event.is_empty());
        assert!(!payload.workflow_run.display_title.is_empty());
        assert!(!payload.workflow_run.html_url.is_empty());
        assert!(!payload.workflow_run.updated_at.to_rfc3339().is_empty());
    }

    #[test]
    fn test_null_head_commit() {
        let json_str = json!({
            "action": "requested",
            "workflow_run": {
                "id": 12345,
                "status": "queued",
                "conclusion": null,
                "head_branch": "main",
                "head_sha": "abc123",
                "head_commit": null,
                "event": "push",
                "display_title": "Test Run",
                "html_url": "https://example.com/run",
                "created_at": "2026-04-11T20:30:23Z",
                "updated_at": "2026-04-11T20:30:23Z"
            },
            "workflow": null,
            "repository": {
                "owner": {"login": "test"},
                "name": "repo"
            }
        })
        .to_string();

        let payload: WorkflowRunWebhook =
            serde_json::from_str(&json_str).expect("Should deserialize with null head_commit");

        assert!(payload.workflow_run.head_commit.is_none());
    }

    #[test]
    fn test_null_workflow() {
        let json_str = json!({
            "action": "in_progress",
            "workflow_run": {
                "id": 12345,
                "status": "in_progress",
                "conclusion": null,
                "head_branch": "main",
                "head_sha": "abc123",
                "head_commit": {"message": "Test commit"},
                "event": "push",
                "display_title": "Test Run",
                "html_url": "https://example.com/run",
                "created_at": "2026-04-11T20:30:23Z",
                "updated_at": "2026-04-11T20:30:23Z"
            },
            "workflow": null,
            "repository": {
                "owner": {"login": "test"},
                "name": "repo"
            }
        })
        .to_string();

        let payload: WorkflowRunWebhook =
            serde_json::from_str(&json_str).expect("Should deserialize with null workflow");

        assert!(payload.workflow.is_none());
    }

    #[test]
    fn test_workflow_job_null_runner_fields() {
        // queued jobs don't have runners assigned yet
        let json = include_str!("../../tests/fixtures/workflow_job_queued.json");
        let payload: WorkflowJobWebhook = serde_json::from_str(json).expect("Should deserialize");

        assert!(payload.workflow_job.runner_id.is_none());
        assert!(payload.workflow_job.runner_name.is_none());
    }

    #[test]
    fn test_unknown_fields_ignored() {
        let json_str = json!({
            "action": "requested",
            "unknown_future_field": 42,
            "workflow_run": {
                "id": 12345,
                "status": "queued",
                "conclusion": null,
                "head_branch": "main",
                "head_sha": "abc123",
                "head_commit": null,
                "event": "push",
                "display_title": "Test Run",
                "html_url": "https://example.com/run",
                "created_at": "2026-04-11T20:30:23Z",
                "updated_at": "2026-04-11T20:30:23Z"
            },
            "workflow": null,
            "repository": {
                "owner": {"login": "test"},
                "name": "repo"
            }
        })
        .to_string();

        let payload: WorkflowRunWebhook =
            serde_json::from_str(&json_str).expect("Should deserialize and ignore unknown fields");

        assert_eq!(payload.action, "requested");
    }
}
