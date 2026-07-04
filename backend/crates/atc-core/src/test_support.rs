//! Shared test fixtures for the domain model.
//!
//! Compiled only under `#[cfg(any(test, feature = "test-support"))]`, mirroring
//! the [`crate::clock::TestClock`] / [`crate::fixed_test_timestamp`] gate so
//! cross-crate dev-deps (`atc-core = { path = "...", features =
//! ["test-support"] }`) opt in explicitly.
//!
//! Two shapes live here:
//!
//! - **Event-envelope builders** ([`make_run_event`], [`make_job_event`]) take
//!   the identifying fields plus the `action` under test and fill the rest with
//!   fixed defaults. They derive timestamps from the action the same way the
//!   GitHub translation layer does. These replace byte-identical private copies
//!   that previously lived in `atc-core`'s state-machine tests and
//!   `atc-server`'s in-memory store tests.
//! - **Domain-struct factories** ([`make_workflow_run`], [`make_job`],
//!   [`make_step`], [`make_runner_info`]) take no arguments and return a
//!   canonical instance. Tests override the fields they care about with
//!   struct-update syntax: `WorkflowRun { status: RunStatus::Completed,
//!   ..make_workflow_run() }`.
//!
//! All builders use [`crate::fixed_test_timestamp`] so timestamps are
//! deterministic across runs.

use crate::clock::fixed_test_timestamp;
use crate::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use crate::job::{Job, JobStatus, RunnerInfo, Step, StepStatus};
use crate::run::{RunStatus, WorkflowRun};
use crate::types::{JobId, RepoId, RunId};

/// Test repository id for `octocat/Hello-World` — GitHub's own API-docs
/// sample repository id, reused here so envelope/run fixtures look real.
const TEST_REPO_ID: RepoId = RepoId(1_296_269);

/// Build a [`RunEventEnvelope`] with sensible defaults for the given `run_id`
/// and `action`.
///
/// `completed_at` defaults to `Some(now)` only when the action is
/// [`RunEvent::Completed`], mirroring the GitHub translation layer's behavior
/// (`workflow_run.updated_at` is the best-available proxy for the completion
/// timestamp on the `completed` action; absent on non-completed actions).
/// Override any other field with struct-update syntax.
#[must_use]
pub fn make_run_event(run_id: RunId, action: RunEvent) -> RunEventEnvelope {
    let now = fixed_test_timestamp();
    let completed_at = matches!(action, RunEvent::Completed { .. }).then_some(now);
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
        completed_at,
        run_attempt: 1,
        repo_id: Some(TEST_REPO_ID),
        action,
    }
}

/// Build a [`JobEventEnvelope`] with sensible defaults for the given ids,
/// `org`/`repo`, and `action`.
///
/// `started_at` and `completed_at` default to `None`; tests that need a
/// completion timestamp set it with struct-update syntax:
/// `JobEventEnvelope { completed_at: Some(t), ..make_job_event(..) }`.
#[must_use]
pub fn make_job_event(
    job_id: JobId,
    run_id: RunId,
    org: &str,
    repo: &str,
    action: JobEvent,
) -> JobEventEnvelope {
    let now = fixed_test_timestamp();
    JobEventEnvelope {
        job_id,
        run_id,
        org: org.to_string(),
        repo: repo.to_string(),
        name: "Test Job".to_string(),
        created_at: now,
        started_at: None,
        completed_at: None,
        run_attempt: 1,
        repo_id: Some(TEST_REPO_ID),
        action,
    }
}

/// Build a canonical [`WorkflowRun`] (a `Queued` run for `octocat/Hello-World`).
///
/// Override the fields under test with struct-update syntax:
/// `WorkflowRun { status: RunStatus::Completed, ..make_workflow_run() }`.
#[must_use]
pub fn make_workflow_run() -> WorkflowRun {
    let now = fixed_test_timestamp();
    WorkflowRun {
        id: RunId(1),
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: Some("CI".to_string()),
        workflow_path: Some(".github/workflows/ci.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123def456".to_string(),
        commit_message: Some("Fix bug".to_string()),
        event: "push".to_string(),
        display_title: "CI Run".to_string(),
        status: RunStatus::Queued,
        conclusion: None,
        html_url: "https://github.com/octocat/Hello-World/actions/runs/1".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        completed_at: None,
        run_attempt: 1,
        repo_id: Some(TEST_REPO_ID),
    }
}

/// Build a canonical [`Job`] (a `Queued` job with no runner, labels, or steps).
///
/// Override the fields under test with struct-update syntax:
/// `Job { status: JobStatus::Completed, ..make_job() }`.
#[must_use]
pub fn make_job() -> Job {
    let now = fixed_test_timestamp();
    Job {
        id: JobId(1),
        name: "Test Job".to_string(),
        run_id: RunId(1),
        status: JobStatus::Queued,
        conclusion: None,
        runner: None,
        labels: vec![],
        steps: vec![],
        created_at: now,
        started_at: None,
        completed_at: None,
        run_attempt: 1,
    }
}

/// Build a canonical [`Step`] (a `Queued` step with no conclusion or
/// timestamps).
///
/// Override the fields under test with struct-update syntax:
/// `Step { status: StepStatus::Completed, ..make_step() }`.
#[must_use]
pub fn make_step() -> Step {
    Step {
        number: 1,
        name: "test step".to_string(),
        status: StepStatus::Queued,
        conclusion: None,
        started_at: None,
        completed_at: None,
    }
}

/// Build a canonical [`RunnerInfo`] (`runner-1`, no group).
///
/// Override the fields under test with struct-update syntax:
/// `RunnerInfo { group_name: Some("default".to_string()), ..make_runner_info() }`.
#[must_use]
pub fn make_runner_info() -> RunnerInfo {
    RunnerInfo {
        id: 1,
        name: "runner-1".to_string(),
        group_name: None,
    }
}
