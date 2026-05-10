use super::*;
use crate::clock::TestClock;
use crate::job::{JobConclusion, RunnerInfo, Step, StepStatus};
use crate::run::RunConclusion;
use chrono::{DateTime, Utc};

mod edge_cases;
mod event_ingestion;
mod eviction;
mod queries;
mod webhook_domain_updates;

/// Helper to build a `RunEventEnvelope` with sensible defaults.
fn make_run_event(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = Utc::now();
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

/// Helper to build a `JobEventEnvelope` with sensible defaults.
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

/// Helper to build a `JobEventEnvelope` with custom `completed_at` timestamp.
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
