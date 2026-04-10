//! In-memory state store for domain entities.
//!
//! The [`StateStore`] ingests domain events, maintains current state
//! with secondary indexes, and supports configurable TTL eviction of
//! completed entries.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;

use crate::clock::Clock;
use crate::event::{RunEvent, RunEventEnvelope};
use crate::job::{InvalidJobTransition, Job};
use crate::run::{InvalidRunTransition, RunStatus, WorkflowRun};
use crate::types::{JobId, RepoKey, RunId};

/// Errors that can occur during state store operations.
#[derive(Debug)]
pub enum StoreError {
    /// A run status transition was invalid.
    InvalidRunTransition(InvalidRunTransition),
    /// A job status transition was invalid.
    InvalidJobTransition(InvalidJobTransition),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRunTransition(e) => write!(f, "{e}"),
            Self::InvalidJobTransition(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidRunTransition(e) => Some(e),
            Self::InvalidJobTransition(e) => Some(e),
        }
    }
}

impl From<InvalidRunTransition> for StoreError {
    fn from(e: InvalidRunTransition) -> Self {
        Self::InvalidRunTransition(e)
    }
}

impl From<InvalidJobTransition> for StoreError {
    fn from(e: InvalidJobTransition) -> Self {
        Self::InvalidJobTransition(e)
    }
}

/// In-memory state store for workflow runs and jobs.
///
/// Thread-safe via `tokio::sync::RwLock`. Wrap in `Arc` for sharing
/// across async tasks and Axum handlers.
pub struct StateStore {
    state: RwLock<StateData>,
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
    /// How long to retain completed jobs before eviction.
    #[allow(dead_code)]
    completed_ttl: Duration,
}

/// Mutable state behind the `RwLock`.
struct StateData {
    /// Primary map of runs by ID.
    runs: HashMap<RunId, WorkflowRun>,
    /// Primary map of jobs by ID.
    #[allow(dead_code)]
    jobs: HashMap<JobId, Job>,
    /// Jobs grouped by parent run.
    #[allow(dead_code)]
    jobs_by_run: HashMap<RunId, HashSet<JobId>>,
    /// Jobs grouped by repository.
    #[allow(dead_code)]
    jobs_by_repo: HashMap<RepoKey, HashSet<JobId>>,
}

impl StateStore {
    /// Creates a new empty state store.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>, completed_ttl: Duration) -> Self {
        Self {
            state: RwLock::new(StateData {
                runs: HashMap::new(),
                jobs: HashMap::new(),
                jobs_by_run: HashMap::new(),
                jobs_by_repo: HashMap::new(),
            }),
            clock,
            completed_ttl,
        }
    }

    /// Ingest a run event, creating or updating the corresponding
    /// [`WorkflowRun`].
    ///
    /// - Unknown run IDs create a new entry (first-sight).
    /// - Known runs apply the state machine transition.
    /// - Same-status events are idempotent — fields update without error.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidRunTransition`] if the event implies
    /// a status transition that the state machine rejects (e.g.,
    /// `Completed` -> `InProgress`).
    pub async fn apply_run_event(&self, envelope: RunEventEnvelope) -> Result<(), StoreError> {
        let mut state = self.state.write().await;

        let (target_status, conclusion) = match &envelope.action {
            RunEvent::Requested => (RunStatus::Queued, None),
            RunEvent::InProgress => (RunStatus::InProgress, None),
            RunEvent::Completed { conclusion } => (RunStatus::Completed, Some(*conclusion)),
        };

        // Phase 1: Validate transition before touching state.
        // If this fails, the map is untouched.
        if let Some(existing) = state.runs.get(&envelope.run_id) {
            existing.status.transition_to(target_status)?;
        }

        // Phase 2: Build the new value and insert.
        // For updates, remove the old value and use struct update syntax
        // to carry forward unchanged fields.
        let run = match state.runs.remove(&envelope.run_id) {
            Some(existing) => WorkflowRun {
                status: target_status,
                conclusion: conclusion.or(existing.conclusion),
                workflow_name: envelope.workflow_name,
                workflow_path: envelope.workflow_path,
                branch: envelope.branch,
                head_sha: envelope.head_sha,
                commit_message: envelope.commit_message,
                display_title: envelope.display_title,
                html_url: envelope.html_url,
                run_started_at: envelope.run_started_at.or(existing.run_started_at),
                updated_at: envelope.updated_at,
                ..existing // id, org, repo, event, created_at unchanged
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
            },
        };

        state.runs.insert(run.id, run);
        Ok(())
    }

    /// Returns a snapshot of a run by ID, if it exists.
    pub async fn get_run(&self, run_id: &RunId) -> Option<WorkflowRun> {
        self.state.read().await.runs.get(run_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use crate::run::RunConclusion;
    use chrono::Utc;

    /// Helper to build a RunEventEnvelope with sensible defaults.
    fn make_run_event(
        run_id: RunId,
        action: RunEvent,
    ) -> RunEventEnvelope {
        let now = Utc::now();
        RunEventEnvelope {
            run_id,
            org: "octocat".to_string(),
            repo: "Hello-World".to_string(),
            workflow_name: "CI".to_string(),
            workflow_path: ".github/workflows/ci.yml".to_string(),
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

    #[tokio::test]
    async fn test_ac3_1_create_run_from_requested() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(1);

        let envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(envelope).await.unwrap();

        let run = store.get_run(&run_id).await.expect("run should exist");
        assert_eq!(run.id, run_id);
        assert_eq!(run.status, RunStatus::Queued);
        assert_eq!(run.org, "octocat");
        assert_eq!(run.repo, "Hello-World");
        assert_eq!(run.workflow_name, "CI");
        assert_eq!(run.branch, Some("main".to_string()));
        assert_eq!(run.head_sha, "abc123def456");
        assert_eq!(run.commit_message, Some("Fix bug".to_string()));
        assert_eq!(run.event, "push");
        assert_eq!(run.display_title, "CI Run");
        assert_eq!(run.conclusion, None);
    }

    #[tokio::test]
    async fn test_ac3_1_update_run_to_in_progress() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(2);

        // Create with Requested
        let envelope1 = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(envelope1).await.unwrap();

        // Update to InProgress
        let envelope2 = make_run_event(run_id, RunEvent::InProgress);
        store.apply_run_event(envelope2).await.unwrap();

        let run = store.get_run(&run_id).await.expect("run should exist");
        assert_eq!(run.status, RunStatus::InProgress);
        assert_eq!(run.conclusion, None);
    }

    #[tokio::test]
    async fn test_ac3_1_complete_run_with_conclusion() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(3);

        // Requested
        let envelope1 = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(envelope1).await.unwrap();

        // InProgress
        let envelope2 = make_run_event(run_id, RunEvent::InProgress);
        store.apply_run_event(envelope2).await.unwrap();

        // Completed
        let envelope3 = make_run_event(
            run_id,
            RunEvent::Completed {
                conclusion: RunConclusion::Success,
            },
        );
        store.apply_run_event(envelope3).await.unwrap();

        let run = store.get_run(&run_id).await.expect("run should exist");
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.conclusion, Some(RunConclusion::Success));
    }

    #[tokio::test]
    async fn test_ac3_6_idempotent_requested_twice() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(4);

        let envelope = make_run_event(run_id, RunEvent::Requested);

        // Send Requested twice
        store.apply_run_event(envelope.clone()).await.unwrap();
        store.apply_run_event(envelope).await.unwrap();

        let run = store.get_run(&run_id).await.expect("run should exist");
        assert_eq!(run.status, RunStatus::Queued);
        assert_eq!(run.id, run_id);
    }
}
