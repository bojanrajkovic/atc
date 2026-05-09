//! Persistence abstraction and backends for domain events.
//!
//! Defines [`PersistentStore`], a common interface over any backend that can
//! durably apply domain events. Two implementations live here:
//!
//! - [`PgStore`] — PostgreSQL-backed. Opens its own transaction per event:
//!   UPSERT + outbox INSERT + `pg_notify` → commit → returns allocated seq.
//! - [`InMemoryStore`] — In-memory-only (dev/test). Holds `Arc<RunStateMachine>`
//!   + `Arc<Mutex<u64>>` + broadcast sender; locks seq, applies, broadcasts, returns seq.
//!
//! The module also contains a [`SqlRepr`] trait implemented for the status and
//! conclusion enums. Implementations match the SQL CHECK constraint values exactly,
//! independent of serde representation so a future `#[serde(rename)]` cannot
//! silently break a SQL bind.

use std::sync::Arc;

use atc_core::{
    Job, JobConclusion, JobId, JobStatus, PersistError, RunConclusion, RunId, RunStateMachine,
    RunStatus, RunnerInfo, Step, WorkflowRun,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use atc_github::WebhookEvent;
use sqlx::PgPool;
use tokio::sync::{Mutex, broadcast};

use crate::state::SeqEvent;

// ---------------------------------------------------------------------------
// PersistentStore trait
// ---------------------------------------------------------------------------

/// A store that can durably apply domain events and return the allocated seq.
///
/// - [`PgStore`]: opens its own transaction per call (UPSERT + outbox + notify → commit).
/// - [`InMemoryStore`]: locks seq, applies to `RunStateMachine`, broadcasts, returns seq.
///
/// Implementations must be `Send + Sync` for use behind `Arc` in async contexts.
#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    /// Apply a run event envelope, creating or updating the corresponding run.
    /// Returns the monotonic seq assigned to this event.
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError>;

    /// Apply a job event envelope, creating or updating the corresponding job.
    /// Returns the monotonic seq assigned to this event.
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError>;
}

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
    /// Upsert a run event inside a new transaction: UPSERT + outbox INSERT + NOTIFY → commit.
    ///
    /// Returns the outbox `seq` (converted from BIGSERIAL `i64` to `u64` at this boundary).
    /// Zero rows affected in the predicated UPSERT maps to [`PersistError::InvalidTransition`].
    ///
    /// Emits `atc_pg_write_failures_total{kind="transient"}` on pool/commit failures,
    /// `atc_pg_write_failures_total{kind="parity"}` on predicate rejections, and
    /// `atc_pg_notify_emitted_total{kind="run"}` after a successful commit.
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient").increment(1);
            PersistError::Backend(Box::new(e))
        })?;
        match upsert_run_in_txn(&mut tx, &env).await {
            Ok(()) => {}
            Err(PersistError::InvalidTransition) => {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "parity").increment(1);
                return Err(PersistError::InvalidTransition);
            }
            Err(e) => {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
                return Err(e);
            }
        }
        let seq_i64 = insert_outbox_run_in_txn(&mut tx, &env)
            .await
            .inspect_err(|_| {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
            })?;
        notify_outbox_seq_in_txn(&mut tx, seq_i64)
            .await
            .inspect_err(|_| {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
            })?;
        tx.commit().await.map_err(|e| {
            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient").increment(1);
            PersistError::Backend(Box::new(e))
        })?;
        // Emit AFTER commit: PG delivers NOTIFYs on COMMIT; aborted txns drop them.
        metrics::counter!("atc_pg_notify_emitted_total", "kind" => "run").increment(1);
        // BIGSERIAL is always positive; conversion is infallible.
        Ok(u64::try_from(seq_i64).expect("BIGSERIAL is non-negative"))
    }

    /// Upsert a job event inside a new transaction: UPSERT + outbox INSERT + NOTIFY → commit.
    ///
    /// Returns the outbox `seq`. Zero rows affected maps to [`PersistError::InvalidTransition`].
    ///
    /// Emits `atc_pg_write_failures_total{kind="transient"}` on pool/commit failures,
    /// `atc_pg_write_failures_total{kind="parity"}` on predicate rejections, and
    /// `atc_pg_notify_emitted_total{kind="job"}` after a successful commit.
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient").increment(1);
            PersistError::Backend(Box::new(e))
        })?;
        match upsert_job_in_txn(&mut tx, &env).await {
            Ok(()) => {}
            Err(PersistError::InvalidTransition) => {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "parity").increment(1);
                return Err(PersistError::InvalidTransition);
            }
            Err(e) => {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
                return Err(e);
            }
        }
        let seq_i64 = insert_outbox_job_in_txn(&mut tx, &env)
            .await
            .inspect_err(|_| {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
            })?;
        notify_outbox_seq_in_txn(&mut tx, seq_i64)
            .await
            .inspect_err(|_| {
                metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                    .increment(1);
            })?;
        tx.commit().await.map_err(|e| {
            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient").increment(1);
            PersistError::Backend(Box::new(e))
        })?;
        // Emit AFTER commit: PG delivers NOTIFYs on COMMIT; aborted txns drop them.
        metrics::counter!("atc_pg_notify_emitted_total", "kind" => "job").increment(1);
        // BIGSERIAL is always positive; conversion is infallible.
        Ok(u64::try_from(seq_i64).expect("BIGSERIAL is non-negative"))
    }
}

