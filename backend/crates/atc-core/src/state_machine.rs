//! Pure state-transition functions for domain entities.
//!
//! This module provides three free functions:
//!
//! - [`apply_run_event`] — produces an updated [`WorkflowRun`] from an event envelope.
//! - [`apply_job_event`] — produces an updated [`Job`] from an event envelope.
//! - [`is_evictable`] — predicate for TTL eviction of completed jobs.
//!
//! All functions are synchronous and side-effect-free; locking, sequencing,
//! indexing, and broadcasting are the responsibility of the caller (typically
//! the `InMemoryStore` in `atc-store-mem` or `PgStore` in the in-tree PG
//! backend).

use std::fmt;

use crate::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use crate::job::{InvalidJobTransition, Job, JobStatus};
use crate::run::{InvalidRunTransition, RunStatus, WorkflowRun};

/// Errors that can occur during state machine operations.
#[derive(Debug)]
pub enum StateMachineError {
    /// A run status transition was invalid.
    InvalidRunTransition(InvalidRunTransition),
    /// A job status transition was invalid.
    InvalidJobTransition(InvalidJobTransition),
}

impl fmt::Display for StateMachineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRunTransition(e) => write!(f, "{e}"),
            Self::InvalidJobTransition(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StateMachineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRunTransition(e) => Some(e),
            Self::InvalidJobTransition(e) => Some(e),
        }
    }
}

impl From<InvalidRunTransition> for StateMachineError {
    fn from(e: InvalidRunTransition) -> Self {
        Self::InvalidRunTransition(e)
    }
}

impl From<InvalidJobTransition> for StateMachineError {
    fn from(e: InvalidJobTransition) -> Self {
        Self::InvalidJobTransition(e)
    }
}

/// Apply a run event to an optional existing run, returning the updated run.
///
/// - `existing = None` — first-sight creation; all fields come from the envelope.
/// - `existing = Some(run)` — existing run is updated; unchanged fields are
///   carried forward via struct-update syntax. Same-status events are idempotent.
///
/// No locks, no async, no side effects beyond the returned value.
///
/// # Errors
///
/// Returns [`StateMachineError::InvalidRunTransition`] if the event implies
/// a status transition that the state machine rejects (e.g.,
/// `Completed` -> `InProgress`).
pub fn apply_run_event(
    existing: Option<WorkflowRun>,
    envelope: RunEventEnvelope,
) -> Result<WorkflowRun, StateMachineError> {
    let (target_status, conclusion) = match &envelope.action {
        RunEvent::Requested => (RunStatus::Queued, None),
        RunEvent::InProgress => (RunStatus::InProgress, None),
        RunEvent::Completed { conclusion } => (RunStatus::Completed, Some(*conclusion)),
    };

    // Validate: check transition before touching state.
    if let Some(ref run) = existing {
        run.status.transition_to(target_status)?;
    }

    let run = match existing {
        Some(existing) => WorkflowRun {
            status: target_status,
            conclusion: conclusion.or(existing.conclusion),
            workflow_name: envelope.workflow_name.or(existing.workflow_name),
            workflow_path: envelope.workflow_path.or(existing.workflow_path),
            branch: envelope.branch,
            head_sha: envelope.head_sha,
            commit_message: envelope.commit_message,
            display_title: envelope.display_title,
            html_url: envelope.html_url,
            run_started_at: envelope.run_started_at.or(existing.run_started_at),
            updated_at: envelope.updated_at,
            // Preserve-first semantics. The run FSM is forward-only (Completed
            // never reverts), so a second arrival on the same run cannot
            // legitimately move `completed_at` backward; preserve the first
            // observation across idempotent replay.
            completed_at: envelope.completed_at.or(existing.completed_at),
            run_attempt: envelope.run_attempt,
            ..existing
        },
        None => WorkflowRun {
            id: envelope.run_id,
            org: envelope.org,
            repo: envelope.repo,
            workflow_name: envelope.workflow_name,
            workflow_path: envelope.workflow_path,
            branch: envelope.branch,
            head_sha: envelope.head_sha,
            commit_message: envelope.commit_message,
            event: envelope.trigger_event,
            display_title: envelope.display_title,
            status: target_status,
            conclusion,
            html_url: envelope.html_url,
            created_at: envelope.created_at,
            run_started_at: envelope.run_started_at,
            updated_at: envelope.updated_at,
            completed_at: envelope.completed_at,
            run_attempt: envelope.run_attempt,
        },
    };

    Ok(run)
}

/// Apply a job event to an optional existing job, returning the updated job.
///
/// - `existing = None` — first-sight creation; all fields come from the envelope.
/// - `existing = Some(job)` — existing job is updated; unchanged fields are
///   carried forward. Steps use snapshot semantics (fully replaced).
///
/// Secondary indexes (`jobs_by_run`, `jobs_by_repo`) are **not** managed here;
/// callers are responsible for index maintenance on first sight.
///
/// No locks, no async, no side effects beyond the returned value.
///
/// # Errors
///
/// Returns [`StateMachineError::InvalidJobTransition`] if the event implies
/// a backward status transition on an existing job.
pub fn apply_job_event(
    existing: Option<Job>,
    envelope: JobEventEnvelope,
) -> Result<Job, StateMachineError> {
    let (target_status, conclusion, runner, labels, steps) = match envelope.action {
        JobEvent::Queued { labels, steps } => (JobStatus::Queued, None, None, labels, steps),
        JobEvent::Waiting { labels, steps } => (JobStatus::Waiting, None, None, labels, steps),
        JobEvent::InProgress {
            runner,
            labels,
            steps,
        } => (JobStatus::InProgress, None, runner, labels, steps),
        JobEvent::Completed {
            conclusion,
            runner,
            labels,
            steps,
        } => (
            JobStatus::Completed,
            Some(conclusion),
            runner,
            labels,
            steps,
        ),
    };

    // Validate: check transition before touching state.
    if let Some(ref job) = existing {
        job.status.transition_to(target_status)?;
    }

    let job = match existing {
        Some(existing) => Job {
            status: target_status,
            conclusion: conclusion.or(existing.conclusion),
            runner: runner.or(existing.runner),
            labels,
            steps, // Snapshot replacement
            started_at: envelope.started_at.or(existing.started_at),
            completed_at: envelope.completed_at.or(existing.completed_at),
            ..existing
        },
        None => Job {
            id: envelope.job_id,
            name: envelope.name,
            run_id: envelope.run_id,
            status: target_status,
            conclusion,
            runner,
            labels,
            steps,
            created_at: envelope.created_at,
            started_at: envelope.started_at,
            completed_at: envelope.completed_at,
        },
    };

    Ok(job)
}

/// Return whether a job is eligible for eviction.
///
/// A job is evictable when:
/// - its status is `Completed`,
/// - `completed_at` is set, and
/// - `now - completed_at > ttl`.
///
/// Active jobs (`Queued`, `Waiting`, `InProgress`) and completed jobs without
/// a `completed_at` timestamp are never evictable.
#[must_use]
pub fn is_evictable(
    job: &Job,
    now: chrono::DateTime<chrono::Utc>,
    ttl: std::time::Duration,
) -> bool {
    let ttl_delta = chrono::TimeDelta::from_std(ttl).unwrap_or(chrono::TimeDelta::MAX);
    job.status == JobStatus::Completed
        && job
            .completed_at
            .is_some_and(|t| now.signed_duration_since(t) > ttl_delta)
}

#[cfg(test)]
mod tests;
