//! Tests for the pure free functions in `state_machine`:
//! `apply_run_event`, `apply_job_event`, and `is_evictable`.
//!
//! These tests verify transition behavior, idempotency, first-sight creation,
//! struct-update field merging, and the evictability predicate — all without
//! any locks or async runtime.

use super::*;
use crate::clock::fixed_test_timestamp;
use crate::job::{JobConclusion, Step, StepStatus};
use crate::run::RunConclusion;
use chrono::Utc;

// ===== apply_run_event =====

/// A completed run cannot transition back to `InProgress`.
#[test]
fn run_forward_only_rejects_completed_to_in_progress() {
    let now = fixed_test_timestamp();
    let run_id = RunId(1001);
    let completed_run = WorkflowRun {
        id: run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: Some("CI".to_string()),
        workflow_path: Some(".github/workflows/ci.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("Fix bug".to_string()),
        event: "push".to_string(),
        display_title: "CI Run".to_string(),
        status: RunStatus::Completed,
        conclusion: Some(RunConclusion::Success),
        html_url: "https://github.com/octocat/Hello-World/actions/runs/1001".to_string(),
        created_at: now,
        run_started_at: Some(now),
        updated_at: now,
    };

    let envelope = make_run_event(run_id, RunEvent::InProgress);
    let result = apply_run_event(Some(completed_run), envelope);
    assert!(
        matches!(result, Err(StateMachineError::InvalidRunTransition(_))),
        "expected InvalidRunTransition error, got {result:?}"
    );
}

/// Sending `Requested` twice is idempotent — status stays Queued.
#[test]
fn run_idempotent_same_status_requested_twice() {
    let run_id = RunId(1002);

    // First application: None -> Queued
    let envelope1 = make_run_event(run_id, RunEvent::Requested);
    let queued_run = apply_run_event(None, envelope1).expect("first-sight should succeed");
    assert_eq!(queued_run.status, RunStatus::Queued);

    // Second application: Queued -> Queued (idempotent)
    let envelope2 = make_run_event(run_id, RunEvent::Requested);
    let result = apply_run_event(Some(queued_run), envelope2).expect("idempotent should succeed");
    assert_eq!(result.status, RunStatus::Queued);
}

/// First-sight: `apply_run_event(None, Requested)` creates a new run with envelope fields.
#[test]
fn run_first_sight_from_none_creates_run() {
    let now = fixed_test_timestamp();
    let run_id = RunId(1003);
    let envelope = RunEventEnvelope {
        run_id,
        org: "myorg".to_string(),
        repo: "myrepo".to_string(),
        workflow_name: Some("Build".to_string()),
        workflow_path: Some(".github/workflows/build.yml".to_string()),
        branch: Some("feature".to_string()),
        head_sha: "deadbeef".to_string(),
        commit_message: Some("Add feature".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Build #42".to_string(),
        html_url: "https://github.com/myorg/myrepo/actions/runs/1003".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        action: RunEvent::Requested,
    };

    let run = apply_run_event(None, envelope).expect("first-sight should succeed");
    assert_eq!(run.id, run_id);
    assert_eq!(run.org, "myorg");
    assert_eq!(run.repo, "myrepo");
    assert_eq!(run.workflow_name, Some("Build".to_string()));
    assert_eq!(run.branch, Some("feature".to_string()));
    assert_eq!(run.status, RunStatus::Queued);
    assert_eq!(run.conclusion, None);
    assert_eq!(run.head_sha, "deadbeef");
    assert_eq!(run.display_title, "Build #42");
}

/// Struct-update merge: an envelope without `workflow_name` preserves the existing value.
#[test]
fn run_struct_update_merge_preserves_workflow_name() {
    let now = fixed_test_timestamp();
    let run_id = RunId(1004);

    // Build an existing run with a workflow_name already set.
    let existing = WorkflowRun {
        id: run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: Some("My Workflow".to_string()),
        workflow_path: Some(".github/workflows/my.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: None,
        event: "push".to_string(),
        display_title: "Run".to_string(),
        status: RunStatus::Queued,
        conclusion: None,
        html_url: "https://example.com".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
    };

    // Envelope with workflow_name = None (common for in_progress events).
    let envelope = RunEventEnvelope {
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        workflow_name: None,
        workflow_path: None,
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: None,
        trigger_event: "push".to_string(),
        display_title: "Run".to_string(),
        html_url: "https://example.com".to_string(),
        created_at: now,
        run_started_at: None,
        updated_at: now,
        action: RunEvent::InProgress,
    };

    let updated = apply_run_event(Some(existing), envelope).expect("transition should succeed");
    assert_eq!(updated.status, RunStatus::InProgress);
    // Existing workflow_name must be preserved via .or()
    assert_eq!(updated.workflow_name, Some("My Workflow".to_string()));
    assert_eq!(
        updated.workflow_path,
        Some(".github/workflows/my.yml".to_string())
    );
}

// ===== apply_job_event =====

/// Job snapshot replacement: updating a 3-step job with a 2-step envelope
/// produces a job with exactly 2 steps (not 5).
#[test]
fn job_snapshot_step_replacement() {
    let now = fixed_test_timestamp();
    let job_id = JobId(2001);
    let run_id = RunId(200);

    let three_steps: Vec<Step> = (1..=3)
        .map(|n| Step {
            number: n,
            name: format!("Step {n}"),
            status: StepStatus::Completed,
            conclusion: None,
            started_at: None,
            completed_at: None,
        })
        .collect();

    let initial_envelope = JobEventEnvelope {
        job_id,
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        name: "build".to_string(),
        created_at: now,
        started_at: None,
        completed_at: None,
        action: JobEvent::Queued {
            labels: vec![],
            steps: three_steps,
        },
    };

    // Create the job first.
    let job_with_3_steps =
        apply_job_event(None, initial_envelope).expect("first-sight job should succeed");
    assert_eq!(job_with_3_steps.steps.len(), 3);

    // Now apply an InProgress envelope with only 2 steps.
    let two_steps: Vec<Step> = (1..=2)
        .map(|n| Step {
            number: n,
            name: format!("Step {n}"),
            status: StepStatus::InProgress,
            conclusion: None,
            started_at: Some(now),
            completed_at: None,
        })
        .collect();

    let update_envelope = JobEventEnvelope {
        job_id,
        run_id,
        org: "octocat".to_string(),
        repo: "Hello-World".to_string(),
        name: "build".to_string(),
        created_at: now,
        started_at: Some(now),
        completed_at: None,
        action: JobEvent::InProgress {
            runner: None,
            labels: vec![],
            steps: two_steps,
        },
    };

    let updated =
        apply_job_event(Some(job_with_3_steps), update_envelope).expect("update should succeed");
    assert_eq!(
        updated.steps.len(),
        2,
        "steps should be replaced, not appended"
    );
}

// ===== is_evictable =====

/// A completed job whose TTL has elapsed is evictable.
#[test]
fn evictable_completed_job_past_ttl() {
    let completed_at = fixed_test_timestamp() - chrono::Duration::hours(2);
    let job = build_completed_job(Some(completed_at));
    let now = fixed_test_timestamp();
    let ttl = Duration::from_mins(30);

    assert!(
        is_evictable(&job, now, ttl),
        "completed job older than TTL should be evictable"
    );
}

/// An active (`InProgress`) job is never evictable regardless of age.
#[test]
fn not_evictable_active_job() {
    let now = fixed_test_timestamp();
    let job = Job {
        id: JobId(3001),
        name: "build".to_string(),
        run_id: RunId(300),
        status: JobStatus::InProgress,
        conclusion: None,
        runner: None,
        labels: vec![],
        steps: vec![],
        created_at: now - chrono::Duration::hours(2),
        started_at: Some(now - chrono::Duration::hours(2)),
        completed_at: None,
    };
    let ttl = Duration::from_mins(30);

    assert!(
        !is_evictable(&job, now, ttl),
        "active job should never be evictable"
    );
}

/// A completed job with no `completed_at` timestamp is not evictable
/// (we have no basis to determine TTL expiry).
#[test]
fn not_evictable_completed_job_without_completed_at() {
    let job = build_completed_job(None);
    let now = fixed_test_timestamp();
    let ttl = Duration::from_mins(30);

    assert!(
        !is_evictable(&job, now, ttl),
        "completed job without completed_at should not be evictable"
    );
}

// ===== Helpers =====

fn build_completed_job(completed_at: Option<chrono::DateTime<Utc>>) -> Job {
    let now = fixed_test_timestamp();
    Job {
        id: JobId(9001),
        name: "completed-job".to_string(),
        run_id: RunId(900),
        status: JobStatus::Completed,
        conclusion: Some(JobConclusion::Success),
        runner: None,
        labels: vec![],
        steps: vec![],
        created_at: now - chrono::Duration::hours(3),
        started_at: Some(now - chrono::Duration::hours(3)),
        completed_at,
    }
}
