//! PostgreSQL-backed persistence for domain events.
//!
//! [`PgStore`] implements [`atc_core::PersistentStore`] against a live
//! PostgreSQL connection pool. Every write is a single predicated UPSERT:
//! `INSERT ... ON CONFLICT (id) DO UPDATE ... WHERE status = ANY($preds::text[])`.
//! Zero rows affected ⇒ [`PersistError::InvalidTransition`].
//!
//! The module also contains string-mapping helpers for status and conclusion enums
//! that match the SQL CHECK constraint values exactly, independent of serde repr.

use atc_core::{
    JobConclusion, JobStatus, PersistError, PersistentStore, RunConclusion, RunStatus,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// Status/conclusion → SQL string helpers
// ---------------------------------------------------------------------------

/// Map [`RunStatus`] to the SQL CHECK constraint string value.
///
/// These must exactly match the `CHECK (status IN (...))` constraint in
/// `0001_initial_runs_jobs.sql`. Independent of serde so a future
/// `#[serde(rename)]` cannot silently break the SQL bind.
pub(crate) fn run_status_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Queued => "Queued",
        RunStatus::InProgress => "InProgress",
        RunStatus::Completed => "Completed",
    }
}

/// Map [`JobStatus`] to the SQL CHECK constraint string value.
pub(crate) fn job_status_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Queued => "Queued",
        JobStatus::Waiting => "Waiting",
        JobStatus::InProgress => "InProgress",
        JobStatus::Completed => "Completed",
    }
}

/// Map [`RunConclusion`] to the SQL CHECK constraint string value.
fn run_conclusion_str(c: RunConclusion) -> &'static str {
    match c {
        RunConclusion::Success => "Success",
        RunConclusion::Failure => "Failure",
        RunConclusion::Cancelled => "Cancelled",
        RunConclusion::TimedOut => "TimedOut",
        RunConclusion::ActionRequired => "ActionRequired",
        RunConclusion::Stale => "Stale",
        RunConclusion::Neutral => "Neutral",
        RunConclusion::Skipped => "Skipped",
        RunConclusion::StartupFailure => "StartupFailure",
    }
}

/// Map [`JobConclusion`] to the SQL CHECK constraint string value.
fn job_conclusion_str(c: JobConclusion) -> &'static str {
    match c {
        JobConclusion::Success => "Success",
        JobConclusion::Failure => "Failure",
        JobConclusion::Cancelled => "Cancelled",
        JobConclusion::TimedOut => "TimedOut",
        JobConclusion::ActionRequired => "ActionRequired",
        JobConclusion::Stale => "Stale",
        JobConclusion::Neutral => "Neutral",
        JobConclusion::Skipped => "Skipped",
    }
}

// ---------------------------------------------------------------------------
// Derive target status from event action
// ---------------------------------------------------------------------------

fn derive_run_target(action: &RunEvent) -> RunStatus {
    match action {
        RunEvent::Requested => RunStatus::Queued,
        RunEvent::InProgress => RunStatus::InProgress,
        RunEvent::Completed { .. } => RunStatus::Completed,
    }
}

fn derive_job_target(action: &JobEvent) -> JobStatus {
    match action {
        JobEvent::Queued { .. } => JobStatus::Queued,
        JobEvent::Waiting { .. } => JobStatus::Waiting,
        JobEvent::InProgress { .. } => JobStatus::InProgress,
        JobEvent::Completed { .. } => JobStatus::Completed,
    }
}

// ---------------------------------------------------------------------------
// PgStore
// ---------------------------------------------------------------------------

/// PostgreSQL-backed implementation of [`PersistentStore`].
///
/// Holds a connection pool and performs predicated UPSERTs for run and job
/// events. Each method is a single statement (or two for job-before-run) using
/// `sqlx::query!` compile-time-checked SQL.
pub struct PgStore {
    pool: PgPool,
}

impl PgStore {
    /// Create a new [`PgStore`] backed by the given connection pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Check pool connectivity. Used in tests and health-check utilities.
    pub async fn ping(&self) -> Result<(), sqlx::Error> {
        sqlx::query!("SELECT 1 AS ok")
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
    }
}

