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
use crate::test_support::{make_job, make_job_event, make_workflow_run};
use chrono::Utc;

// ===== apply_run_event =====

/// A completed run cannot transition back to `InProgress`.
#[test]
fn run_forward_only_rejects_completed_to_in_progress() {
    let now = fixed_test_timestamp();
    let run_id = RunId(1001);
    let completed_run = WorkflowRun {
        id: run_id,
        status: RunStatus::Completed,
        conclusion: Some(RunConclusion::Success),
        run_started_at: Some(now),
        completed_at: Some(now),
        ..make_workflow_run()
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
    let run_id = RunId(1003);
    // Distinct-from-default values so the assertions prove the envelope's fields
    // (not the builder's defaults) flow through to the created run.
    let envelope = RunEventEnvelope {
        org: "myorg".to_string(),
        repo: "myrepo".to_string(),
        workflow_name: Some("Build".to_string()),
        workflow_path: Some(".github/workflows/build.yml".to_string()),
        branch: Some("feature".to_string()),
        head_sha: "deadbeef".to_string(),
        commit_message: Some("Add feature".to_string()),
        display_title: "Build #42".to_string(),
        html_url: "https://github.com/myorg/myrepo/actions/runs/1003".to_string(),
        ..make_run_event(run_id, RunEvent::Requested)
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
    let run_id = RunId(1004);

    // Build an existing run with a workflow_name already set.
    let existing = WorkflowRun {
        id: run_id,
        workflow_name: Some("My Workflow".to_string()),
        workflow_path: Some(".github/workflows/my.yml".to_string()),
        ..make_workflow_run()
    };

    // Envelope with workflow_name = None (common for in_progress events).
    let envelope = RunEventEnvelope {
        workflow_name: None,
        workflow_path: None,
        ..make_run_event(run_id, RunEvent::InProgress)
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

/// `apply_run_event` on the `Completed` transition sets `completed_at` from
/// the envelope. First-sight creation captures the envelope's
/// `completed_at` directly.
#[test]
fn run_completed_sets_completed_at_on_first_sight() {
    let now = fixed_test_timestamp();
    let run_id = RunId(1005);
    let mut envelope = make_run_event(
        run_id,
        RunEvent::Completed {
            conclusion: RunConclusion::Success,
        },
    );
    envelope.completed_at = Some(now);

    let run = apply_run_event(None, envelope).expect("first-sight should succeed");
    assert_eq!(run.status, RunStatus::Completed);
    assert_eq!(run.completed_at, Some(now));
}

/// First-sight creation captures the envelope's `repo_id` directly.
#[test]
fn run_first_sight_populates_repo_id() {
    let run_id = RunId(2001);
    let envelope = make_run_event(run_id, RunEvent::Requested);

    let run = apply_run_event(None, envelope.clone()).expect("first-sight should succeed");
    assert_eq!(run.repo_id, envelope.repo_id);
}

/// A subsequent event carrying the same `repo_id` leaves it unchanged.
#[test]
fn run_repo_id_carried_forward_on_update() {
    let run_id = RunId(2002);
    let existing = apply_run_event(None, make_run_event(run_id, RunEvent::Requested))
        .expect("first-sight should succeed");
    assert!(existing.repo_id.is_some());

    let updated = apply_run_event(
        Some(existing.clone()),
        make_run_event(run_id, RunEvent::InProgress),
    )
    .expect("update should succeed");
    assert_eq!(updated.repo_id, existing.repo_id);
}

/// Self-heal: a legacy row with `repo_id: None` is promoted to `Some` the
/// first time an update event carries a repo id.
#[test]
fn run_repo_id_self_heals_from_none() {
    let run_id = RunId(2003);
    let legacy_row = WorkflowRun {
        id: run_id,
        repo_id: None,
        ..make_workflow_run()
    };

    let envelope = make_run_event(run_id, RunEvent::InProgress);
    let repo_id = envelope.repo_id;
    let updated = apply_run_event(Some(legacy_row), envelope).expect("update should succeed");

    assert_eq!(updated.repo_id, repo_id);
}

/// An envelope with no `repo_id` of its own (e.g. a staleness-sweep-
/// synthesized completion) must never erase an already-known `repo_id`.
#[test]
fn run_repo_id_never_regresses_to_none() {
    let run_id = RunId(2004);
    let known_repo_id = Some(crate::types::RepoId(555));
    let existing = WorkflowRun {
        id: run_id,
        repo_id: known_repo_id,
        ..make_workflow_run()
    };

    let envelope = RunEventEnvelope {
        repo_id: None,
        ..make_run_event(run_id, RunEvent::InProgress)
    };
    let updated = apply_run_event(Some(existing), envelope).expect("update should succeed");

    assert_eq!(updated.repo_id, known_repo_id);
}

/// `envelope.completed_at.or(existing.completed_at)` preserves the existing
/// timestamp when a subsequent event arrives with no `completed_at`. This is
/// the protective shape against losing a recorded terminal moment when an
/// out-of-order replay (e.g., a fixture or a late non-completed event) does
/// not carry the field.
#[test]
fn run_completed_at_preserved_when_envelope_lacks_it() {
    let first = fixed_test_timestamp();
    let run_id = RunId(1006);

    let mut env1 = make_run_event(
        run_id,
        RunEvent::Completed {
            conclusion: RunConclusion::Success,
        },
    );
    env1.completed_at = Some(first);
    let first_completed = apply_run_event(None, env1).expect("first-sight should succeed");
    assert_eq!(first_completed.completed_at, Some(first));

    // Subsequent same-state replay with no completed_at — the .or() shape must
    // keep the existing value.
    let mut env2 = make_run_event(
        run_id,
        RunEvent::Completed {
            conclusion: RunConclusion::Success,
        },
    );
    env2.completed_at = None;
    let replayed = apply_run_event(Some(first_completed), env2).expect("idempotent should succeed");
    assert_eq!(replayed.completed_at, Some(first));
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

    let initial_envelope = make_job_event(
        job_id,
        run_id,
        "octocat",
        "Hello-World",
        JobEvent::Queued {
            labels: vec![],
            steps: three_steps,
        },
    );

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
        started_at: Some(now),
        ..make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::InProgress {
                runner: None,
                labels: vec![],
                steps: two_steps,
            },
        )
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
        created_at: now - chrono::Duration::hours(2),
        started_at: Some(now - chrono::Duration::hours(2)),
        ..make_job()
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

// ===== is_stale_job =====

/// A non-terminal job whose last activity predates the threshold is stale.
#[test]
fn stale_job_past_threshold() {
    let now = fixed_test_timestamp();
    let job = Job {
        status: JobStatus::InProgress,
        created_at: now - chrono::Duration::hours(72),
        started_at: Some(now - chrono::Duration::hours(50)),
        ..make_job()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        is_stale_job(&job, now, threshold),
        "non-terminal job past the threshold should be stale"
    );
}

/// A non-terminal job whose last activity is within the threshold is not stale.
#[test]
fn not_stale_job_within_threshold() {
    let now = fixed_test_timestamp();
    let job = Job {
        status: JobStatus::InProgress,
        created_at: now - chrono::Duration::hours(72),
        started_at: Some(now - chrono::Duration::hours(1)),
        ..make_job()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        !is_stale_job(&job, now, threshold),
        "job with recent activity should not be stale"
    );
}

/// A `Completed` job is never stale, regardless of age.
#[test]
fn not_stale_completed_job() {
    let now = fixed_test_timestamp();
    let job = build_completed_job(Some(now - chrono::Duration::hours(100)));
    let threshold = Duration::from_hours(48);

    assert!(
        !is_stale_job(&job, now, threshold),
        "completed job should never be stale"
    );
}

/// `started_at` absent falls back to `created_at` for the activity signal.
#[test]
fn stale_job_uses_created_at_when_started_at_absent() {
    let now = fixed_test_timestamp();
    let job = Job {
        status: JobStatus::Queued,
        created_at: now - chrono::Duration::hours(72),
        started_at: None,
        ..make_job()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        is_stale_job(&job, now, threshold),
        "queued job with no started_at should use created_at as the activity signal"
    );
}

/// A `Waiting` job is never stale, however old — `JobStatus::transition_to`
/// has no `Waiting -> Completed` arm, so it can never be force-completed.
#[test]
fn not_stale_waiting_job_regardless_of_age() {
    let now = fixed_test_timestamp();
    let job = Job {
        status: JobStatus::Waiting,
        created_at: now - chrono::Duration::hours(100),
        started_at: None,
        ..make_job()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        !is_stale_job(&job, now, threshold),
        "a Waiting job can never transition to Completed and must never be a sweep candidate"
    );
}

// ===== is_stale_run =====

/// A non-terminal run past the threshold with no non-terminal jobs is stale.
#[test]
fn stale_run_past_threshold_no_live_jobs() {
    let now = fixed_test_timestamp();
    let run = WorkflowRun {
        status: RunStatus::InProgress,
        updated_at: now - chrono::Duration::hours(72),
        ..make_workflow_run()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        is_stale_run(&run, false, now, threshold),
        "non-terminal run past the threshold with no live jobs should be stale"
    );
}

/// A run with a live non-terminal job is shielded even past the threshold —
/// the `has_non_terminal_jobs` guard prevents falsely sweeping the parent of
/// a legitimately long-running self-hosted job.
#[test]
fn not_stale_run_shielded_by_live_job() {
    let now = fixed_test_timestamp();
    let run = WorkflowRun {
        status: RunStatus::InProgress,
        updated_at: now - chrono::Duration::hours(72),
        ..make_workflow_run()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        !is_stale_run(&run, true, now, threshold),
        "run with a live non-terminal job should be shielded from the sweep"
    );
}

/// A `Completed` run is never stale, regardless of age.
#[test]
fn not_stale_completed_run() {
    let now = fixed_test_timestamp();
    let run = WorkflowRun {
        status: RunStatus::Completed,
        conclusion: Some(RunConclusion::Success),
        updated_at: now - chrono::Duration::hours(100),
        completed_at: Some(now - chrono::Duration::hours(100)),
        ..make_workflow_run()
    };
    let threshold = Duration::from_hours(48);

    assert!(
        !is_stale_run(&run, false, now, threshold),
        "completed run should never be stale"
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
        created_at: now - chrono::Duration::hours(3),
        started_at: Some(now - chrono::Duration::hours(3)),
        completed_at,
        ..make_job()
    }
}
