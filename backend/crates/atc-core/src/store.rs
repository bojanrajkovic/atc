//! In-memory state store for domain entities.
//!
//! The [`StateStore`] ingests domain events, maintains current state
//! with secondary indexes, and supports configurable TTL eviction of
//! completed entries.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use tokio::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::clock::Clock;
use crate::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
use crate::job::{InvalidJobTransition, Job, JobStatus};
use crate::run::{InvalidRunTransition, RunStatus, WorkflowRun};
use crate::types::{JobId, LabelSet, RepoKey, RunId};

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
    /// Clock for determining current time during eviction.
    clock: Arc<dyn Clock>,
    /// How long to retain completed jobs before eviction.
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

/// Derived runner pool statistics.
///
/// Computed on read from live job state — not stored separately.
/// Each entry represents a unique label set with aggregated counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerPoolStats {
    /// The set of labels identifying this pool.
    pub labels: LabelSet,
    /// Number of jobs queued for this label set.
    pub queued: usize,
    /// Number of jobs running on runners with this label set.
    pub running: usize,
    /// Runner group name from the most recently observed `RunnerInfo`
    /// for this label set, if available.
    pub group_name: Option<String>,
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

    /// Compute runner pool statistics from current job state.
    ///
    /// Groups all queued and in-progress jobs by their `LabelSet` to
    /// produce per-pool counts. The `group_name` is taken from the most
    /// recently observed `RunnerInfo` for each label set.
    ///
    /// Completed and waiting jobs are excluded from pool stats.
    pub async fn pool_stats(&self) -> Vec<RunnerPoolStats> {
        let state = self.state.read().await;

        let mut stats_map: HashMap<LabelSet, RunnerPoolStats> = HashMap::new();

        for job in state.jobs.values() {
            if matches!(job.status, JobStatus::Waiting | JobStatus::Completed) {
                continue;
            }

            let label_set = LabelSet::new(job.labels.iter().cloned());
            let entry = stats_map
                .entry(label_set.clone())
                .or_insert_with(|| RunnerPoolStats {
                    labels: label_set,
                    queued: 0,
                    running: 0,
                    group_name: None,
                });

            match job.status {
                JobStatus::Queued => {
                    entry.queued += 1;
                }
                JobStatus::InProgress => {
                    entry.running += 1;
                    // Track group_name from most recently observed runner (AC4.4)
                    if let Some(ref runner) = job.runner
                        && runner.group_name.is_some()
                    {
                        entry.group_name.clone_from(&runner.group_name);
                    }
                }
                _ => {}
            }
        }

        stats_map.into_values().collect()
    }

    /// Evict completed jobs that have exceeded the configured TTL.
    ///
    /// Scans for completed jobs where `completed_at + ttl < now`,
    /// removes them from the primary map and all secondary indexes,
    /// then evicts any runs that have no remaining jobs.
    ///
    /// Active jobs (queued, waiting, in-progress) are never evicted
    /// regardless of age.
    pub async fn evict_expired(&self) {
        tracing::debug!("starting eviction sweep");
        let start = std::time::Instant::now();

        let now = self.clock.now();
        let mut state = self.state.write().await;
        let ttl = TimeDelta::from_std(self.completed_ttl)
            .unwrap_or(TimeDelta::MAX);

        // Find expired completed job IDs
        let expired_job_ids: Vec<JobId> = state
            .jobs
            .iter()
            .filter(|(_, job)| {
                job.status == JobStatus::Completed
                    && job
                        .completed_at
                        .is_some_and(|t| now.signed_duration_since(t) > ttl)
            })
            .map(|(id, _)| *id)
            .collect();

        if expired_job_ids.is_empty() {
            #[allow(clippy::cast_possible_truncation)]
            let elapsed_us = start.elapsed().as_micros() as u64;
            tracing::debug!(
                elapsed_us,
                "eviction sweep complete, nothing to evict"
            );
            return;
        }

        // Remove expired jobs from primary map, collect affected run IDs
        let mut affected_run_ids = HashSet::new();
        for job_id in &expired_job_ids {
            if let Some(job) = state.jobs.remove(job_id) {
                affected_run_ids.insert(job.run_id);
            }
        }

        // Remove from jobs_by_run index
        for run_id in &affected_run_ids {
            if let Some(set) = state.jobs_by_run.get_mut(run_id) {
                for job_id in &expired_job_ids {
                    set.remove(job_id);
                }
            }
        }

        // Remove from jobs_by_repo index
        for set in state.jobs_by_repo.values_mut() {
            for job_id in &expired_job_ids {
                set.remove(job_id);
            }
        }
        state.jobs_by_repo.retain(|_, set| !set.is_empty());
        state.jobs_by_run.retain(|_, set| !set.is_empty());

        // Evict runs with no remaining jobs (AC5.3)
        let mut runs_evicted: u64 = 0;
        for run_id in &affected_run_ids {
            let has_jobs = state
                .jobs_by_run
                .get(run_id)
                .is_some_and(|set| !set.is_empty());
            if !has_jobs {
                state.runs.remove(run_id);
                state.jobs_by_run.remove(run_id);
                runs_evicted += 1;
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let elapsed_us = start.elapsed().as_micros() as u64;
        tracing::info!(
            jobs_evicted = expired_job_ids.len(),
            runs_evicted,
            elapsed_us,
            "eviction sweep complete"
        );
    }

    /// Start a background task that periodically evicts expired entries.
    ///
    /// Must be called on an `Arc<StateStore>`. Returns a
    /// [`JoinHandle`](tokio::task::JoinHandle) — drop or abort it to
    /// stop the eviction loop.
    ///
    /// The first eviction runs after `interval` elapses (not immediately).
    pub fn start_eviction_task(
        self: &Arc<Self>,
        interval: Duration,
    ) -> tokio::task::JoinHandle<()> {
        let store = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // First tick completes immediately — consume it
            ticker.tick().await;
            loop {
                ticker.tick().await;
                store.evict_expired().await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::TestClock;
    use crate::job::JobConclusion;
    use crate::run::RunConclusion;
    use chrono::{DateTime, Utc};

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

    /// Helper to build a JobEventEnvelope with custom completed_at timestamp.
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

    // ===== Task 2: Runner Pool Stats Derivation =====

    #[tokio::test]
    async fn test_ac4_3_basic_pool_counts() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1200);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create 2 queued jobs with labels ["linux", "self-hosted"]
        let labels = vec!["linux".to_string(), "self-hosted".to_string()];
        let job_id_1 = JobId(1201);
        let envelope_1 = make_job_event(
            job_id_1,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: labels.clone(),
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_1).await.unwrap();

        let job_id_2 = JobId(1202);
        let envelope_2 = make_job_event(
            job_id_2,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: labels.clone(),
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_2).await.unwrap();

        // Create 1 running job with same labels
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_id_3 = JobId(1203);
        let envelope_3 = make_job_event(
            job_id_3,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner,
                labels: labels.clone(),
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_3).await.unwrap();

        // Get pool stats
        let stats = store.pool_stats().await;

        // Verify one entry with correct counts
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].queued, 2);
        assert_eq!(stats[0].running, 1);
    }

    #[tokio::test]
    async fn test_ac4_3_multiple_pools() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1300);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create queued jobs with labels ["linux"]
        let job_id_1 = JobId(1301);
        let envelope_1 = make_job_event(
            job_id_1,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_1).await.unwrap();

        // Create queued jobs with labels ["macos"]
        let job_id_2 = JobId(1302);
        let envelope_2 = make_job_event(
            job_id_2,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["macos".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_2).await.unwrap();

        // Get pool stats
        let stats = store.pool_stats().await;

        // Verify two entries
        assert_eq!(stats.len(), 2);
        let mut counts: Vec<(usize, usize)> = stats
            .iter()
            .map(|s| (s.queued, s.running))
            .collect();
        counts.sort();
        assert_eq!(counts, vec![(1, 0), (1, 0)]);
    }

    #[tokio::test]
    async fn test_ac4_3_label_normalization() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1400);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create job with ["self-hosted", "linux"]
        let job_id_1 = JobId(1401);
        let envelope_1 = make_job_event(
            job_id_1,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["self-hosted".to_string(), "linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_1).await.unwrap();

        // Create job with ["linux", "self-hosted"] (different order)
        let job_id_2 = JobId(1402);
        let envelope_2 = make_job_event(
            job_id_2,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string(), "self-hosted".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_2).await.unwrap();

        // Get pool stats
        let stats = store.pool_stats().await;

        // Verify single entry (same label set regardless of order)
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].queued, 2);
        assert_eq!(stats[0].running, 0);
    }

    #[tokio::test]
    async fn test_ac4_3_excludes_completed() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1500);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        let labels = vec!["linux".to_string()];

        // Create a queued job
        let job_id_1 = JobId(1501);
        let envelope_1 = make_job_event(
            job_id_1,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: labels.clone(),
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_1).await.unwrap();

        // Create a completed job
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_id_2 = JobId(1502);
        let envelope_2 = make_job_event(
            job_id_2,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: labels.clone(),
                steps: vec![],
            },
        );
        store.apply_job_event(envelope_2).await.unwrap();

        // Get pool stats
        let stats = store.pool_stats().await;

        // Verify completed job is not counted
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].queued, 1);
        assert_eq!(stats[0].running, 0);
    }

    #[tokio::test]
    async fn test_ac4_4_group_name_from_runner_info() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1600);
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create a running job with group_name
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: Some(42),
            group_name: Some("default".to_string()),
        };
        let job_id = JobId(1601);
        let labels = vec!["linux".to_string()];
        let envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner,
                labels,
                steps: vec![],
            },
        );
        store.apply_job_event(envelope).await.unwrap();

        // Get pool stats
        let stats = store.pool_stats().await;

        // Verify group_name is captured
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].group_name, Some("default".to_string()));
    }

    // ===== Task 1: TTL Eviction =====

    #[tokio::test]
    async fn test_ac5_1_completed_job_within_ttl_retained() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(1700);
        let job_id = JobId(1701);

        // Create and complete a job at t0
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_envelope = make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Advance clock to t0 + 30 minutes (within 1-hour TTL)
        clock.advance(TimeDelta::minutes(30));

        // Call evict_expired()
        store.evict_expired().await;

        // Verify job still exists
        let job = store.get_job(&job_id).await;
        assert!(job.is_some(), "Job should be retained within TTL");
    }

    #[tokio::test]
    async fn test_ac5_2_completed_job_past_ttl_evicted() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(1800);
        let job_id = JobId(1801);

        // Create and complete a job at t0
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_envelope = make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Advance clock to t0 + 2 hours (past 1-hour TTL)
        clock.advance(TimeDelta::hours(2));

        // Call evict_expired()
        store.evict_expired().await;

        // Verify job is removed from primary map
        let job = store.get_job(&job_id).await;
        assert!(job.is_none(), "Completed job past TTL should be evicted from primary map");

        // Verify job is removed from jobs_for_run
        let jobs = store.jobs_for_run(&run_id).await;
        assert!(!jobs.contains(&job_id), "Evicted job should not be in jobs_for_run");

        // Verify job is removed from jobs_for_repo
        let repo = RepoKey::new("org", "repo");
        let jobs = store.jobs_for_repo(&repo).await;
        assert!(!jobs.contains(&job_id), "Evicted job should not be in jobs_for_repo");
    }

    #[tokio::test]
    async fn test_ac5_3_run_with_no_jobs_evicted() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(1900);
        let job_id = JobId(1901);

        // Create a run with one job
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_envelope = make_job_event_with_completed_at(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Advance clock past TTL and evict
        clock.advance(TimeDelta::hours(2));
        store.evict_expired().await;

        // Verify both job and run are evicted
        let job = store.get_job(&job_id).await;
        assert!(job.is_none(), "Job should be evicted");
        let run = store.get_run(&run_id).await;
        assert!(run.is_none(), "Run with no remaining jobs should be evicted");
    }

    #[tokio::test]
    async fn test_ac5_3_run_with_active_job_retained() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(2000);
        let completed_job_id = JobId(2001);
        let active_job_id = JobId(2002);

        // Create a run with two jobs
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };

        // Complete one job
        let completed_envelope = make_job_event_with_completed_at(
            completed_job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner.clone()),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store.apply_job_event(completed_envelope).await.unwrap();

        // Create an active (running) job
        let active_envelope = make_job_event(
            active_job_id,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner,
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(active_envelope).await.unwrap();

        // Advance clock past TTL and evict
        clock.advance(TimeDelta::hours(2));
        store.evict_expired().await;

        // Verify completed job is evicted but run and active job remain
        let completed_job = store.get_job(&completed_job_id).await;
        assert!(completed_job.is_none(), "Completed job past TTL should be evicted");

        let active_job = store.get_job(&active_job_id).await;
        assert!(active_job.is_some(), "Active job should be retained");

        let run = store.get_run(&run_id).await;
        assert!(run.is_some(), "Run should be retained because it still has an active job");
    }

    #[tokio::test]
    async fn test_ac5_4_active_jobs_never_evicted() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(2100);
        let queued_job_id = JobId(2101);
        let running_job_id = JobId(2102);
        let waiting_job_id = JobId(2103);

        // Create a run
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Create queued job
        let queued_envelope = make_job_event(
            queued_job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(queued_envelope).await.unwrap();

        // Create running job
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let running_envelope = make_job_event(
            running_job_id,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner,
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(running_envelope).await.unwrap();

        // Create waiting job
        let waiting_envelope = make_job_event(
            waiting_job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        );
        store.apply_job_event(waiting_envelope).await.unwrap();

        // Advance clock well past TTL and evict
        clock.advance(TimeDelta::days(10));
        store.evict_expired().await;

        // Verify all active jobs are retained
        let queued_job = store.get_job(&queued_job_id).await;
        assert!(queued_job.is_some(), "Queued job should never be evicted");

        let running_job = store.get_job(&running_job_id).await;
        assert!(running_job.is_some(), "Running job should never be evicted");

        let waiting_job = store.get_job(&waiting_job_id).await;
        assert!(waiting_job.is_some(), "Waiting job should never be evicted");
    }

    #[tokio::test]
    async fn test_ac5_5_ttl_configurable() {
        let start_time = Utc::now();
        let clock_1h = Arc::new(TestClock::new(start_time));
        let clock_5m = Arc::new(TestClock::new(start_time));

        // Store with 1-hour TTL
        let store_1h = StateStore::new(clock_1h.clone(), Duration::from_secs(3600));
        // Store with 5-minute TTL
        let store_5m = StateStore::new(clock_5m.clone(), Duration::from_secs(300));

        let run_id_1h = RunId(2200);
        let job_id_1h = JobId(2201);
        let run_id_5m = RunId(2300);
        let job_id_5m = JobId(2301);

        // Setup store with 1-hour TTL
        let run_envelope_1h = make_run_event(run_id_1h, RunEvent::Requested);
        store_1h.apply_run_event(run_envelope_1h).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let job_envelope_1h = make_job_event_with_completed_at(
            job_id_1h,
            run_id_1h,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner.clone()),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store_1h.apply_job_event(job_envelope_1h).await.unwrap();

        // Setup store with 5-minute TTL
        let run_envelope_5m = make_run_event(run_id_5m, RunEvent::Requested);
        store_5m.apply_run_event(run_envelope_5m).await.unwrap();

        let job_envelope_5m = make_job_event_with_completed_at(
            job_id_5m,
            run_id_5m,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec![],
                steps: vec![],
            },
            Some(start_time),
        );
        store_5m.apply_job_event(job_envelope_5m).await.unwrap();

        // Advance both clocks to t0 + 30 minutes
        clock_1h.advance(TimeDelta::minutes(30));
        clock_5m.advance(TimeDelta::minutes(30));

        // Evict from both stores
        store_1h.evict_expired().await;
        store_5m.evict_expired().await;

        // Verify: 1-hour store retains job, 5-minute store evicts it
        let job_1h = store_1h.get_job(&job_id_1h).await;
        assert!(job_1h.is_some(), "Job in 1-hour store should be retained");

        let job_5m = store_5m.get_job(&job_id_5m).await;
        assert!(job_5m.is_none(), "Job in 5-minute store should be evicted");
    }

    // Edge case tests for AC6.5 — out-of-order, duplicate, and unknown-ID events
    #[tokio::test]
    async fn test_ac6_5_out_of_order_job_before_run() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(1);
        let job_id = JobId(1);

        // Send JobEvent::Completed before RunEvent::Requested
        let job_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(job_envelope).await.unwrap();

        // Then send RunEvent::Requested
        let run_envelope = make_run_event(run_id, RunEvent::Requested);
        store.apply_run_event(run_envelope).await.unwrap();

        // Verify job exists in completed state
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Completed);

        // Verify run exists in queued state
        let run = store.get_run(&run_id).await.expect("run should exist");
        assert_eq!(run.status, RunStatus::Queued);

        // Verify indexes are consistent
        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_out_of_order_completed_before_queued() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(2);
        let job_id = JobId(2);

        // First send JobEvent::Completed
        let completed_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(completed_envelope).await.unwrap();

        // Then try to send JobEvent::Queued for same job
        let queued_envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        let result = store.apply_job_event(queued_envelope).await;

        // Second event should return error (backward transition)
        assert!(result.is_err(), "Backward transition should be rejected");

        // Job should still be completed
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Completed);

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_duplicate_queued_events() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(3);
        let job_id = JobId(3);

        // Send JobEvent::Queued twice
        let envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );

        store.apply_job_event(envelope.clone()).await.unwrap();
        let result = store.apply_job_event(envelope).await;

        // Second send should not error (idempotent)
        assert!(result.is_ok(), "Duplicate same-status event should be idempotent");

        // Job should still be queued, and only appear once in indexes
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Queued);

        let jobs_for_run = store.jobs_for_run(&run_id).await;
        assert_eq!(
            jobs_for_run.len(),
            1,
            "Job should appear exactly once in jobs_by_run"
        );

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_duplicate_completed_events() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(4);
        let job_id = JobId(4);

        // Send JobEvent::Completed twice
        let envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );

        store.apply_job_event(envelope.clone()).await.unwrap();
        let result = store.apply_job_event(envelope).await;

        // Second send should not error (idempotent)
        assert!(result.is_ok(), "Duplicate completed event should be idempotent");

        // Job should still be completed
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Completed);

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_unknown_run_id_on_job() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(99); // No run event for this ID
        let job_id = JobId(5);

        // Send JobEvent::Queued with run_id that has no corresponding RunEvent
        let envelope = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(envelope).await.unwrap();

        // Job should be created successfully with the unknown run_id
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.run_id, run_id);
        assert_eq!(job.status, JobStatus::Queued);

        // Verify indexes are consistent (job is in jobs_by_run even though run doesn't exist)
        let jobs_for_run = store.jobs_for_run(&run_id).await;
        assert!(jobs_for_run.contains(&job_id));

        // Run itself should not exist
        let run = store.get_run(&run_id).await;
        assert!(run.is_none(), "Run should not exist if no RunEvent was sent");

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_rapid_status_cycling() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run_id = RunId(5);
        let job_id = JobId(6);

        // Send Queued
        let queued = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(queued).await.unwrap();

        // Send InProgress
        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };
        let in_progress = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::InProgress {
                runner: runner.clone(),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(in_progress).await.unwrap();

        // Send Completed
        let completed = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: Some(runner),
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        store.apply_job_event(completed).await.unwrap();

        // Try to send Queued again
        let queued_again = make_job_event(
            job_id,
            run_id,
            "org",
            "repo",
            JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            },
        );
        let result = store.apply_job_event(queued_again).await;

        // Should be rejected
        assert!(result.is_err(), "Cannot go from Completed back to Queued");

        // Job should remain completed
        let job = store.get_job(&job_id).await.expect("job should exist");
        assert_eq!(job.status, JobStatus::Completed);

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_interleaved_multi_job() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock, Duration::from_secs(3600));

        let run1_id = RunId(10);
        let run2_id = RunId(11);

        // Create two runs
        store.apply_run_event(make_run_event(run1_id, RunEvent::Requested)).await.unwrap();
        store.apply_run_event(make_run_event(run2_id, RunEvent::Requested)).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };

        // Add jobs 1-3 to run 1
        for i in 1..=3 {
            let job_id = JobId(i * 100);
            let queued = make_job_event(job_id, run1_id, "org", "repo", JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            });
            store.apply_job_event(queued).await.unwrap();

            let in_progress = make_job_event(job_id, run1_id, "org", "repo", JobEvent::InProgress {
                runner: runner.clone(),
                labels: vec!["linux".to_string()],
                steps: vec![],
            });
            store.apply_job_event(in_progress).await.unwrap();
        }

        // Add jobs 4-5 to run 2
        for i in 4..=5 {
            let job_id = JobId(i * 100);
            let queued = make_job_event(job_id, run2_id, "org", "repo", JobEvent::Queued {
                labels: vec!["linux".to_string()],
                steps: vec![],
            });
            store.apply_job_event(queued).await.unwrap();
        }

        // Verify both runs' indexes are correct and no cross-contamination
        let run1_jobs = store.jobs_for_run(&run1_id).await;
        assert_eq!(run1_jobs.len(), 3);
        for i in 1..=3 {
            assert!(run1_jobs.contains(&JobId(i * 100)));
        }

        let run2_jobs = store.jobs_for_run(&run2_id).await;
        assert_eq!(run2_jobs.len(), 2);
        for i in 4..=5 {
            assert!(run2_jobs.contains(&JobId(i * 100)));
        }

        store.assert_invariants().await;
    }

    #[tokio::test]
    async fn test_ac6_5_eviction_with_mixed_state() {
        let start_time = Utc::now();
        let clock = Arc::new(TestClock::new(start_time));
        let store = StateStore::new(clock.clone(), Duration::from_secs(3600));

        let run_id = RunId(20);

        // Create run
        store.apply_run_event(make_run_event(run_id, RunEvent::Requested)).await.unwrap();

        use crate::job::RunnerInfo;
        let runner = RunnerInfo {
            id: 1,
            name: "runner-1".to_string(),
            group_id: None,
            group_name: None,
        };

        // Create some completed jobs
        for i in 1..=3 {
            let job_id = JobId(i);
            let completed_envelope = make_job_event_with_completed_at(
                job_id,
                run_id,
                "org",
                "repo",
                JobEvent::Completed {
                    conclusion: JobConclusion::Success,
                    runner: Some(runner.clone()),
                    labels: vec!["linux".to_string()],
                    steps: vec![],
                },
                Some(start_time),
            );
            store.apply_job_event(completed_envelope).await.unwrap();
        }

        // Create some active jobs
        for i in 4..=6 {
            let job_id = JobId(i);
            let queued_envelope = make_job_event(
                job_id,
                run_id,
                "org",
                "repo",
                JobEvent::Queued {
                    labels: vec!["linux".to_string()],
                    steps: vec![],
                },
            );
            store.apply_job_event(queued_envelope).await.unwrap();
        }

        // Advance time past TTL
        clock.advance(TimeDelta::hours(2));
        store.evict_expired().await;

        // Verify: completed jobs (1-3) evicted, active jobs (4-6) retained
        for i in 1..=3 {
            let job = store.get_job(&JobId(i)).await;
            assert!(job.is_none(), "Completed job {} should be evicted", i);
        }

        for i in 4..=6 {
            let job = store.get_job(&JobId(i)).await;
            assert!(job.is_some(), "Active job {} should be retained", i);
        }

        // Verify indexes are consistent
        store.assert_invariants().await;
    }
}

