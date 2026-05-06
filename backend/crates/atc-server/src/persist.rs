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
    Job, JobConclusion, JobId, JobStatus, PersistError, PersistentStore, RunConclusion, RunId,
    RunStatus, RunnerInfo, Step, WorkflowRun,
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
                updated_at     = EXCLUDED.updated_at,
                placeholder    = false
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
        // placeholder = true marks the stub for filtering out of /v1/state.
        // ON CONFLICT DO NOTHING: safe to call concurrently and repeatedly.
        sqlx::query!(
            r#"
            INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at, placeholder)
            VALUES ($1, $2, $3, '', '', '', '', 'Queued', $4, $4, true)
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
                runner_group_id   = CASE WHEN EXCLUDED.runner_id IS NOT NULL THEN EXCLUDED.runner_group_id   ELSE jobs.runner_group_id END,
                runner_group_name = CASE WHEN EXCLUDED.runner_id IS NOT NULL THEN EXCLUDED.runner_group_name ELSE jobs.runner_group_name END,
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
// Transaction helpers (outbox pattern)
// ---------------------------------------------------------------------------

/// Upsert a run event inside an open transaction.
///
/// Executes the same predicated UPSERT as [`PgStore::apply_run_event`] but
/// against an existing `Transaction<Postgres>` instead of the pool, so the
/// caller can group this with outbox inserts atomically.
///
/// Uses `&mut **tx` (double-deref through `Transaction<Postgres>` →
/// `PgConnection`) as required by sqlx 0.8's `Executor` bound.
#[allow(dead_code)]
pub(crate) async fn upsert_run_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &RunEventEnvelope,
) -> Result<(), PersistError> {
    let target = derive_run_target(&env.action);
    let preds = RunStatus::predecessors_of(target);
    let preds_strs: Vec<&'static str> = preds.iter().copied().map(SqlRepr::sql_repr).collect();
    let target_str = target.sql_repr();

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
            updated_at     = EXCLUDED.updated_at,
            placeholder    = false
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
    .execute(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    if result.rows_affected() == 0 {
        return Err(PersistError::InvalidTransition);
    }
    Ok(())
}

/// Upsert a job event inside an open transaction.
///
/// Executes the same stub-run preamble + predicated job UPSERT as
/// [`PgStore::apply_job_event`] but against an existing transaction. The FK
/// stub-row and the job row are written in the same transaction, so PostgreSQL
/// same-transaction visibility satisfies the FK check.
#[allow(dead_code)]
pub(crate) async fn upsert_job_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &JobEventEnvelope,
) -> Result<(), PersistError> {
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

    let steps_json = serde_json::to_value(steps).map_err(|e| PersistError::Backend(Box::new(e)))?;

    let run_id = env.run_id.0;
    let job_id = env.job_id.0;

    // Statement 1: Ensure a stub run row exists to satisfy FK.
    //
    // placeholder = true marks the row so `read_all_runs` can filter it out of
    // /v1/state. A subsequent workflow_run UPSERT will overwrite the stub
    // fields and leave placeholder = false (the column default), promoting it
    // to a real run row. This realigns PG /v1/state semantics with the
    // in-memory store, which never exposed FK-only stubs.
    sqlx::query!(
        r#"
        INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at, placeholder)
        VALUES ($1, $2, $3, '', '', '', '', 'Queued', $4, $4, true)
        ON CONFLICT (id) DO NOTHING
        "#,
        run_id,
        env.org,
        env.repo,
        env.created_at,
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    // Statement 2: Predicated job UPSERT.
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
            runner_group_id   = CASE WHEN EXCLUDED.runner_id IS NOT NULL THEN EXCLUDED.runner_group_id   ELSE jobs.runner_group_id END,
            runner_group_name = CASE WHEN EXCLUDED.runner_id IS NOT NULL THEN EXCLUDED.runner_group_name ELSE jobs.runner_group_name END,
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
    .execute(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    if result.rows_affected() == 0 {
        return Err(PersistError::InvalidTransition);
    }
    Ok(())
}

/// Insert a run event envelope into the outbox inside an open transaction.
///
/// Returns the `seq` (BIGSERIAL primary key) assigned to the inserted row.
#[allow(dead_code)]
pub(crate) async fn insert_outbox_run_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &RunEventEnvelope,
) -> Result<i64, PersistError> {
    let run_id = env.run_id.0;
    let payload = serde_json::to_value(env).map_err(|e| PersistError::Backend(Box::new(e)))?;

    let row = sqlx::query!(
        r#"
        INSERT INTO outbox (kind, run_id, payload) VALUES ('run', $1, $2::jsonb) RETURNING seq
        "#,
        run_id,
        payload,
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    Ok(row.seq)
}

/// Emit a PG NOTIFY for the given outbox row sequence number inside an open transaction.
///
/// PG queues NOTIFYs during a transaction and delivers them only on COMMIT.
/// Aborted transactions silently drop the NOTIFY — no notification if no row was written.
#[allow(dead_code)]
pub(crate) async fn notify_outbox_seq_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    seq: i64,
) -> Result<(), atc_core::PersistError> {
    sqlx::query!(
        "SELECT pg_notify($1::text, $2::text)",
        crate::listener::NOTIFY_CHANNEL,
        seq.to_string(),
    )
    .execute(&mut **tx)
    .await
    .map_err(|e| atc_core::PersistError::Backend(Box::new(e)))?;
    Ok(())
}

