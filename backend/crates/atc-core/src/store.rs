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
                workflow_name: envelope.workflow_name.or(existing.workflow_name),
                workflow_path: envelope.workflow_path.or(existing.workflow_path),
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

    /// Return all runs and jobs in the store.
    ///
    /// This is the unfiltered read path used by `GET /v1/state` before
    /// per-user scoping (Phase 11). Returns owned snapshots.
    pub async fn query_all(&self) -> QueryResult {
        let state = self.state.read().await;

        let runs: Vec<WorkflowRun> = state.runs.values().cloned().collect();
        let jobs: Vec<Job> = state.jobs.values().cloned().collect();

        QueryResult { runs, jobs }
    }

    /// Return a consistent snapshot of all state: runs, jobs, and pool stats.
    ///
    /// Reads everything under a single `RwLock` acquisition so the
    /// returned data describes the same point in time. The REST handler
    /// uses this instead of separate `query_all()` + `pool_stats()` calls
    /// to prevent interleaving with concurrent webhook mutations.
    pub async fn snapshot(&self) -> (QueryResult, Vec<RunnerPoolStats>) {
        let state = self.state.read().await;

        let runs: Vec<WorkflowRun> = state.runs.values().cloned().collect();
        let jobs: Vec<Job> = state.jobs.values().cloned().collect();

        // Compute pool stats inline (same logic as pool_stats() but
        // under the same lock acquisition).
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
                JobStatus::Queued => entry.queued += 1,
                JobStatus::InProgress => entry.running += 1,
                _ => {}
            }
            if let Some(ref runner) = job.runner {
                if let Some(ref name) = runner.group_name {
                    entry.group_name = Some(name.clone());
                }
            }
        }

        (
            QueryResult { runs, jobs },
            stats_map.into_values().collect(),
        )
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
        let ttl = TimeDelta::from_std(self.completed_ttl).unwrap_or(TimeDelta::MAX);

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
            tracing::debug!(elapsed_us, "eviction sweep complete, nothing to evict");
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
                let in_repo = state.jobs_by_repo.values().any(|set| set.contains(job_id));
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
mod property_tests;
#[cfg(test)]
mod tests;
