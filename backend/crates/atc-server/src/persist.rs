//! PostgreSQL-backed persistence for domain events.
//!
//! [`PgStore`] implements [`atc_core::PersistentStore`] against a live
//! PostgreSQL connection pool. Every write is a single predicated UPSERT:
//! `INSERT ... ON CONFLICT (id) DO UPDATE ... WHERE status = ANY($preds::text[])`.
//! Zero rows affected ⇒ [`PersistError::InvalidTransition`].
//!
//! The module also contains a [`SqlRepr`] trait implemented for the status and
//! conclusion enums. Implementations match the SQL CHECK constraint values exactly,
//! independent of serde representation so a future `#[serde(rename)]` cannot
//! silently break a SQL bind.

use atc_core::{
    JobConclusion, JobStatus, PersistError, PersistentStore, RunConclusion, RunStatus,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use sqlx::PgPool;

// ---------------------------------------------------------------------------
// SqlRepr: status/conclusion → SQL CHECK constraint string
// ---------------------------------------------------------------------------

/// Maps a value to its SQL CHECK constraint string representation.
///
/// Must exactly match the `CHECK (... IN (...))` constraints in
/// `0001_initial_runs_jobs.sql`.
trait SqlRepr {
    fn sql_repr(self) -> &'static str;
}

impl SqlRepr for RunStatus {
    fn sql_repr(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::InProgress => "InProgress",
            Self::Completed => "Completed",
        }
    }
}

impl SqlRepr for JobStatus {
    fn sql_repr(self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Waiting => "Waiting",
            Self::InProgress => "InProgress",
            Self::Completed => "Completed",
        }
    }
}

impl SqlRepr for RunConclusion {
    fn sql_repr(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Cancelled => "Cancelled",
            Self::TimedOut => "TimedOut",
            Self::ActionRequired => "ActionRequired",
            Self::Stale => "Stale",
            Self::Neutral => "Neutral",
            Self::Skipped => "Skipped",
            Self::StartupFailure => "StartupFailure",
        }
    }
}

impl SqlRepr for JobConclusion {
    fn sql_repr(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Failure => "Failure",
            Self::Cancelled => "Cancelled",
            Self::TimedOut => "TimedOut",
            Self::ActionRequired => "ActionRequired",
            Self::Stale => "Stale",
            Self::Neutral => "Neutral",
            Self::Skipped => "Skipped",
        }
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
        let preds_strs: Vec<&'static str> = preds.iter().copied().map(SqlRepr::sql_repr).collect();
        let target_str = target.sql_repr();

        // Conclusion is only set on RunEvent::Completed.
        let conclusion_str: Option<&'static str> =
            if let RunEvent::Completed { conclusion } = &env.action {
                Some(conclusion.sql_repr())
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
        let preds_strs: Vec<&'static str> = preds.iter().copied().map(SqlRepr::sql_repr).collect();
        let target_str = target.sql_repr();

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
            } => (Some(conclusion.sql_repr()), labels, steps, runner.as_ref()),
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use atc_core::event::{JobEvent, RunEvent};
    use atc_core::{JobConclusion, JobStatus, RunConclusion, RunStatus};

    use super::*;

    #[allow(dead_code, clippy::used_underscore_items)]
    fn _assert_pg_store_impls_trait() {
        fn _f<T: PersistentStore>() {}
        _f::<PgStore>();
    }

    #[test]
    fn run_status_sql_repr() {
        assert_eq!(RunStatus::Queued.sql_repr(), "Queued");
        assert_eq!(RunStatus::InProgress.sql_repr(), "InProgress");
        assert_eq!(RunStatus::Completed.sql_repr(), "Completed");
    }

    #[test]
    fn job_status_sql_repr() {
        assert_eq!(JobStatus::Queued.sql_repr(), "Queued");
        assert_eq!(JobStatus::Waiting.sql_repr(), "Waiting");
        assert_eq!(JobStatus::InProgress.sql_repr(), "InProgress");
        assert_eq!(JobStatus::Completed.sql_repr(), "Completed");
    }

    #[test]
    fn run_conclusion_sql_repr() {
        assert_eq!(RunConclusion::Success.sql_repr(), "Success");
        assert_eq!(RunConclusion::Failure.sql_repr(), "Failure");
        assert_eq!(RunConclusion::Cancelled.sql_repr(), "Cancelled");
        assert_eq!(RunConclusion::TimedOut.sql_repr(), "TimedOut");
        assert_eq!(RunConclusion::ActionRequired.sql_repr(), "ActionRequired");
        assert_eq!(RunConclusion::Stale.sql_repr(), "Stale");
        assert_eq!(RunConclusion::Neutral.sql_repr(), "Neutral");
        assert_eq!(RunConclusion::Skipped.sql_repr(), "Skipped");
        assert_eq!(RunConclusion::StartupFailure.sql_repr(), "StartupFailure");
    }

    #[test]
    fn job_conclusion_sql_repr() {
        assert_eq!(JobConclusion::Success.sql_repr(), "Success");
        assert_eq!(JobConclusion::Failure.sql_repr(), "Failure");
        assert_eq!(JobConclusion::Cancelled.sql_repr(), "Cancelled");
        assert_eq!(JobConclusion::TimedOut.sql_repr(), "TimedOut");
        assert_eq!(JobConclusion::ActionRequired.sql_repr(), "ActionRequired");
        assert_eq!(JobConclusion::Stale.sql_repr(), "Stale");
        assert_eq!(JobConclusion::Neutral.sql_repr(), "Neutral");
        assert_eq!(JobConclusion::Skipped.sql_repr(), "Skipped");
    }

    #[test]
    fn derive_run_target_all_variants() {
        assert_eq!(derive_run_target(&RunEvent::Requested), RunStatus::Queued);
        assert_eq!(
            derive_run_target(&RunEvent::InProgress),
            RunStatus::InProgress
        );
        assert_eq!(
            derive_run_target(&RunEvent::Completed {
                conclusion: RunConclusion::Success
            }),
            RunStatus::Completed
        );
    }

    #[test]
    fn derive_job_target_all_variants() {
        assert_eq!(
            derive_job_target(&JobEvent::Queued {
                labels: vec![],
                steps: vec![]
            }),
            JobStatus::Queued
        );
        assert_eq!(
            derive_job_target(&JobEvent::Waiting {
                labels: vec![],
                steps: vec![]
            }),
            JobStatus::Waiting
        );
        assert_eq!(
            derive_job_target(&JobEvent::InProgress {
                runner: None,
                labels: vec![],
                steps: vec![]
            }),
            JobStatus::InProgress
        );
        assert_eq!(
            derive_job_target(&JobEvent::Completed {
                conclusion: JobConclusion::Success,
                runner: None,
                labels: vec![],
                steps: vec![]
            }),
            JobStatus::Completed
        );
    }
}