/// Insert a job event envelope into the outbox inside an open transaction.
///
/// Returns the `seq` (BIGSERIAL primary key) assigned to the inserted row.
#[allow(dead_code)]
pub(crate) async fn insert_outbox_job_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &JobEventEnvelope,
) -> Result<i64, PersistError> {
    let run_id = env.run_id.0;
    let job_id = env.job_id.0;
    let payload = serde_json::to_value(env).map_err(|e| PersistError::Backend(Box::new(e)))?;

    let row = sqlx::query!(
        r#"
        INSERT INTO outbox (kind, run_id, job_id, payload) VALUES ('job', $1, $2, $3::jsonb) RETURNING seq
        "#,
        run_id,
        job_id,
        payload,
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    Ok(row.seq)
}

// ---------------------------------------------------------------------------
// Read-side helpers for /v1/state PG snapshot (Phase 3c)
// ---------------------------------------------------------------------------

/// Parse a SQL CHECK constraint string back to [`RunStatus`].
///
/// Inverse of [`SqlRepr::sql_repr`]. The persistence layer writes only valid
/// constraint strings, so an unrecognized value indicates schema/code drift —
/// surfaced as [`PersistError::Backend`].
fn parse_run_status(s: &str) -> Result<RunStatus, PersistError> {
    match s {
        "Queued" => Ok(RunStatus::Queued),
        "InProgress" => Ok(RunStatus::InProgress),
        "Completed" => Ok(RunStatus::Completed),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown run status from PG: {other}"
        )))),
    }
}

fn parse_job_status(s: &str) -> Result<JobStatus, PersistError> {
    match s {
        "Queued" => Ok(JobStatus::Queued),
        "Waiting" => Ok(JobStatus::Waiting),
        "InProgress" => Ok(JobStatus::InProgress),
        "Completed" => Ok(JobStatus::Completed),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown job status from PG: {other}"
        )))),
    }
}

fn parse_run_conclusion(s: &str) -> Result<RunConclusion, PersistError> {
    match s {
        "Success" => Ok(RunConclusion::Success),
        "Failure" => Ok(RunConclusion::Failure),
        "Cancelled" => Ok(RunConclusion::Cancelled),
        "TimedOut" => Ok(RunConclusion::TimedOut),
        "ActionRequired" => Ok(RunConclusion::ActionRequired),
        "Stale" => Ok(RunConclusion::Stale),
        "Neutral" => Ok(RunConclusion::Neutral),
        "Skipped" => Ok(RunConclusion::Skipped),
        "StartupFailure" => Ok(RunConclusion::StartupFailure),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown run conclusion from PG: {other}"
        )))),
    }
}

