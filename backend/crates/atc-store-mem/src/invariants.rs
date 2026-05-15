//! Inspection helpers for tests.
//!
//! Enabled under `#[cfg(test)]` (unit tests) and the `test-support` feature
//! (integration tests compiled as a separate crate). All methods are `pub` so
//! integration test binaries can call them; the feature gate is the only guard
//! against inclusion in production builds.

use std::collections::HashSet;

use atc_core::{Job, JobId, RunId, WorkflowRun, types::RepoKey};

use crate::InMemoryStore;

impl InMemoryStore {
    /// Return a snapshot of a job by ID (for test assertions).
    pub async fn get_job(&self, job_id: &JobId) -> Option<Job> {
        self.state.read().await.jobs.get(job_id).cloned()
    }

    /// Return a snapshot of a run by ID (for test assertions).
    pub async fn get_run(&self, run_id: &RunId) -> Option<WorkflowRun> {
        self.state.read().await.runs.get(run_id).cloned()
    }

    /// Return the set of job IDs for a given run (for test assertions).
    pub async fn jobs_for_run(&self, run_id: &RunId) -> HashSet<JobId> {
        self.state
            .read()
            .await
            .jobs_by_run
            .get(run_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Return the set of job IDs for a given repository (for test assertions).
    pub async fn jobs_for_repo(&self, repo_key: &RepoKey) -> HashSet<JobId> {
        self.state
            .read()
            .await
            .jobs_by_repo
            .get(repo_key)
            .cloned()
            .unwrap_or_default()
    }

    /// Return the current seq value (for test assertions).
    pub async fn current_seq(&self) -> u64 {
        *self.seq.lock().await
    }

    /// Assert all store invariants hold. Panics with a descriptive
    /// message if any invariant is violated.
    pub async fn assert_invariants(&self) {
        use atc_core::JobStatus;
        let state = self.state.read().await;

        // Every job in jobs_by_repo exists in jobs map
        for (repo, job_ids) in &state.jobs_by_repo {
            for job_id in job_ids {
                assert!(
                    state.jobs.contains_key(job_id),
                    "jobs_by_repo[{repo}] contains {job_id:?} not in jobs map"
                );
            }
        }

        // Every job in jobs map exists in exactly one jobs_by_repo set
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

        // Every job in jobs_by_run exists in jobs map
        for (run_id, job_ids) in &state.jobs_by_run {
            for job_id in job_ids {
                assert!(
                    state.jobs.contains_key(job_id),
                    "jobs_by_run[{run_id:?}] contains {job_id:?} not in jobs map"
                );
            }
        }

        // Every job in jobs map exists in jobs_by_run under its run_id
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

        // Active jobs are never evicted — verify symmetrically with the indexes.
        for (job_id, job) in &state.jobs {
            if matches!(
                job.status,
                JobStatus::Queued | JobStatus::Waiting | JobStatus::InProgress
            ) {
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