// ---------------------------------------------------------------------------
// InMemoryStore
// ---------------------------------------------------------------------------

/// In-memory backend for [`PersistentStore`] (dev/test only).
///
/// Holds a reference to the `RunStateMachine` state machine, a shared seq
/// counter, and a broadcast sender. On each successful apply the seq is
/// incremented under the same lock that serializes the apply + broadcast,
/// ensuring WebSocket event order matches ingestion order.
pub struct InMemoryStore {
    /// Domain state machine for workflow runs and jobs.
    state_machine: Arc<RunStateMachine>,
    /// Monotonic event counter. Shared with `AppState.seq` via `Arc`.
    seq: Arc<Mutex<u64>>,
    /// Broadcast sender for pushing domain events to WebSocket clients.
    broadcast_tx: broadcast::Sender<SeqEvent>,
}

impl InMemoryStore {
    /// Create a new [`InMemoryStore`].
    pub fn new(
        state_machine: Arc<RunStateMachine>,
        seq: Arc<Mutex<u64>>,
        broadcast_tx: broadcast::Sender<SeqEvent>,
    ) -> Self {
        Self {
            state_machine,
            seq,
            broadcast_tx,
        }
    }
}

#[async_trait::async_trait]
impl PersistentStore for InMemoryStore {
    /// Apply a run event to the in-memory state machine and broadcast.
    ///
    /// Acquires the seq mutex before the apply so that WS event order matches
    /// ingestion order. On invalid transition returns `Err(PersistError::InvalidTransition)`
    /// without incrementing seq or emitting a broadcast.
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut guard = self.seq.lock().await;
        // `?` auto-converts StateMachineError → PersistError::InvalidTransition via From impl.
        self.state_machine
            .apply_run_event(env.clone())
            .await
            .map_err(atc_core::PersistError::from)?;
        *guard += 1;
        let allocated = *guard;
        let _ = self.broadcast_tx.send(SeqEvent {
            seq: allocated,
            event: WebhookEvent::Run(env),
        });
        Ok(allocated)
    }

    /// Apply a job event to the in-memory state machine and broadcast.
    ///
    /// Same locking semantics as [`apply_run_event`]. Invalid transitions return
    /// `Err(PersistError::InvalidTransition)` without side effects.
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError> {
        let mut guard = self.seq.lock().await;
        self.state_machine
            .apply_job_event(env.clone())
            .await
            .map_err(atc_core::PersistError::from)?;
        *guard += 1;
        let allocated = *guard;
        let _ = self.broadcast_tx.send(SeqEvent {
            seq: allocated,
            event: WebhookEvent::Job(env),
        });
        Ok(allocated)
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
// Read-side helpers for /v1/state PG snapshot
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use atc_core::event::{JobEvent, RunEvent};
    use atc_core::{JobConclusion, JobStatus, RunConclusion, RunStatus};

    use super::*;

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

    // ---------------------------------------------------------------------------
    // parse_* error paths — cover the unknown-string Err arms (Goal 1 / Codecov)
    // ---------------------------------------------------------------------------

    #[test]
    fn parse_run_status_valid_variants() {
        assert!(matches!(parse_run_status("Queued"), Ok(RunStatus::Queued)));
        assert!(matches!(
            parse_run_status("InProgress"),
            Ok(RunStatus::InProgress)
        ));
        assert!(matches!(
            parse_run_status("Completed"),
            Ok(RunStatus::Completed)
        ));
    }

    #[test]
    fn parse_run_status_unknown_returns_backend_error() {
        let result = parse_run_status("Frobnicated");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Frobnicated"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_job_status_valid_variants() {
        assert!(matches!(parse_job_status("Queued"), Ok(JobStatus::Queued)));
        assert!(matches!(
            parse_job_status("Waiting"),
            Ok(JobStatus::Waiting)
        ));
        assert!(matches!(
            parse_job_status("InProgress"),
            Ok(JobStatus::InProgress)
        ));
        assert!(matches!(
            parse_job_status("Completed"),
            Ok(JobStatus::Completed)
        ));
    }

    #[test]
    fn parse_job_status_unknown_returns_backend_error() {
        let result = parse_job_status("Obliterated");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Obliterated"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_run_conclusion_valid_variants() {
        assert!(matches!(
            parse_run_conclusion("Success"),
            Ok(RunConclusion::Success)
        ));
        assert!(matches!(
            parse_run_conclusion("Failure"),
            Ok(RunConclusion::Failure)
        ));
        assert!(matches!(
            parse_run_conclusion("Cancelled"),
            Ok(RunConclusion::Cancelled)
        ));
        assert!(matches!(
            parse_run_conclusion("TimedOut"),
            Ok(RunConclusion::TimedOut)
        ));
        assert!(matches!(
            parse_run_conclusion("ActionRequired"),
            Ok(RunConclusion::ActionRequired)
        ));
        assert!(matches!(
            parse_run_conclusion("Stale"),
            Ok(RunConclusion::Stale)
        ));
        assert!(matches!(
            parse_run_conclusion("Neutral"),
            Ok(RunConclusion::Neutral)
        ));
        assert!(matches!(
            parse_run_conclusion("Skipped"),
            Ok(RunConclusion::Skipped)
        ));
        assert!(matches!(
            parse_run_conclusion("StartupFailure"),
            Ok(RunConclusion::StartupFailure)
        ));
    }

    #[test]
    fn parse_run_conclusion_unknown_returns_backend_error() {
        let result = parse_run_conclusion("Exploded");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Exploded"),
            "error should mention the bad input; got {err_str}"
        );
    }

    #[test]
    fn parse_job_conclusion_valid_variants() {
        assert!(matches!(
            parse_job_conclusion("Success"),
            Ok(JobConclusion::Success)
        ));
        assert!(matches!(
            parse_job_conclusion("Failure"),
            Ok(JobConclusion::Failure)
        ));
        assert!(matches!(
            parse_job_conclusion("Cancelled"),
            Ok(JobConclusion::Cancelled)
        ));
        assert!(matches!(
            parse_job_conclusion("TimedOut"),
            Ok(JobConclusion::TimedOut)
        ));
        assert!(matches!(
            parse_job_conclusion("ActionRequired"),
            Ok(JobConclusion::ActionRequired)
        ));
        assert!(matches!(
            parse_job_conclusion("Stale"),
            Ok(JobConclusion::Stale)
        ));
        assert!(matches!(
            parse_job_conclusion("Neutral"),
            Ok(JobConclusion::Neutral)
        ));
        assert!(matches!(
            parse_job_conclusion("Skipped"),
            Ok(JobConclusion::Skipped)
        ));
    }

    #[test]
    fn parse_job_conclusion_unknown_returns_backend_error() {
        let result = parse_job_conclusion("Vaporized");
        assert!(
            matches!(result, Err(PersistError::Backend(_))),
            "expected Backend error, got: {result:?}"
        );
        let err_str = format!("{:?}", result.unwrap_err());
        assert!(
            err_str.contains("Vaporized"),
            "error should mention the bad input; got {err_str}"
        );
    }
}