#[cfg(test)]
impl StateStore {
    /// Assert all store invariants hold. Panics with a descriptive
    /// message if any invariant is violated.
    pub(crate) async fn assert_invariants(&self) {
        let state = self.state.read().await;

        // AC6.1: Every job in jobs_by_repo exists in jobs map
        for (repo, job_ids) in &state.jobs_by_repo {
            for job_id in job_ids {
                assert!(
                    state.jobs.contains_key(job_id),
                    "jobs_by_repo[{repo}] contains {job_id:?} not in jobs map"
                );
            }
        }

        // AC6.1: Every job in jobs map exists in exactly one jobs_by_repo set
        for job_id in state.jobs.keys() {
            let count = state
                .jobs_by_repo
                .values()
                .filter(|set| set.contains(job_id))
                .count();
            assert!(
                count == 1,
                "job {job_id:?} appears in {count} jobs_by_repo sets (expected 1)"
            );
        }

        // AC6.1: Every job in jobs_by_run exists in jobs map
        for (run_id, job_ids) in &state.jobs_by_run {
            for job_id in job_ids {
                assert!(
                    state.jobs.contains_key(job_id),
                    "jobs_by_run[{run_id:?}] contains {job_id:?} not in jobs map"
                );
            }
        }

        // AC6.1: Every job in jobs map exists in jobs_by_run under its run_id
        for (job_id, job) in &state.jobs {
            let in_run_index = state
                .jobs_by_run
                .get(&job.run_id)
                .is_some_and(|set| set.contains(job_id));
            assert!(
                in_run_index,
                "job {job_id:?} not in jobs_by_run[{:?}]",
                job.run_id
            );
        }

        // AC6.3: No job has a "backward" status relative to its conclusion.
        // If conclusion is set, status must be Completed.
        for (job_id, job) in &state.jobs {
            if job.conclusion.is_some() {
                assert!(
                    job.status == JobStatus::Completed,
                    "job {job_id:?} has conclusion but status {:?}",
                    job.status
                );
            }
        }

        // AC6.4: Active jobs are never evicted — if a job exists,
        // and has an active status, it must be in the primary map.
        // (This is tautological from the map, but we verify it
        // symmetrically with the indexes.)
        for (job_id, job) in &state.jobs {
            if matches!(
                job.status,
                JobStatus::Queued | JobStatus::Waiting | JobStatus::InProgress
            ) {
                // Active job must be in both indexes
                let in_run = state
                    .jobs_by_run
                    .get(&job.run_id)
                    .is_some_and(|set| set.contains(job_id));
                let in_repo = state
                    .jobs_by_repo
                    .values()
                    .any(|set| set.contains(job_id));
                assert!(in_run, "active job {job_id:?} missing from jobs_by_run");
                assert!(in_repo, "active job {job_id:?} missing from jobs_by_repo");
            }
        }

        // No empty index entries (cleanup correctness)
        for (key, set) in &state.jobs_by_repo {
            assert!(!set.is_empty(), "empty jobs_by_repo entry for {key}");
        }
        for (key, set) in &state.jobs_by_run {
            assert!(!set.is_empty(), "empty jobs_by_run entry for {key:?}");
        }
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use crate::job::RunnerInfo;
    use proptest::prelude::*;
    use chrono::Utc;

    /// All possible test actions that can be applied to the store.
    #[derive(Debug, Clone)]
    enum TestAction {
        RequestRun(i64),
        StartRun(i64),
        CompleteRun(i64),
        QueueJob(i64, i64),
        StartJob(i64),
        CompleteJob(i64),
        AdvanceTimeAndEvict,
    }

    /// Generate a strategy for test actions.
    fn test_action_strategy() -> impl Strategy<Value = TestAction> {
        prop_oneof![
            (1i64..=3).prop_map(TestAction::RequestRun),
            (1i64..=3).prop_map(TestAction::StartRun),
            (1i64..=3).prop_map(TestAction::CompleteRun),
            (1i64..=3, 1i64..=10).prop_map(|(run_id, job_id)| TestAction::QueueJob(run_id, job_id)),
            (1i64..=10).prop_map(TestAction::StartJob),
            (1i64..=10).prop_map(TestAction::CompleteJob),
            Just(TestAction::AdvanceTimeAndEvict),
        ]
    }

    /// Apply a test action to the store, silently ignoring errors.
    async fn apply_action(
        store: &StateStore,
        clock: &Arc<crate::clock::TestClock>,
        action: &TestAction,
    ) {
        match action {
            TestAction::RequestRun(run_id) => {
                let run_id = RunId(*run_id);
                let now = Utc::now();
                let envelope = RunEventEnvelope {
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    workflow_name: "test-workflow".to_string(),
                    workflow_path: ".github/workflows/test.yml".to_string(),
                    branch: Some("main".to_string()),
                    head_sha: "abc123".to_string(),
                    commit_message: Some("test commit".to_string()),
                    trigger_event: "push".to_string(),
                    display_title: "Test Run".to_string(),
                    html_url: "https://example.com/run".to_string(),
                    created_at: now,
                    run_started_at: None,
                    updated_at: now,
                    action: RunEvent::Requested,
                };
                let _ = store.apply_run_event(envelope).await;
            }
            TestAction::StartRun(run_id) => {
                let run_id = RunId(*run_id);
                let now = Utc::now();
                let envelope = RunEventEnvelope {
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    workflow_name: "test-workflow".to_string(),
                    workflow_path: ".github/workflows/test.yml".to_string(),
                    branch: Some("main".to_string()),
                    head_sha: "abc123".to_string(),
                    commit_message: Some("test commit".to_string()),
                    trigger_event: "push".to_string(),
                    display_title: "Test Run".to_string(),
                    html_url: "https://example.com/run".to_string(),
                    created_at: now,
                    run_started_at: None,
                    updated_at: now,
                    action: RunEvent::InProgress,
                };
                let _ = store.apply_run_event(envelope).await;
            }
            TestAction::CompleteRun(run_id) => {
                let run_id = RunId(*run_id);
                let now = Utc::now();
                let envelope = RunEventEnvelope {
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    workflow_name: "test-workflow".to_string(),
                    workflow_path: ".github/workflows/test.yml".to_string(),
                    branch: Some("main".to_string()),
                    head_sha: "abc123".to_string(),
                    commit_message: Some("test commit".to_string()),
                    trigger_event: "push".to_string(),
                    display_title: "Test Run".to_string(),
                    html_url: "https://example.com/run".to_string(),
                    created_at: now,
                    run_started_at: None,
                    updated_at: now,
                    action: RunEvent::Completed {
                        conclusion: crate::run::RunConclusion::Success,
                    },
                };
                let _ = store.apply_run_event(envelope).await;
            }
            TestAction::QueueJob(run_id, job_id) => {
                let run_id = RunId(*run_id);
                let job_id = JobId(*job_id);
                let now = Utc::now();
                let envelope = JobEventEnvelope {
                    job_id,
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    name: "test-job".to_string(),
                    created_at: now,
                    started_at: None,
                    completed_at: None,
                    action: JobEvent::Queued {
                        labels: vec!["linux".to_string()],
                        steps: vec![],
                    },
                };
                let _ = store.apply_job_event(envelope).await;
            }
            TestAction::StartJob(job_id) => {
                let job_id = JobId(*job_id);
                let now = Utc::now();
                let runner = RunnerInfo {
                    id: 1,
                    name: "runner-1".to_string(),
                    group_id: None,
                    group_name: None,
                };
                // We need to get the run_id from the existing job
                // For simplicity in property tests, use run_id = 1
                let run_id = RunId(1);
                let envelope = JobEventEnvelope {
                    job_id,
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    name: "test-job".to_string(),
                    created_at: now,
                    started_at: None,
                    completed_at: None,
                    action: JobEvent::InProgress {
                        runner,
                        labels: vec!["linux".to_string()],
                        steps: vec![],
                    },
                };
                let _ = store.apply_job_event(envelope).await;
            }
            TestAction::CompleteJob(job_id) => {
                let job_id = JobId(*job_id);
                let now = Utc::now();
                let runner = RunnerInfo {
                    id: 1,
                    name: "runner-1".to_string(),
                    group_id: None,
                    group_name: None,
                };
                // Same simplification: use run_id = 1
                let run_id = RunId(1);
                let envelope = JobEventEnvelope {
                    job_id,
                    run_id,
                    org: "test-org".to_string(),
                    repo: "test-repo".to_string(),
                    name: "test-job".to_string(),
                    created_at: now,
                    started_at: None,
                    completed_at: Some(now),
                    action: JobEvent::Completed {
                        conclusion: crate::job::JobConclusion::Success,
                        runner: Some(runner),
                        labels: vec!["linux".to_string()],
                        steps: vec![],
                    },
                };
                let _ = store.apply_job_event(envelope).await;
            }
            TestAction::AdvanceTimeAndEvict => {
                clock.advance(chrono::TimeDelta::hours(2));
                store.evict_expired().await;
            }
        }
    }

    proptest! {
        #[test]
        fn store_invariants_hold(
            actions in prop::collection::vec(test_action_strategy(), 10..100)
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let clock = Arc::new(crate::clock::TestClock::new(Utc::now()));
                let store = StateStore::new(
                    clock.clone(),
                    Duration::from_secs(3600),
                );

                for action in &actions {
                    // Apply action, ignore errors (invalid transitions expected)
                    apply_action(&store, &clock, action).await;
                }

                // After all actions, invariants must hold
                store.assert_invariants().await;
            });
        }
    }
}