#[async_trait::async_trait]
impl PersistentStore for PgStore {
    /// Upsert a run event into the `runs` table.
    ///
    /// Uses a predicated `ON CONFLICT DO UPDATE ... WHERE runs.status = ANY($preds)`.
    /// Zero rows affected maps to [`PersistError::InvalidTransition`].
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<(), PersistError> {
        let target = derive_run_target(&env.action);
        let preds = RunStatus::predecessors_of(target);
        let preds_strs: Vec<&'static str> = preds.iter().copied().map(run_status_str).collect();
        let target_str = run_status_str(target);

        // Conclusion is only set on RunEvent::Completed.
        let conclusion_str: Option<&'static str> =
            if let RunEvent::Completed { conclusion } = &env.action {
                Some(run_conclusion_str(*conclusion))
            } else {
                None
            };

        let run_id = env.run_id.0;

        let result = sqlx::query!(
            r#"
            INSERT INTO runs (
                id, org, repo, workflow_name, workflow_path, branch, head_sha,
                commit_message, event, display_title, status, conclusion,
                html_url, created_at, run_started_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16
            )
            ON CONFLICT (id) DO UPDATE SET
                workflow_name  = COALESCE(EXCLUDED.workflow_name, runs.workflow_name),
                workflow_path  = COALESCE(EXCLUDED.workflow_path, runs.workflow_path),
                branch         = EXCLUDED.branch,
                head_sha       = EXCLUDED.head_sha,
                commit_message = EXCLUDED.commit_message,
                event          = EXCLUDED.event,
                display_title  = EXCLUDED.display_title,
                status         = EXCLUDED.status,
                conclusion     = COALESCE(EXCLUDED.conclusion, runs.conclusion),
                html_url       = EXCLUDED.html_url,
                created_at     = EXCLUDED.created_at,
                run_started_at = COALESCE(EXCLUDED.run_started_at, runs.run_started_at),
                updated_at     = EXCLUDED.updated_at
            WHERE runs.status = ANY($17::text[])
            "#,
            run_id,
            env.org,
            env.repo,
            env.workflow_name,
            env.workflow_path,
            env.branch,
            env.head_sha,
            env.commit_message,
            env.trigger_event,
            env.display_title,
            target_str as &str,
            conclusion_str as Option<&str>,
            env.html_url,
            env.created_at,
            env.run_started_at,
            env.updated_at,
            &preds_strs as &[&str],
        )
        .execute(&self.pool)
        .await
        .map_err(|e| PersistError::Backend(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(PersistError::InvalidTransition);
        }
        Ok(())
    }

    /// Upsert a job event into the `jobs` table.
    ///
    /// Precedes the job UPSERT with a stub run INSERT (`ON CONFLICT DO NOTHING`)
    /// to satisfy the FK constraint when jobs arrive before their run.
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<(), PersistError> {
        let target = derive_job_target(&env.action);
        let preds = JobStatus::predecessors_of(target);
        let preds_strs: Vec<&'static str> = preds.iter().copied().map(job_status_str).collect();
        let target_str = job_status_str(target);

        let (conclusion_str, labels, steps, runner) = match &env.action {
            JobEvent::Queued { labels, steps } => (None, labels, steps, None),
            JobEvent::Waiting { labels, steps } => (None, labels, steps, None),
            JobEvent::InProgress {
                runner,
                labels,
                steps,
            } => (None, labels, steps, runner.as_ref()),
            JobEvent::Completed {
                conclusion,
                runner,
                labels,
                steps,
            } => (
                Some(job_conclusion_str(*conclusion)),
                labels,
                steps,
                runner.as_ref(),
            ),
        };

        let steps_json =
            serde_json::to_value(steps).map_err(|e| PersistError::Backend(Box::new(e)))?;

        let run_id = env.run_id.0;
        let job_id = env.job_id.0;

        // Statement 1: Ensure a stub run row exists to satisfy FK.
        // head_sha, event, display_title, html_url are NOT NULL; stub uses ''.
        // ON CONFLICT DO NOTHING: safe to call concurrently and repeatedly.
        sqlx::query!(
            r#"
            INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at)
            VALUES ($1, $2, $3, '', '', '', '', 'Queued', $4, $4)
            ON CONFLICT (id) DO NOTHING
            "#,
            run_id,
            env.org,
            env.repo,
            env.created_at,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| PersistError::Backend(Box::new(e)))?;

        // Statement 2: Predicated job UPSERT.
        // NOTE: jobs table has NO updated_at column (see migration 0001).
        // name is non-optional String → use jobs.name (identity, never overwritten).
        let runner_id: Option<i64> = runner.map(|r| r.id);
        let runner_name: Option<&str> = runner.map(|r| r.name.as_str());
        let runner_group_id: Option<i64> = runner.and_then(|r| r.group_id);
        let runner_group_name: Option<&str> = runner.and_then(|r| r.group_name.as_deref());

        let result = sqlx::query!(
            r#"
            INSERT INTO jobs (
                id, run_id, name, status, conclusion, labels, steps,
                runner_id, runner_name, runner_group_id, runner_group_name,
                started_at, completed_at, created_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11,
                $12, $13, $14
            )
            ON CONFLICT (id) DO UPDATE SET
                name              = jobs.name,
                run_id            = jobs.run_id,
                status            = EXCLUDED.status,
                conclusion        = COALESCE(EXCLUDED.conclusion, jobs.conclusion),
                labels            = EXCLUDED.labels,
                steps             = EXCLUDED.steps,
                runner_id         = COALESCE(EXCLUDED.runner_id,         jobs.runner_id),
                runner_name       = COALESCE(EXCLUDED.runner_name,       jobs.runner_name),
                runner_group_id   = COALESCE(EXCLUDED.runner_group_id,   jobs.runner_group_id),
                runner_group_name = COALESCE(EXCLUDED.runner_group_name, jobs.runner_group_name),
                started_at        = COALESCE(EXCLUDED.started_at,        jobs.started_at),
                completed_at      = COALESCE(EXCLUDED.completed_at,      jobs.completed_at),
                created_at        = jobs.created_at
            WHERE jobs.status = ANY($15::text[])
            "#,
            job_id,
            run_id,
            env.name,
            target_str as &str,
            conclusion_str as Option<&str>,
            &labels.clone() as &[String],
            steps_json,
            runner_id,
            runner_name,
            runner_group_id,
            runner_group_name,
            env.started_at,
            env.completed_at,
            env.created_at,
            &preds_strs as &[&str],
        )
        .execute(&self.pool)
        .await
        .map_err(|e| PersistError::Backend(Box::new(e)))?;

        if result.rows_affected() == 0 {
            return Err(PersistError::InvalidTransition);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Compile-time trait impl proof
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code, clippy::used_underscore_items)]
    fn _assert_pg_store_impls_trait() {
        fn _f<T: PersistentStore>() {}
        _f::<PgStore>();
    }
}
