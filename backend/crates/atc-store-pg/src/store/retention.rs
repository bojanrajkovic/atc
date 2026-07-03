//! Outbox retention — heartbeat and sweep background tasks.
//!
//! Both tasks are spawned by `PgStore::start_inner` and joined by
//! [`super::PgStore::shutdown`] under the per-task shutdown budgets defined in
//! [`super`]. The split between `spawn_*` and `*_tick` exists so integration
//! tests can drive a single iteration synchronously without waiting on the
//! task's interval (see `test_hooks::PgStore::outbox_heartbeat_once` and
//! `outbox_sweep_once`).

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use crate::TracedPool;
use atc_core::Clock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::metrics::PgMetrics;

use super::{
    OUTBOX_HEARTBEAT_INTERVAL, OUTBOX_STALE_THRESHOLD, OUTBOX_SWEEP_INTERVAL,
    OUTBOX_SWEEP_MAX_ROWS, OUTBOX_WATERMARK_CLEANUP_WINDOW,
};

// ---------------------------------------------------------------------------
// Outbox retention — heartbeat task
// ---------------------------------------------------------------------------

/// Spawn the outbox heartbeat task.
///
/// The first iteration runs unconditionally so `outbox_watermarks` is
/// populated within milliseconds of `PgStore::start_inner` returning —
/// mirroring the drain task's first-iter pattern. Subsequent iterations wait
/// on either cancellation or `OUTBOX_HEARTBEAT_INTERVAL`. Each tick is its
/// own root span (`outbox.heartbeat.tick`) via the
/// `#[tracing::instrument(...)]` decoration on `outbox_heartbeat_tick`. A
/// task-lifetime `.instrument(span)` here would never close under normal
/// operation; see `docs/architecture/metrics.md` § "Task-lifetime root spans
/// are an anti-pattern".
pub(crate) fn spawn_outbox_heartbeat(
    clock: Arc<dyn Clock>,
    pool: TracedPool,
    replica_id: Arc<str>,
    broadcast_watermark: Arc<AtomicI64>,
    min_replica_watermark_atomic: Arc<AtomicI64>,
    oldest_row_age_seconds_atomic: Arc<AtomicI64>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut first_iter = true;
        loop {
            if !first_iter {
                tokio::select! {
                    () = cancel.cancelled() => break,
                    () = tokio::time::sleep(OUTBOX_HEARTBEAT_INTERVAL) => {}
                }
            }
            first_iter = false;

            if let Err(e) = outbox_heartbeat_tick(
                clock.as_ref(),
                &pool,
                replica_id.as_ref(),
                broadcast_watermark.as_ref(),
                min_replica_watermark_atomic.as_ref(),
                oldest_row_age_seconds_atomic.as_ref(),
            )
            .await
            {
                tracing::warn!(
                    error.message = %e,
                    replica_id = %replica_id,
                    "outbox heartbeat tick failed",
                );
            }
        }
    })
}

