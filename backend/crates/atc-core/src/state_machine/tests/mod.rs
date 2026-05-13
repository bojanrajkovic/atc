use std::time::Duration;

use super::*;
use crate::clock::fixed_test_timestamp;
use crate::types::{JobId, RunId};

mod pure_application;

/// Helper to build a `RunEventEnvelope` with sensible defaults.
fn make_run_event(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = fixed_test_timestamp();
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
