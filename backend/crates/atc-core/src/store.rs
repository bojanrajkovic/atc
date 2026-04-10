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

use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use crate::job::{InvalidJobTransition, Job, JobStatus};
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
    /// Used in Phase 5 for TTL eviction.
    #[allow(dead_code)]
    clock: Arc<dyn Clock>,
    /// How long to retain completed jobs before eviction.
    /// Used in Phase 5 for TTL eviction.
    #[allow(dead_code)]
    completed_ttl: Duration,
}

/// Mutable state behind the `RwLock`.
struct StateData {
    /// Primary map of runs by ID.
    runs: HashMap<RunId, WorkflowRun>,
    /// Primary map of jobs by ID.
    jobs: HashMap<JobId, Job>,
    /// Jobs grouped by parent run.
    jobs_by_run: HashMap<RunId, HashSet<JobId>>,
    /// Jobs grouped by repository.
    jobs_by_repo: HashMap<RepoKey, HashSet<JobId>>,
}

/// Result of a repository-scoped query.
///
/// Contains owned snapshots — callers can hold these without
/// blocking the store's `RwLock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    /// Workflow runs that have jobs in the queried repositories.
    pub runs: Vec<WorkflowRun>,
    /// Jobs in the queried repositories.
    pub jobs: Vec<Job>,
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

    /// Ingest a job event, creating or updating the corresponding [`Job`].
    ///
    /// - Unknown job IDs create a new entry in whatever status the event
    ///   implies (out-of-order tolerance, AC3.5).
    /// - Known jobs apply the state machine transition.
    /// - Steps are fully replaced on every event (snapshot semantics, AC3.4).
    /// - Secondary indexes (`jobs_by_run`, `jobs_by_repo`) are updated on
    ///   job creation (AC3.3).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::InvalidJobTransition`] if the event implies
    /// a backward status transition on an existing job.
    pub async fn apply_job_event(&self, envelope: JobEventEnvelope) -> Result<(), StoreError> {
        let mut state = self.state.write().await;

        let (target_status, conclusion, runner, labels, steps) = match envelope.action {
            JobEvent::Queued { labels, steps } => {
                (JobStatus::Queued, None, None, labels, steps)
            }
            JobEvent::InProgress { runner, labels, steps } => {
                (JobStatus::InProgress, None, Some(runner), labels, steps)
            }
            JobEvent::Completed {
                conclusion,
                runner,
                labels,
                steps,
            } => (JobStatus::Completed, Some(conclusion), runner, labels, steps),
        };

        let job_id = envelope.job_id;
        let run_id = envelope.run_id;

        // Phase 1: Validate transition before touching state.
        if let Some(existing) = state.jobs.get(&job_id) {
            existing.status.transition_to(target_status)?;
        }

        // Phase 2: Build the new value and insert.
        let is_new = !state.jobs.contains_key(&job_id);

        let job = match state.jobs.remove(&job_id) {
            Some(existing) => Job {
                status: target_status,
                conclusion: conclusion.or(existing.conclusion),
                runner: runner.or(existing.runner),
                labels,
                steps, // Snapshot replacement (AC3.4)
                started_at: envelope.started_at.or(existing.started_at),
                completed_at: envelope.completed_at.or(existing.completed_at),
                ..existing // id, name, run_id, created_at unchanged
            },
            None => Job {
                id: job_id,
                name: envelope.name,
                run_id,
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

        state.jobs.insert(job_id, job);

        // Update secondary indexes on first sight (AC3.3)
        if is_new {
            let repo_key = RepoKey::new(envelope.org, envelope.repo);
            state.jobs_by_run.entry(run_id).or_default().insert(job_id);
            state
                .jobs_by_repo
                .entry(repo_key)
                .or_default()
                .insert(job_id);
        }

        Ok(())
    }

    /// Returns a snapshot of a job by ID, if it exists.
    pub async fn get_job(&self, job_id: &JobId) -> Option<Job> {
        self.state.read().await.jobs.get(job_id).cloned()
    }

    /// Returns the set of job IDs for a given run.
    pub async fn jobs_for_run(&self, run_id: &RunId) -> HashSet<JobId> {
        self.state
            .read()
            .await
            .jobs_by_run
            .get(run_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the set of job IDs for a given repository.
    pub async fn jobs_for_repo(&self, repo_key: &RepoKey) -> HashSet<JobId> {
        self.state
            .read()
            .await
            .jobs_by_repo
            .get(repo_key)
            .cloned()
            .unwrap_or_default()
    }

    /// Query jobs and their parent runs, filtered by repository.
    ///
    /// Returns owned snapshots that can be held independently of the
    /// store's lock. Only jobs belonging to the provided repositories
    /// are included; their parent runs are collected automatically.
    ///
    /// An empty `repos` slice returns an empty result.
    pub async fn query_by_repos(&self, repos: &[RepoKey]) -> QueryResult {
        let state = self.state.read().await;

        let mut jobs = Vec::new();
        let mut run_ids = HashSet::new();

        for repo in repos {
            if let Some(job_ids) = state.jobs_by_repo.get(repo) {
                for job_id in job_ids {
                    if let Some(job) = state.jobs.get(job_id) {
                        run_ids.insert(job.run_id);
                        jobs.push(job.clone());
                    }
                }
            }
        }

        let runs: Vec<WorkflowRun> = run_ids
            .iter()
            .filter_map(|id| state.runs.get(id).cloned())
            .collect();

        QueryResult { runs, jobs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use crate::job::JobConclusion;
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

    /// Helper to build a JobEventEnvelope with sensible defaults.
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

    // ===== Job Event Tests (Task 3) =====

    #[tokio::test]
    async fn test_ac3_2_create_job_from_queued() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let job_id = JobId(100);
        let run_id = RunId(10);

        let envelope = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.id, job_id);
        assert_eq!(job.run_id, run_id);
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.name, "Test Job");
        assert_eq!(job.conclusion, None);
        assert_eq!(job.runner, None);
        assert_eq!(job.labels, vec!["linux".to_string()]);
    }

    #[tokio::test]
    async fn test_ac3_2_update_job_to_in_progress() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let job_id = JobId(101);
        let run_id = RunId(11);

        // Create with Queued
        let envelope1 = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope1).await.unwrap();

        // Update to InProgress with runner
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let envelope2 = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::InProgress {
                runner: runner.clone(),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope2).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::InProgress);
        assert_eq!(job.runner, Some(runner));
        assert_eq!(job.conclusion, None);
    }

    #[tokio::test]
    async fn test_ac3_3_jobs_by_run() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(20);
        let job_id_1 = JobId(201);
        let job_id_2 = JobId(202);

        // Create two jobs for the same run
        let envelope1 = make_job_event(
            job_id_1,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope1).await.unwrap();

        let envelope2 = make_job_event(
            job_id_2,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope2).await.unwrap();

        let jobs = store.jobs_for_run(&run_id).await;
        assert_eq!(jobs.len(), 2);
        assert!(jobs.contains(&job_id_1));
        assert!(jobs.contains(&job_id_2));

        // Create a job for a different run
        let run_id_2 = RunId(21);
        let job_id_3 = JobId(203);
        let envelope3 = make_job_event(
            job_id_3,
            run_id_2,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope3).await.unwrap();

        // Verify first run still only has 2 jobs
        let jobs_run1 = store.jobs_for_run(&run_id).await;
        assert_eq!(jobs_run1.len(), 2);

        // Verify second run has 1 job
        let jobs_run2 = store.jobs_for_run(&run_id_2).await;
        assert_eq!(jobs_run2.len(), 1);
        assert!(jobs_run2.contains(&job_id_3));
    }

    #[tokio::test]
    async fn test_ac3_3_jobs_by_repo() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let run_id = RunId(30);

        // Create jobs for different repos
        let job_id_1 = JobId(301);
        let repo_a_key = RepoKey::new("octocat", "repo-a");
        let envelope1 = make_job_event(
            job_id_1,
            run_id,
            "octocat",
            "repo-a",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope1).await.unwrap();

        let job_id_2 = JobId(302);
        let repo_b_key = RepoKey::new("octocat", "repo-b");
        let envelope2 = make_job_event(
            job_id_2,
            run_id,
            "octocat",
            "repo-b",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope2).await.unwrap();

        // Verify jobs are separated by repo
        let jobs_a = store.jobs_for_repo(&repo_a_key).await;
        let jobs_b = store.jobs_for_repo(&repo_b_key).await;

        assert_eq!(jobs_a.len(), 1);
        assert!(jobs_a.contains(&job_id_1));

        assert_eq!(jobs_b.len(), 1);
        assert!(jobs_b.contains(&job_id_2));
    }

    #[tokio::test]
    async fn test_ac3_4_steps_snapshot_replacement() {
        use crate::job::{Step, StepStatus};

        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let job_id = JobId(400);
        let run_id = RunId(40);

        // Create with 2 steps
        let steps_1 = vec![
            Step {
                number: 1,
                name: "Step A".to_string(),
                status: StepStatus::Queued,
                conclusion: None,
                started_at: None,
                completed_at: None,
            },
            Step {
                number: 2,
                name: "Step B".to_string(),
                status: StepStatus::Queued,
                conclusion: None,
                started_at: None,
                completed_at: None,
            },
        ];

        let envelope1 = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec![],
                steps: steps_1,
            },
        );
        store.apply_job_event(envelope1).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.steps.len(), 2);

        // Update to InProgress with 3 steps (should replace, not append)
        let steps_2 = vec![
            Step {
                number: 1,
                name: "Step A".to_string(),
                status: StepStatus::InProgress,
                conclusion: None,
                started_at: Some(start_time),
                completed_at: None,
            },
            Step {
                number: 2,
                name: "Step B".to_string(),
                status: StepStatus::Queued,
                conclusion: None,
                started_at: None,
                completed_at: None,
            },
            Step {
                number: 3,
                name: "Step C".to_string(),
                status: StepStatus::Queued,
                conclusion: None,
                started_at: None,
                completed_at: None,
            },
        ];

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };

        let envelope2 = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::InProgress {
                runner,
                labels: vec![],
                steps: steps_2,
            },
        );
        store.apply_job_event(envelope2).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.steps.len(), 3); // Should be 3, not 5 (replaced, not appended)
        assert_eq!(job.steps[0].name, "Step A");
        assert_eq!(job.steps[1].name, "Step B");
        assert_eq!(job.steps[2].name, "Step C");
    }

    #[tokio::test]
    async fn test_ac3_5_first_sight_completed_job() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let job_id = JobId(500);
        let run_id = RunId(50);

        // Send a Completed event for an unknown job (out-of-order delivery)
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };

        let envelope = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.conclusion, Some(JobConclusion::Success));
    }

    #[tokio::test]
    async fn test_ac3_6_idempotent_queued_twice() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));
        let job_id = JobId(600);
        let run_id = RunId(60);

        let envelope = make_job_event(
            job_id,
            run_id,
            "octocat",
            "Hello-World",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );

        // Send Queued twice
        store.apply_job_event(envelope.clone()).await.unwrap();
        store.apply_job_event(envelope).await.unwrap();

        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.id, job_id);
    }

    // ===== Task 1: Repository-Scoped Queries =====

    #[tokio::test]
    async fn test_ac4_1_query_returns_only_queried_repos() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        // Create a run
        let run_id = RunId(700);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create jobs in two different repos
        let job_id_alpha = JobId(701);
        let job_id_beta = JobId(702);

        let alpha_envelope = make_job_event(
            job_id_alpha,
            run_id,
            "org",
            "alpha",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(alpha_envelope).await.unwrap();

        let beta_envelope = make_job_event(
            job_id_beta,
            run_id,
            "org",
            "beta",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(beta_envelope).await.unwrap();

        // Query only alpha repo
        let alpha_repo = RepoKey::new("org", "alpha");
        let result = store.query_by_repos(&[alpha_repo]).await;

        // Verify only alpha's job is returned
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.jobs[0].id, job_id_alpha);
        assert_eq!(result.runs.len(), 1);
        assert_eq!(result.runs[0].id, run_id);
    }

    #[tokio::test]
    async fn test_ac4_2_query_returns_owned_snapshots() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        // Create a run and job
        let run_id = RunId(800);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        let job_id = JobId(801);
        let job_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Query and hold the result
        let repo = RepoKey::new("org", "repo");
        let result = store.query_by_repos(&[repo]).await;

        // Verify we can use the result without holding the store lock
        // (This is implicitly tested by the API returning owned types,
        // but we verify the data is present and usable)
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.runs.len(), 1);
        assert_eq!(result.jobs[0].id, job_id);
        assert_eq!(result.runs[0].id, run_id);
    }

    #[tokio::test]
    async fn test_ac4_5_empty_repos_returns_empty_result() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        // Create a run and job
        let run_id = RunId(900);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        let job_id = JobId(901);
        let job_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Query with empty repos slice
        let result = store.query_by_repos(&[]).await;

        // Verify empty result
        assert_eq!(result.jobs.len(), 0);
        assert_eq!(result.runs.len(), 0);
    }

    #[tokio::test]
    async fn test_ac4_6_multi_repo_query_isolation() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        // Create a run
        let run_id = RunId(1000);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create jobs in repo A
        let job_a1 = JobId(1001);
        let job_a2 = JobId(1002);

        let envelope_a1 = make_job_event(
            job_a1,
            run_id,
            "org",
            "repoA",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_a1).await.unwrap();

        let envelope_a2 = make_job_event(
            job_a2,
            run_id,
            "org",
            "repoA",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_a2).await.unwrap();

        // Create jobs in repo B
        let job_b1 = JobId(1003);
        let job_b2 = JobId(1004);

        let envelope_b1 = make_job_event(
            job_b1,
            run_id,
            "org",
            "repoB",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_b1).await.unwrap();

        let envelope_b2 = make_job_event(
            job_b2,
            run_id,
            "org",
            "repoB",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_b2).await.unwrap();

        // Query both repos
        let repo_a = RepoKey::new("org", "repoA");
        let repo_b = RepoKey::new("org", "repoB");
        let result = store.query_by_repos(&[repo_a.clone(), repo_b.clone()]).await;

        // Verify both repos' jobs are returned (4 jobs total)
        assert_eq!(result.jobs.len(), 4);

        // Verify all expected job IDs are present
        let job_ids: HashSet<JobId> = result.jobs.iter().map(|job| job.id).collect();
        assert!(job_ids.contains(&job_a1), "Job A1 should be in result");
        assert!(job_ids.contains(&job_a2), "Job A2 should be in result");
        assert!(job_ids.contains(&job_b1), "Job B1 should be in result");
        assert!(job_ids.contains(&job_b2), "Job B2 should be in result");

        // Verify repo A query alone returns only repo A's jobs
        let result_a = store.query_by_repos(&[repo_a]).await;
        assert_eq!(result_a.jobs.len(), 2);
        let job_ids_a: HashSet<JobId> = result_a.jobs.iter().map(|job| job.id).collect();
        assert!(job_ids_a.contains(&job_a1));
        assert!(job_ids_a.contains(&job_a2));
        assert!(!job_ids_a.contains(&job_b1));
        assert!(!job_ids_a.contains(&job_b2));

        // Verify repo B query alone returns only repo B's jobs
        let result_b = store.query_by_repos(&[repo_b]).await;
        assert_eq!(result_b.jobs.len(), 2);
        let job_ids_b: HashSet<JobId> = result_b.jobs.iter().map(|job| job.id).collect();
        assert!(!job_ids_b.contains(&job_a1));
        assert!(!job_ids_b.contains(&job_a2));
        assert!(job_ids_b.contains(&job_b1));
        assert!(job_ids_b.contains(&job_b2));

        // Verify run is included
        assert_eq!(result.runs.len(), 1);
        assert_eq!(result.runs[0].id, run_id);
    }

    #[tokio::test]
    async fn test_ac4_query_includes_parent_runs() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        // Create a run
        let run_id = RunId(1100);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Add a job to the run
        let job_id = JobId(1101);
        let job_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Query by repo
        let repo = RepoKey::new("org", "repo");
        let result = store.query_by_repos(&[repo]).await;

        // Verify both job and run are present
        assert_eq!(result.jobs.len(), 1);
        assert_eq!(result.runs.len(), 1);
        assert_eq!(result.jobs[0].run_id, run_id);
        assert_eq!(result.runs[0].id, run_id);
    }
}