/// Single iteration of the outbox heartbeat: UPSERT this replica's row,
/// refresh `min_replica_watermark` and `oldest_row_age_seconds` atomics.
///
/// All timestamp bindings are Rust-side from `Clock::now()` (no SQL `now()`)
/// per ADR 0007 § Clock discipline — production uses `SystemClock`; tests
/// use `TestClock` and observe deterministic behaviour.
#[tracing::instrument(
    name = "outbox.heartbeat.tick",
    skip_all,
    fields(
        replica_id = %replica_id,
        broadcast_watermark = tracing::field::Empty,
        min_replica_watermark = tracing::field::Empty,
        oldest_row_age_seconds = tracing::field::Empty,
    ),
)]
pub(crate) async fn outbox_heartbeat_tick(
    clock: &dyn Clock,
    pool: &TracedPool,
    replica_id: &str,
    broadcast_watermark: &AtomicI64,
    min_replica_watermark_atomic: &AtomicI64,
    oldest_row_age_seconds_atomic: &AtomicI64,
) -> Result<(), sqlx::Error> {
    let now = clock.now();
    let wm = broadcast_watermark.load(Ordering::Acquire);

    // (1) UPSERT this replica's heartbeat row. `updated_at` is Rust-bound so
    // `TestClock`-driven tests are deterministic; SQL `now()` would route
    // through the DB's wall-clock instead.
    //
    // `GREATEST(...)` on both columns enforces per-replica monotonicity. In
    // production this is a no-op — `broadcast_watermark` only advances and
    // `updated_at` is bound from a monotonic clock per task. But two
    // heartbeats can race against each other (the spawn-time unconditional
    // first tick vs. a test's synchronous `outbox_heartbeat_once`, or a
    // future opt-in heartbeat trigger): without `GREATEST`, the later
    // commit's potentially-stale `wm` could overwrite the newer one, since
    // the SQL value was bound from `broadcast_watermark.load(Acquire)`
    // earlier and the in-process atomic may have advanced since. Monotonic
    // semantics in the UPSERT make this race a no-op.
    sqlx::query!(
        r#"
        INSERT INTO outbox_watermarks (replica_id, broadcast_watermark, updated_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (replica_id) DO UPDATE SET
            broadcast_watermark = GREATEST(EXCLUDED.broadcast_watermark, outbox_watermarks.broadcast_watermark),
            updated_at = GREATEST(EXCLUDED.updated_at, outbox_watermarks.updated_at)
        "#,
        replica_id,
        wm,
        now,
    )
    .execute(pool)
    .await?;

    // (2) Refresh min-replica-watermark gauge atomic. Stale cutoff is
    // Rust-bound. `MIN(...)` returns NULL when no fresh rows match → store -1
    // (NaN sentinel) so dashboards distinguish "no live replicas" from
    // "min watermark is 0".
    let stale_cutoff = now
        - chrono::Duration::from_std(OUTBOX_STALE_THRESHOLD)
            .expect("OUTBOX_STALE_THRESHOLD fits chrono::Duration");
    let min_wm: Option<i64> = sqlx::query_scalar!(
        r#"SELECT MIN(broadcast_watermark) FROM outbox_watermarks WHERE updated_at > $1"#,
        stale_cutoff,
    )
    .fetch_one(pool)
    .await?;
    let min_wm_atomic_value = min_wm.unwrap_or(-1);
    min_replica_watermark_atomic.store(min_wm_atomic_value, Ordering::Release);

    // (3) Refresh oldest-row-age gauge atomic. NULL (empty outbox) → -1.
    let oldest: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar!(r#"SELECT MIN(inserted_at) FROM outbox"#)
            .fetch_one(pool)
            .await?;
    let age_seconds = oldest.map_or(-1, |t| (now - t).num_seconds().max(0));
    oldest_row_age_seconds_atomic.store(age_seconds, Ordering::Release);

    // Record span fields for trace observers.
    let span = tracing::Span::current();
    span.record("broadcast_watermark", wm);
    span.record("min_replica_watermark", min_wm_atomic_value);
    span.record("oldest_row_age_seconds", age_seconds);

    Ok(())
}

// ---------------------------------------------------------------------------
// Outbox retention — sweep task
// ---------------------------------------------------------------------------

/// Spawn the outbox sweep task.
///
/// Each tick deletes outbox rows older than `outbox_retention` AND with
/// `seq <= MIN(broadcast_watermark)` across non-stale replicas, using an
/// ordered candidate-selection CTE with `FOR UPDATE SKIP LOCKED`. The
/// `SKIP LOCKED` semantics let multiple replicas sweep concurrently without
/// deadlocking or duplicating work — each sweeper acquires a disjoint
/// candidate subset.
///
/// No first-iter unconditional run: starts quiet for the first
/// `OUTBOX_SWEEP_INTERVAL` so operators observe the new replica via
/// dashboards before any destructive work fires.
pub(crate) fn spawn_outbox_sweep(
    clock: Arc<dyn Clock>,
    pool: TracedPool,
    outbox_retention: Duration,
    metrics: Arc<PgMetrics>,
    cancel: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = tokio::time::sleep(OUTBOX_SWEEP_INTERVAL) => {}
            }

            if let Err(e) =
                outbox_sweep_tick(clock.as_ref(), &pool, outbox_retention, metrics.as_ref()).await
            {
                tracing::warn!(error.message = %e, "outbox sweep tick failed");
            }
        }
    })
}