fn parse_job_conclusion(s: &str) -> Result<JobConclusion, PersistError> {
    match s {
        "Success" => Ok(JobConclusion::Success),
        "Failure" => Ok(JobConclusion::Failure),
        "Cancelled" => Ok(JobConclusion::Cancelled),
        "TimedOut" => Ok(JobConclusion::TimedOut),
        "ActionRequired" => Ok(JobConclusion::ActionRequired),
        "Stale" => Ok(JobConclusion::Stale),
        "Neutral" => Ok(JobConclusion::Neutral),
        "Skipped" => Ok(JobConclusion::Skipped),
        other => Err(PersistError::Backend(Box::<
            dyn std::error::Error + Send + Sync,
        >::from(format!(
            "unknown job conclusion from PG: {other}"
        )))),
    }
}

/// Read all real (non-placeholder) runs ordered by id.
///
/// Filters out FK-only stub rows created by `upsert_job_in_txn` for
/// job-before-run delivery. Used by `state_handler`'s PG path to project
/// the PG state into the `StateSnapshot` wire contract.
#[allow(dead_code)]
pub(crate) async fn read_all_runs(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<WorkflowRun>, PersistError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, org, repo, workflow_name, workflow_path, branch, head_sha,
               commit_message, event, display_title, status, conclusion,
               html_url, created_at, run_started_at, updated_at
          FROM runs
         WHERE placeholder = false
         ORDER BY id
        "#,
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        let status = parse_run_status(&row.status)?;
        let conclusion = match row.conclusion {
            Some(s) => Some(parse_run_conclusion(&s)?),
            None => None,
        };
        runs.push(WorkflowRun {
            id: RunId(row.id),
            org: row.org,
            repo: row.repo,
            workflow_name: row.workflow_name,
            workflow_path: row.workflow_path,
            branch: row.branch,
            head_sha: row.head_sha,
            commit_message: row.commit_message,
            event: row.event,
            display_title: row.display_title,
            status,
            conclusion,
            html_url: row.html_url,
            created_at: row.created_at,
            run_started_at: row.run_started_at,
            updated_at: row.updated_at,
        });
    }
    Ok(runs)
}

/// Read all jobs ordered by id, reconstructing `RunnerInfo` and `steps`.
#[allow(dead_code)]
pub(crate) async fn read_all_jobs(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<Vec<Job>, PersistError> {
    let rows = sqlx::query!(
        r#"
        SELECT id, run_id, name, status, conclusion,
               runner_id, runner_name, runner_group_id, runner_group_name,
               labels, steps, created_at, started_at, completed_at
          FROM jobs
         ORDER BY id
        "#,
    )
    .fetch_all(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    let mut jobs = Vec::with_capacity(rows.len());
    for row in rows {
        let status = parse_job_status(&row.status)?;
        let conclusion = match row.conclusion {
            Some(s) => Some(parse_job_conclusion(&s)?),
            None => None,
        };
        // RunnerInfo is composed if runner_id/runner_name are present together.
        // The PG schema does not enforce this pairing; if either is missing we
        // treat runner as None.
        let runner = match (row.runner_id, row.runner_name) {
            (Some(id), Some(name)) => Some(RunnerInfo {
                id,
                name,
                group_id: row.runner_group_id,
                group_name: row.runner_group_name,
            }),
            _ => None,
        };
        let steps: Vec<Step> =
            serde_json::from_value(row.steps).map_err(|e| PersistError::Backend(Box::new(e)))?;
        jobs.push(Job {
            id: JobId(row.id),
            name: row.name,
            run_id: RunId(row.run_id),
            status,
            conclusion,
            runner,
            labels: row.labels,
            steps,
            created_at: row.created_at,
            started_at: row.started_at,
            completed_at: row.completed_at,
        });
    }
    Ok(jobs)
}

/// Read the highest committed outbox seq.
///
/// Returns the BIGSERIAL value (`i64`) — the caller converts to `u64` for the
/// wire format. `0` means the outbox is empty (no committed events).
#[allow(dead_code)]
pub(crate) async fn read_last_seq(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> Result<i64, PersistError> {
    let row = sqlx::query!(r#"SELECT COALESCE(MAX(seq), 0) AS "max!: i64" FROM outbox"#)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| PersistError::Backend(Box::new(e)))?;
    Ok(row.max)
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