// ---------------------------------------------------------------------------
// InMemoryStore unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod inmem_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use atc_core::event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope};
    use atc_core::types::{JobId, RunId};
    use atc_core::{PersistError, RunConclusion, RunStateMachine, SystemClock};
    use chrono::Utc;
    use tokio::sync::{Mutex, broadcast};

    use super::{InMemoryStore, PersistentStore};
    use crate::state::SeqEvent;

    fn make_store() -> (InMemoryStore, broadcast::Receiver<SeqEvent>) {
        let state_machine = Arc::new(RunStateMachine::new(
            Arc::new(SystemClock),
            Duration::from_secs(3600),
        ));
        let seq = Arc::new(Mutex::new(0u64));
        let (tx, rx) = broadcast::channel::<SeqEvent>(256);
        let store = InMemoryStore::new(state_machine, seq, tx);
        (store, rx)
    }

    fn run_env(run_id: i64, action: RunEvent) -> RunEventEnvelope {
        RunEventEnvelope {
            run_id: RunId(run_id),
            org: "org".into(),
            repo: "repo".into(),
            workflow_name: None,
            workflow_path: None,
            branch: Some("main".into()),
            head_sha: "abc".into(),
            commit_message: None,
            trigger_event: "push".into(),
            display_title: "run".into(),
            html_url: "https://github.com/".into(),
            created_at: Utc::now(),
            run_started_at: None,
            updated_at: Utc::now(),
            action,
        }
    }

    fn job_env(job_id: i64, run_id: i64, action: JobEvent) -> JobEventEnvelope {
        JobEventEnvelope {
            job_id: JobId(job_id),
            run_id: RunId(run_id),
            org: "org".into(),
            repo: "repo".into(),
            name: "job".into(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            action,
        }
    }

    /// Seq monotonicity: first apply_run_event returns 1, second returns 2,
    /// after 100 mixed run+job calls the final seq is 100.
    #[tokio::test]
    async fn seq_is_monotonic() {
        let (store, _rx) = make_store();

        let s1 = store
            .apply_run_event(run_env(1, RunEvent::Requested))
            .await
            .unwrap();
        assert_eq!(s1, 1, "first call must return seq=1");

        let s2 = store
            .apply_run_event(run_env(1, RunEvent::InProgress))
            .await
            .unwrap();
        assert_eq!(s2, 2, "second call must return seq=2");

        // 98 more mixed calls → seq should reach 100.
        for i in 0u64..49 {
            store
                .apply_run_event(run_env(100 + i as i64, RunEvent::Requested))
                .await
                .unwrap();
            store
                .apply_job_event(job_env(
                    200 + i as i64,
                    100 + i as i64,
                    JobEvent::Queued {
                        labels: vec![],
                        steps: vec![],
                    },
                ))
                .await
                .unwrap();
        }

        let final_seq = store
            .apply_run_event(run_env(999, RunEvent::Requested))
            .await
            .unwrap();
        assert_eq!(final_seq, 101, "after 101 calls seq must be 101");
    }

    /// Broadcast emission on success: a subscriber receives one SeqEvent per
    /// successful apply, and the seq field in the event matches the return value.
    #[tokio::test]
    async fn broadcast_emitted_on_success() {
        let (store, mut rx) = make_store();

        let seq = store
            .apply_run_event(run_env(1, RunEvent::Requested))
            .await
            .unwrap();

        let ev = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv())
            .await
            .expect("timed out waiting for broadcast")
            .expect("channel closed");

        assert_eq!(ev.seq, seq, "broadcast seq must match returned seq");
        assert_eq!(seq, 1, "first call returns seq=1");
    }

    /// Invalid-transition behavior: Completed → InProgress returns
    /// Err(PersistError::InvalidTransition); seq is NOT incremented;
    /// no broadcast is emitted.
    #[tokio::test]
    async fn invalid_transition_no_seq_no_broadcast() {
        let (store, mut rx) = make_store();

        // Advance to Completed.
        store
            .apply_run_event(run_env(1, RunEvent::Requested))
            .await
            .unwrap();
        store
            .apply_run_event(run_env(
                1,
                RunEvent::Completed {
                    conclusion: RunConclusion::Success,
                },
            ))
            .await
            .unwrap();

        // Drain the two successful broadcasts.
        rx.recv().await.unwrap();
        rx.recv().await.unwrap();

        let seq_before = 2u64;

        // Invalid transition: Completed → InProgress.
        let result = store
            .apply_run_event(run_env(1, RunEvent::InProgress))
            .await;
        assert!(
            matches!(result, Err(PersistError::InvalidTransition)),
            "expected InvalidTransition, got {result:?}"
        );

        // Seq must not have advanced.
        let seq_after = *store.seq.lock().await;
        assert_eq!(
            seq_after, seq_before,
            "seq must not increment on invalid transition"
        );

        // No broadcast emitted.
        let broadcast_result =
            tokio::time::timeout(std::time::Duration::from_millis(20), rx.recv()).await;
        assert!(
            broadcast_result.is_err(),
            "no broadcast should be emitted on invalid transition"
        );
    }

    /// Concurrency: two apply_run_event tasks from different tokio tasks produce
    /// sequential, non-interleaved seqs (the set of seqs is {1, 2}).
    #[tokio::test]
    async fn concurrent_applies_produce_sequential_seqs() {
        let state_machine = Arc::new(RunStateMachine::new(
            Arc::new(SystemClock),
            Duration::from_secs(3600),
        ));
        let seq = Arc::new(Mutex::new(0u64));
        let (tx, _rx) = broadcast::channel::<SeqEvent>(256);
        let store = Arc::new(InMemoryStore::new(state_machine, seq, tx));

        let s1 = Arc::clone(&store);
        let s2 = Arc::clone(&store);

        let h1 = tokio::spawn(async move {
            s1.apply_run_event(run_env(1, RunEvent::Requested))
                .await
                .unwrap()
        });
        let h2 = tokio::spawn(async move {
            s2.apply_run_event(run_env(2, RunEvent::Requested))
                .await
                .unwrap()
        });

        let (r1, r2) = tokio::join!(h1, h2);
        let seq1 = r1.unwrap();
        let seq2 = r2.unwrap();

        let mut seqs = [seq1, seq2];
        seqs.sort_unstable();
        assert_eq!(seqs, [1, 2], "concurrent applies must produce seqs {{1,2}}");
    }
}
