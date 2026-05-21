//! [`PersistentStore`] impl on [`PgStore`] plus the free-function transaction
//! helpers it groups (UPSERT + outbox INSERT + NOTIFY in a single
//! transaction).
//!
//! The impl-block side covers `apply_run_event`, `apply_job_event`,
//! `read_snapshot`, `subscribe`, `liveness_check`, and `shutdown`. The free
//! functions (`upsert_run_in_txn`, `upsert_job_in_txn`,
//! `insert_outbox_run_in_txn`, `insert_outbox_job_in_txn`,
//! `notify_outbox_seq_in_txn`) are exposed `pub(crate)` so future callers
//! that want to group the UPSERT-and-emit steps against a caller-managed
//! transaction can reuse them.

use std::sync::atomic::Ordering;

use atc_core::{
    JobStatus, PersistError, RunStatus,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use atc_persist::{LivenessError, PersistentStore, join_with_timeout};
use atc_wire::{CommittedEvent, StateSnapshot};
use tokio::sync::broadcast;

use crate::listener;
use crate::reads;

use super::{
    PgStore, SHUTDOWN_TIMEOUT_DRAIN, SHUTDOWN_TIMEOUT_LISTENER, SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT,
    SHUTDOWN_TIMEOUT_OUTBOX_SWEEP, SqlRepr, derive_job_target, derive_run_target,
};

#[async_trait::async_trait]
impl PersistentStore for PgStore {
    /// Return a consistent snapshot of all state.
    ///
    /// Loads `broadcast_watermark` (Acquire) BEFORE opening the snapshot
    /// transaction — this is the drain's commit-order cursor; every seq ≤ this
    /// value has been fetched by the drain and broadcast through `webhook_tx`.
    ///
    /// Opens a REPEATABLE READ transaction and reads runs/jobs from the same
    /// MVCC snapshot. The snapshot view is taken strictly AFTER the watermark
    /// load, so every row reflected in `lastSeq` is also visible in the snapshot.
    #[tracing::instrument(
        name = "persist.read.snapshot",
        skip_all,
        fields(last_seq = tracing::field::Empty, runs_count = tracing::field::Empty, jobs_count = tracing::field::Empty),
    )]
    async fn read_snapshot(&self) -> Result<StateSnapshot, PersistError> {
        // (1) Load the commit-order cursor BEFORE the snapshot view is taken.
        let watermark_at_start = self.broadcast_watermark.load(Ordering::Acquire);

        // (2) REPEATABLE READ tx around the runs/jobs reads.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| PersistError::Backend(Box::new(e)))?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .map_err(|e| PersistError::Backend(Box::new(e)))?;

        let runs = reads::read_all_runs(&mut tx).await?;
        let jobs = reads::read_all_jobs(&mut tx).await?;

        if let Err(e) = tx.commit().await {
            tracing::warn!(error = %e, "read_snapshot: pg commit failed");
            // Reads succeeded; fall through and return them. A failed commit
            // on a read-only REPEATABLE READ tx is non-fatal for the response.
        }

        let last_seq = u64::try_from(watermark_at_start).unwrap_or(0);
        let snap = StateSnapshot {
            last_seq,
            runs,
            jobs,
            // Operator-declared capacities live in `AppState`, not the store —
            // composed into the response by `routes::state_handler`.
            runner_pool_capacities: Vec::new(),
        };
        let span = tracing::Span::current();
        span.record("last_seq", snap.last_seq);
        span.record("runs_count", snap.runs.len());
        span.record("jobs_count", snap.jobs.len());
        Ok(snap)
    }

    fn subscribe(&self) -> broadcast::Receiver<CommittedEvent> {
        self.broadcast_tx.subscribe()
    }

    async fn shutdown(&self) {
        // Drop the std::sync::Mutex guard BEFORE awaiting — holding it across
        // `.await` would `!Send` the future and break the `async_trait` bound.
        //
        // Callers must cancel the shutdown token they passed to `start()` /
        // `start_with_test_hooks()` before invoking `shutdown()` — otherwise
        // the listener / drain / heartbeat / sweep tasks never observe
        // cancellation and this method waits the full per-task timeout
        // budget before aborting them. Production: `run_shutdown_orchestration`
        // cancels the token in step 1 before calling `persist.shutdown()` in
        // step 4. Tests: see `common::start_pg_store_for_test`, which
        // returns the token to the caller for end-of-test cancel.
        let handles = self.handles.lock().expect("handles mutex poisoned").take();
        if let Some(handles) = handles {
            // Join drain first: it owns the broadcast write end and may still
            // be advancing the watermark. Listener exits within one
            // `select!` iteration once the shutdown token cancels. Heartbeat
            // and sweep are bounded by their own small timeouts since each
            // wakes on either cancellation or its tick interval.
            join_with_timeout(handles.drain, SHUTDOWN_TIMEOUT_DRAIN, "drain").await;
            join_with_timeout(handles.listener, SHUTDOWN_TIMEOUT_LISTENER, "listener").await;
            join_with_timeout(
                handles.heartbeat,
                SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT,
                "outbox_heartbeat",
            )
            .await;
            if let Some(sweep) = handles.sweep {
                join_with_timeout(sweep, SHUTDOWN_TIMEOUT_OUTBOX_SWEEP, "outbox_sweep").await;
            }
        }
    }

    /// Check liveness: SELECT 1 for DB connectivity, then check drain heartbeat age.
    ///
    /// The drain task refreshes the heartbeat at the top of every loop iteration
    /// (whether woken by NOTIFY or the 5 s heartbeat tick). If the heartbeat is
    /// older than 30 s the drain task has stalled; return `DrainStale`.
    async fn liveness_check(&self) -> Result<(), LivenessError> {
        if let Err(e) = sqlx::query("SELECT 1").execute(&self.pool).await {
            return Err(LivenessError::DbUnreachable(Box::new(e)));
        }

        let now_ms = self.clock.now().timestamp_millis();
        let last = self.last_drain_pass_at.load(Ordering::Relaxed);
        let age = now_ms.saturating_sub(last);
        const READYZ_HEARTBEAT_STALENESS_MS: i64 = 30_000;
        if age > READYZ_HEARTBEAT_STALENESS_MS {
            return Err(LivenessError::DrainStale { age_ms: age });
        }

        Ok(())
    }

    /// Upsert a run event inside a new transaction: UPSERT + outbox INSERT + NOTIFY → commit.
    ///
    /// Returns the outbox `seq` (converted from BIGSERIAL `i64` to `u64` at this boundary).
    /// Zero rows affected in the predicated UPSERT maps to [`PersistError::InvalidTransition`].
    ///
    /// Emits `atc_pg_write_failures_total{kind="transient"}` on pool/commit failures,
    /// `atc_pg_write_failures_total{kind="parity"}` on predicate rejections, and
    /// `atc_pg_notify_emitted_total{kind="run"}` after a successful commit.
    #[tracing::instrument(
        name = "persist.apply.run_event",
        skip_all,
        fields(run_id = env.run_id.0, seq = tracing::field::Empty),
    )]
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            self.metrics.write_failure_transient();
            PersistError::Backend(Box::new(e))
        })?;
        match upsert_run_in_txn(&mut tx, &env).await {
            Ok(()) => {}
            Err(PersistError::InvalidTransition) => {
                self.metrics.write_failure_parity();
                return Err(PersistError::InvalidTransition);
            }
            Err(e) => {
                self.metrics.write_failure_transient();
                return Err(e);
            }
        }
        let seq_i64 = insert_outbox_run_in_txn(&mut tx, &env)
            .await
            .inspect_err(|_| {
                self.metrics.write_failure_transient();
            })?;
        tracing::Span::current().record("seq", seq_i64);
        notify_outbox_seq_in_txn(&mut tx, "run", seq_i64)
            .await
            .inspect_err(|_| {
                self.metrics.write_failure_transient();
            })?;
        tx.commit().await.map_err(|e| {
            self.metrics.write_failure_transient();
            PersistError::Backend(Box::new(e))
        })?;
        // Emit AFTER commit: PG delivers NOTIFYs on COMMIT; aborted txns drop them.
        self.metrics.notify_emitted_run();
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
    #[tracing::instrument(
        name = "persist.apply.job_event",
        skip_all,
        fields(run_id = env.run_id.0, job_id = env.job_id.0, seq = tracing::field::Empty),
    )]
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError> {
        let mut tx = self.pool.begin().await.map_err(|e| {
            self.metrics.write_failure_transient();
            PersistError::Backend(Box::new(e))
        })?;
        match upsert_job_in_txn(&mut tx, &env).await {
            Ok(()) => {}
            Err(PersistError::InvalidTransition) => {
                self.metrics.write_failure_parity();
                return Err(PersistError::InvalidTransition);
            }
            Err(e) => {
                self.metrics.write_failure_transient();
                return Err(e);
            }
        }
        let seq_i64 = insert_outbox_job_in_txn(&mut tx, &env)
            .await
            .inspect_err(|_| {
                self.metrics.write_failure_transient();
            })?;
        tracing::Span::current().record("seq", seq_i64);
        notify_outbox_seq_in_txn(&mut tx, "job", seq_i64)
            .await
            .inspect_err(|_| {
                self.metrics.write_failure_transient();
            })?;
        tx.commit().await.map_err(|e| {
            self.metrics.write_failure_transient();
            PersistError::Backend(Box::new(e))
        })?;
        // Emit AFTER commit: PG delivers NOTIFYs on COMMIT; aborted txns drop them.
        self.metrics.notify_emitted_job();
        // BIGSERIAL is always positive; conversion is infallible.
        Ok(u64::try_from(seq_i64).expect("BIGSERIAL is non-negative"))
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
#[tracing::instrument(skip_all, fields(run_id = env.run_id.0))]
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
        tracing::warn!(
            target_status = target_str,
            "run predicated UPSERT rejected (0 rows affected)"
        );
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
#[tracing::instrument(skip_all, fields(run_id = env.run_id.0, job_id = env.job_id.0))]
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
    let runner_group_name: Option<&str> = runner.and_then(|r| r.group_name.as_deref());

    let result = sqlx::query!(
        r#"
        INSERT INTO jobs (
            id, run_id, name, status, conclusion, labels, steps,
            runner_id, runner_name, runner_group_name,
            started_at, completed_at, created_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7,
            $8, $9, $10,
            $11, $12, $13
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
            runner_group_name = CASE WHEN EXCLUDED.runner_id IS NOT NULL THEN EXCLUDED.runner_group_name ELSE jobs.runner_group_name END,
            started_at        = COALESCE(EXCLUDED.started_at,        jobs.started_at),
            completed_at      = COALESCE(EXCLUDED.completed_at,      jobs.completed_at),
            created_at        = jobs.created_at
        WHERE jobs.status = ANY($14::text[])
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
        tracing::warn!(
            target_status = target_str,
            "job predicated UPSERT rejected (0 rows affected)"
        );
        return Err(PersistError::InvalidTransition);
    }
    Ok(())
}

/// Insert a run event envelope into the outbox inside an open transaction.
///
/// Returns the `seq` (BIGSERIAL primary key) assigned to the inserted row.
#[allow(dead_code)]
#[tracing::instrument(skip_all, fields(run_id = env.run_id.0))]
pub(crate) async fn insert_outbox_run_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &RunEventEnvelope,
) -> Result<i64, PersistError> {
    let run_id = env.run_id.0;
    let payload = serde_json::to_value(env).map_err(|e| PersistError::Backend(Box::new(e)))?;
    // Capture the current span's W3C traceparent. Stored on the outbox row so
    // the drain task can attach an OTel span link from `drain.broadcast` back
    // to this `webhook.handler` trace. `None` under no-op OTel SDK.
    let traceparent = crate::traceparent::current();

    let row = sqlx::query!(
        r#"
        INSERT INTO outbox (kind, run_id, payload, traceparent)
        VALUES ('run', $1, $2::jsonb, $3)
        RETURNING seq
        "#,
        run_id,
        payload,
        traceparent.as_deref(),
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
#[tracing::instrument(
    name = "persist.notify.emit",
    skip(tx),
    fields(notify.kind = kind, notify.seq = seq),
)]
pub(crate) async fn notify_outbox_seq_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    kind: &'static str,
    seq: i64,
) -> Result<(), atc_core::PersistError> {
    sqlx::query!(
        "SELECT pg_notify($1::text, $2::text)",
        listener::NOTIFY_CHANNEL,
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
#[tracing::instrument(skip_all, fields(run_id = env.run_id.0, job_id = env.job_id.0))]
pub(crate) async fn insert_outbox_job_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    env: &JobEventEnvelope,
) -> Result<i64, PersistError> {
    let run_id = env.run_id.0;
    let job_id = env.job_id.0;
    let payload = serde_json::to_value(env).map_err(|e| PersistError::Backend(Box::new(e)))?;
    // See `insert_outbox_run_in_txn` for the traceparent rationale.
    let traceparent = crate::traceparent::current();

    let row = sqlx::query!(
        r#"
        INSERT INTO outbox (kind, run_id, job_id, payload, traceparent)
        VALUES ('job', $1, $2, $3::jsonb, $4)
        RETURNING seq
        "#,
        run_id,
        job_id,
        payload,
        traceparent.as_deref(),
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| PersistError::Backend(Box::new(e)))?;

    Ok(row.seq)
}