/// Single iteration of the outbox sweep. Cutoffs bound Rust-side from
/// `Clock::now()` (no SQL `now()`) per ADR 0007. Ordered-CTE + `FOR UPDATE
/// SKIP LOCKED` semantics described inline above the statement.
///
/// Piggybacks a cleanup pass over `outbox_watermarks` — rows whose
/// `updated_at` is older than `OUTBOX_WATERMARK_CLEANUP_WINDOW` are
/// considered dead replicas and removed, preventing infinite growth in the
/// rare case of unclean shutdowns repeatedly producing fresh replica ids.
#[tracing::instrument(
    name = "outbox.sweep.tick",
    skip_all,
    fields(
        retention_seconds = outbox_retention.as_secs(),
        rows_deleted = tracing::field::Empty,
        watermarks_cleaned = tracing::field::Empty,
    ),
)]
pub(crate) async fn outbox_sweep_tick(
    clock: &dyn Clock,
    pool: &TracedPool,
    outbox_retention: Duration,
    metrics: &PgMetrics,
) -> Result<u64, sqlx::Error> {
    let now = clock.now();
    let retention_cutoff = now
        - chrono::Duration::from_std(outbox_retention).expect("retention fits chrono::Duration");
    let stale_cutoff = now
        - chrono::Duration::from_std(OUTBOX_STALE_THRESHOLD)
            .expect("OUTBOX_STALE_THRESHOLD fits chrono::Duration");
    let watermark_cleanup_cutoff = now
        - chrono::Duration::from_std(OUTBOX_WATERMARK_CLEANUP_WINDOW)
            .expect("OUTBOX_WATERMARK_CLEANUP_WINDOW fits chrono::Duration");

    // (1) Sweep retired outbox rows.
    //
    // The CTE locks candidate `seq` values in monotonic order via the PK
    // index, with `SKIP LOCKED` so a second concurrent sweeper acquires a
    // disjoint set rather than waiting. The `seq IN (SELECT seq FROM
    // victims)` step performs the DELETE under those locks. `RETURNING seq`
    // gives us a directly-counted affected-row count without depending on
    // sqlx's `affected_rows` reporting (mildly surprising under `WITH ...
    // RETURNING`).
    let deleted_seqs: Vec<i64> = sqlx::query_scalar!(
        r#"
        WITH victims AS (
            SELECT seq FROM outbox
            WHERE inserted_at < $1
              AND seq <= (
                  SELECT COALESCE(MIN(broadcast_watermark), 0)
                  FROM outbox_watermarks
                  WHERE updated_at > $2
              )
            ORDER BY seq
            LIMIT $3
            FOR UPDATE SKIP LOCKED
        )
        DELETE FROM outbox WHERE seq IN (SELECT seq FROM victims)
        RETURNING seq
        "#,
        retention_cutoff,
        stale_cutoff,
        OUTBOX_SWEEP_MAX_ROWS,
    )
    .fetch_all(pool)
    .await?;

    let rows_deleted = u64::try_from(deleted_seqs.len()).unwrap_or(0);
    metrics.outbox_rows_deleted.add(rows_deleted, &[]);

    // (2) Piggyback watermark cleanup. Dead replica rows (`updated_at`
    // older than the cleanup window) are removed. `OUTBOX_WATERMARK_CLEANUP_WINDOW`
    // is 30× `OUTBOX_STALE_THRESHOLD` so live replicas can't be accidentally
    // pruned.
    let cleanup = sqlx::query!(
        r#"DELETE FROM outbox_watermarks WHERE updated_at < $1"#,
        watermark_cleanup_cutoff,
    )
    .execute(pool)
    .await?;
    let watermarks_cleaned = cleanup.rows_affected();

    let span = tracing::Span::current();
    span.record("rows_deleted", rows_deleted);
    span.record("watermarks_cleaned", watermarks_cleaned);

    Ok(rows_deleted)
}
