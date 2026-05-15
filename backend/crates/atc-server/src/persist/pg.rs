//! PostgreSQL-backed persistence implementation.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use atc_core::{
    Clock, JobConclusion, JobStatus, PersistError, RunConclusion, RunStatus,
    event::{JobEvent, JobEventEnvelope, RunEvent, RunEventEnvelope},
};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::{Notify, broadcast};
#[cfg(any(test, feature = "test-support"))]
use tokio::task::AbortHandle;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::listener;
use crate::metrics::PgMetrics;
use crate::shutdown::{
    SHUTDOWN_TIMEOUT_DRAIN, SHUTDOWN_TIMEOUT_LISTENER, SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT,
    SHUTDOWN_TIMEOUT_OUTBOX_SWEEP, join_with_timeout,
};
use crate::state::{SeqEvent, StateSnapshot};

use super::{LivenessError, PersistentStore, reads};

/// Production broadcast capacity for the PG-mode event stream.
const BROADCAST_CAPACITY: usize = 256;

/// Minimum supported outbox retention. Values below this floor are rejected
/// by [`PgStore::start_inner`] with [`PgStoreStartError::RetentionTooShort`].
///
/// The floor exists because `inserted_at` on outbox rows is transaction-start
/// wall-clock (Postgres `now()` defaults to `transaction_timestamp()`), not
/// commit time. A retention shorter than the longest practical writer
/// transaction would let a row commit and immediately satisfy the
/// `inserted_at < now() - retention` predicate before any replica has drained
/// it. 1 hour comfortably dominates any practical writer transaction in this
/// codebase (webhook handlers commit within milliseconds). See ADR 0007.
pub const OUTBOX_RETENTION_FLOOR: Duration = Duration::from_secs(3600);

/// Cadence the heartbeat task uses to upsert this replica's row in
/// `outbox_watermarks` and refresh the retention gauge atomics.
const OUTBOX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Replicas whose `updated_at` is older than this cutoff are excluded from
/// the `MIN(broadcast_watermark)` floor used by the sweep statement. 3×
/// heartbeat tolerates one missed tick before a replica drops out.
const OUTBOX_STALE_THRESHOLD: Duration = Duration::from_secs(90);

/// Cadence the sweep task uses to delete retired outbox rows. Every 5 minutes
/// is sufficient for a 7-day retention default.
const OUTBOX_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// Per-tick maximum on rows deleted by the sweep statement. Bounds work under
/// traffic spikes; total bounded by `OUTBOX_SWEEP_MAX_ROWS × replicas`.
const OUTBOX_SWEEP_MAX_ROWS: i64 = 10_000;

/// Watermark rows whose `updated_at` is older than this cutoff are removed by
/// the sweep task's piggyback cleanup. 30× stale_threshold leaves a wide
/// margin against false-positive cleanup of live replicas.
const OUTBOX_WATERMARK_CLEANUP_WINDOW: Duration = Duration::from_secs(3600);

/// Errors returned by [`PgStore::start`].
///
/// The startup path runs `SELECT MAX(seq) FROM outbox` to seed the
/// broadcast watermark before spawning the listener and drain tasks. A
/// pool-side failure surfaces here instead of inside a spawned task where
/// `main.rs` could not observe it. The retention floor check also runs
/// during startup so a misconfigured `ATC_OUTBOX_RETENTION` fails the
/// process at startup instead of silently allowing unsafe deletes.
#[derive(Debug)]
pub enum PgStoreStartError {
    /// The seed query against `outbox` failed.
    Watermark(sqlx::Error),
    /// `outbox_retention` was below [`OUTBOX_RETENTION_FLOOR`]. Rejected per
    /// ADR 0007: shorter values can't be made safe under MVCC visibility of
    /// long-held writer transactions.
    RetentionTooShort {
        configured: Duration,
        minimum: Duration,
    },
}

impl std::fmt::Display for PgStoreStartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Watermark(e) => {
                write!(
                    f,
                    "failed to query outbox watermark during PgStore startup: {e}"
                )
            }
            Self::RetentionTooShort {
                configured,
                minimum,
            } => {
                write!(
                    f,
                    "outbox retention {configured:?} is below the supported floor {minimum:?}; \
                     shorter retention is unsafe because Postgres `inserted_at` is \
                     transaction-start time, not commit time. See ADR 0007."
                )
            }
        }
    }
}

impl std::error::Error for PgStoreStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Watermark(e) => Some(e),
            Self::RetentionTooShort { .. } => None,
        }
    }
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
/// Owns the connection pool, the broadcast sender that fans events out to WS
/// subscribers, the drain task's watermark + heartbeat atomics, and the
/// JoinHandles for the listener and drain tasks themselves. Constructed via
/// [`PgStore::start`] (production) or [`PgStore::start_with_test_hooks`]
/// (tests).
pub struct PgStore {
    pub(super) pool: PgPool,
    /// Wall-clock source for the drain heartbeat, outbox-lag observation, and
    /// retention path. Owning this here (rather than reading `Utc::now()`
    /// directly at each call site) lets tests advance time deterministically
    /// via `TestClock`. See `atc-core/src/clock.rs` for the wall-vs-monotonic
    /// split.
    clock: Arc<dyn Clock>,
    /// Highest outbox `seq` the drain has fetched and broadcast.
    /// Used as `lastSeq` in `read_snapshot`. Memory ordering: drain writes
    /// via `Release`; we read via `Acquire` before opening the REPEATABLE READ tx.
    pub(crate) broadcast_watermark: Arc<AtomicI64>,
    /// Drain-task heartbeat (epoch milliseconds). Refreshed by drain each loop
    /// iteration. Used by `readyz` to detect stalled drain tasks.
    pub(crate) last_drain_pass_at: Arc<AtomicI64>,
    /// Broadcast sender for fanning `SeqEvent`s out to WS subscribers. Cloned
    /// into the drain task at spawn time so the drain is the sole writer in
    /// PG mode.
    broadcast_tx: broadcast::Sender<SeqEvent>,
    /// Cached metric handles for every `atc_pg_*` emit site. Constructed once
    /// per store in `start_inner` (which requires the global recorder to be
    /// installed first — see [`PgMetrics::register`]) and cloned into the
    /// listener and drain task closures.
    metrics: Arc<PgMetrics>,
    /// JoinHandles for the spawned listener, drain, heartbeat, and sweep
    /// tasks. Consumed by the first `shutdown()` call; subsequent calls
    /// observe `None` and return immediately.
    handles: StdMutex<Option<PgStoreHandles>>,
    /// Per-process replica identity, written into `outbox_watermarks` so the
    /// sweep statement can compute a multi-replica safety floor on the
    /// outbox seq range. Format: `hostname-<8-hex-uuid>`. UUID suffix
    /// prevents collision when two `PgStore` instances run against the same
    /// database with the same hostname (multi-instance dev / integration
    /// tests). Production code reads this only via the heartbeat task's
    /// spawn-time `Arc::clone`; the cfg-gated `replica_id()` accessor reads
    /// it back off `self` for test inspection.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub(crate) replica_id: Arc<str>,
    /// Configured retention age for the outbox. Validated `>= OUTBOX_RETENTION_FLOOR`
    /// in `start_inner`. Production code reads this only at spawn time (the
    /// sweep task captures it by value); the `#[cfg(any(test, feature =
    /// "test-support"))] outbox_sweep_once()` entry point reads it back off
    /// `self`, so production builds correctly flag the field as never
    /// re-read after construction.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub(crate) outbox_retention: Duration,
    /// Atomic mirror of `MIN(broadcast_watermark)` across non-stale replicas.
    /// Refreshed by the heartbeat task on every tick; read by the
    /// `atc_pg_outbox_min_replica_watermark` observable gauge callback via a
    /// `Weak<AtomicI64>` registered with the meter. `-1` is the NaN sentinel
    /// — rendered as NaN by the gauge so dashboards distinguish "no live
    /// replicas seen" from "min watermark is 0". Production keeps the
    /// strong reference alive through the heartbeat task's spawn-time
    /// Arc::clone; this field is read directly by the cfg-gated test
    /// accessors.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub(crate) min_replica_watermark_atomic: Arc<AtomicI64>,
    /// Atomic mirror of `clock.now() - MIN(inserted_at) FROM outbox`, in
    /// seconds. Refreshed by the heartbeat task on every tick; read by the
    /// `atc_pg_outbox_oldest_row_age_seconds` observable gauge callback via
    /// a `Weak<AtomicI64>`. `-1` is the NaN sentinel — rendered as NaN by
    /// the gauge when the outbox is empty.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub(crate) oldest_row_age_seconds_atomic: Arc<AtomicI64>,
}

struct PgStoreHandles {
    listener: JoinHandle<()>,
    drain: JoinHandle<()>,
    heartbeat: JoinHandle<()>,
    sweep: Option<JoinHandle<()>>,
}

/// Test hooks for [`PgStore::start_with_test_hooks`]. Mirrors the optional
/// instrumentation params on the existing `spawn_listener_task` /
/// `spawn_drain_task` signatures so existing fixtures can keep observing
/// listener / drain progress.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct PgStoreTestHooks {
    pub received_counter: Option<Arc<AtomicU64>>,
    pub observed_passes: Option<Arc<AtomicU64>>,
    pub drain_started: Option<Arc<Notify>>,
    pub drain_delay: Option<Duration>,
}

/// Handles returned by [`PgStore::start_with_test_hooks`] for the test fixture.
///
/// Carries abort handles (extracted via `.abort_handle()` before the
/// `JoinHandle`s were stored on the store) plus the watermark / heartbeat
/// atomics that integration tests poll. No cfg-gated accessor methods on the
/// store itself — tests get everything they need at construction time.
#[cfg(any(test, feature = "test-support"))]
pub struct PgStoreTestHandles {
    pub drain_abort: AbortHandle,
    pub listener_abort: AbortHandle,
    pub last_drain_pass_at: Arc<AtomicI64>,
    pub broadcast_watermark: Arc<AtomicI64>,
}

impl PgStore {
    /// Construct a [`PgStore`] and spawn its listener and drain tasks.
    ///
    /// Seeds the broadcast watermark from `MAX(outbox.seq)` before spawning,
    /// so `/v1/state` returns a sensible `lastSeq` even before the first
    /// post-startup drain pass completes. The seed query is the last fallible
    /// operation in this function: after the tasks are spawned, this function
    /// returns `Ok` unconditionally — any future contributor adding a fallible
    /// step after the spawn calls must cancel and join the already-spawned
    /// tasks before returning `Err`.
    pub async fn start(
        clock: Arc<dyn Clock>,
        pool: PgPool,
        listener_conn: PgListener,
        shutdown: CancellationToken,
        outbox_retention: Duration,
    ) -> Result<Arc<Self>, PgStoreStartError> {
        Self::start_inner(
            clock,
            pool,
            listener_conn,
            shutdown,
            outbox_retention,
            None,
            None,
            None,
            None,
        )
        .await
    }

    /// Test variant that mirrors [`PgStore::start`] but accepts optional
    /// instrumentation hooks (`received_counter`, `observed_passes`,
    /// `drain_started`, `drain_delay`) and returns a [`PgStoreTestHandles`]
    /// alongside the store so test fixtures can poll the watermark / abort
    /// the drain mid-pass.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn start_with_test_hooks(
        clock: Arc<dyn Clock>,
        pool: PgPool,
        listener_conn: PgListener,
        shutdown: CancellationToken,
        outbox_retention: Duration,
        hooks: PgStoreTestHooks,
    ) -> Result<(Arc<Self>, PgStoreTestHandles), PgStoreStartError> {
        let store = Self::start_inner(
            clock,
            pool,
            listener_conn,
            shutdown,
            outbox_retention,
            hooks.received_counter,
            hooks.observed_passes,
            hooks.drain_started,
            hooks.drain_delay,
        )
        .await?;
        let (drain_abort, listener_abort) = {
            let guard = store.handles.lock().expect("handles mutex poisoned");
            let inner = guard.as_ref().expect("handles populated by start_inner");
            (inner.drain.abort_handle(), inner.listener.abort_handle())
        };
        let handles = PgStoreTestHandles {
            drain_abort,
            listener_abort,
            last_drain_pass_at: Arc::clone(&store.last_drain_pass_at),
            broadcast_watermark: Arc::clone(&store.broadcast_watermark),
        };
        Ok((store, handles))
    }

    /// Shared core of `start` and `start_with_test_hooks`. The four trailing
    /// `Option<_>` parameters are always `None` in production and used by the
    /// test-hooks variant to inject counters / notifies / per-pass delays.
    #[allow(clippy::too_many_arguments)]
    async fn start_inner(
        clock: Arc<dyn Clock>,
        pool: PgPool,
        listener_conn: PgListener,
        shutdown: CancellationToken,
        outbox_retention: Duration,
        received_counter: Option<Arc<AtomicU64>>,
        observed_passes: Option<Arc<AtomicU64>>,
        drain_started: Option<Arc<Notify>>,
        drain_delay: Option<Duration>,
    ) -> Result<Arc<Self>, PgStoreStartError> {
        // Reject sub-floor retention before any side effect. Per ADR 0007:
        // shorter retention is unsafe because `inserted_at` is
        // transaction-start time, not commit time, so a long-held writer
        // transaction could commit a row past the retention cutoff before
        // any replica has drained it.
        if outbox_retention < OUTBOX_RETENTION_FLOOR {
            return Err(PgStoreStartError::RetentionTooShort {
                configured: outbox_retention,
                minimum: OUTBOX_RETENTION_FLOOR,
            });
        }

        // Construct broadcast channel (sentinel receiver dropped immediately —
        // production subscribers come from `subscribe()`).
        let (broadcast_tx, _sentinel) = broadcast::channel::<SeqEvent>(BROADCAST_CAPACITY);

        // Arcs the listener and drain need to coordinate. Created before
        // `PgMetrics::register` so the observable gauges' callbacks close
        // over the actual atomics.
        let broadcast_watermark = Arc::new(AtomicI64::new(0));
        let last_drain_pass_at = Arc::new(AtomicI64::new(clock.now().timestamp_millis()));
        let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));
        let drain_in_flight = Arc::new(AtomicBool::new(false));
        let drain_notify = Arc::new(Notify::new());

        // Retention gauge atomics. Initialised to -1 (the NaN sentinel) so
        // the period between store construction and the first heartbeat tick
        // renders as NaN on dashboards instead of a misleading 0. The
        // heartbeat task populates real values on its first iteration (which
        // runs unconditionally, mirroring the drain task's first-iter
        // pattern).
        let min_replica_watermark_atomic = Arc::new(AtomicI64::new(-1));
        let oldest_row_age_seconds_atomic = Arc::new(AtomicI64::new(-1));

        // Register cached metric handles. Must happen before any emit. The
        // OTel global meter precondition is upheld by callers: production
        // `main.rs` runs `otel::init_otel` before `PgStore::start`, and the
        // integration harness installs an in-memory meter provider once per
        // binary via the `OnceLock` in `tests/integration/common/mod.rs`
        // before any test constructs a `PgStore`.
        // PgMetrics' observable gauges take Weak references so callbacks from
        // prior PgStore instances in the same process (integration tests)
        // become no-ops once their tasks drop their strong references — see
        // `PgMetrics::register_with_meter`.
        let pg_metrics = PgMetrics::register(
            &broadcast_watermark,
            &min_pending_seq,
            &min_replica_watermark_atomic,
            &oldest_row_age_seconds_atomic,
        );

        // Capture startup_at BEFORE the seed query so the drain startup
        // histogram includes the cold-pool query cost.
        let startup_at = Instant::now();

        // Seed the watermark. The observable `atc_pg_broadcast_watermark`
        // gauge reads this atomic on every collection cycle, so no separate
        // gauge `record` is needed.
        let initial_watermark: i64 =
            sqlx::query_scalar!("SELECT COALESCE(MAX(seq), 0) AS \"max!: i64\" FROM outbox")
                .fetch_one(&pool)
                .await
                .map_err(PgStoreStartError::Watermark)?;
        broadcast_watermark.store(initial_watermark, Ordering::Release);

        // Generate the per-process replica identity. UUID suffix prevents
        // collision when two `PgStore` instances run against the same
        // database with the same hostname (multi-instance dev / integration
        // tests).
        let replica_id: Arc<str> = {
            let host = hostname_or_unknown();
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            Arc::from(format!("{host}-{}", &suffix[..8]))
        };

        // Spawn listener.
        let listener_handle = listener::spawn_listener_task(
            listener_conn,
            Arc::clone(&drain_notify),
            Arc::clone(&min_pending_seq),
            Arc::clone(&drain_in_flight),
            shutdown.clone(),
            received_counter,
            Arc::clone(&pg_metrics),
        );

        // Spawn drain.
        let drain_handle = listener::spawn_drain_task(
            Arc::clone(&clock),
            pool.clone(),
            initial_watermark,
            startup_at,
            drain_notify,
            min_pending_seq,
            Arc::clone(&last_drain_pass_at),
            Arc::clone(&broadcast_watermark),
            drain_in_flight,
            broadcast_tx.clone(),
            shutdown.clone(),
            observed_passes,
            drain_started,
            drain_delay,
            Arc::clone(&pg_metrics),
        );

        // Spawn outbox heartbeat. Mirrors the drain task's first-iter
        // unconditional run so the first tick fires immediately at startup —
        // operators (and tests) observe `outbox_watermarks` populated within
        // milliseconds rather than waiting up to `OUTBOX_HEARTBEAT_INTERVAL`.
        let heartbeat_handle = spawn_outbox_heartbeat(
            Arc::clone(&clock),
            pool.clone(),
            Arc::clone(&replica_id),
            Arc::clone(&broadcast_watermark),
            Arc::clone(&min_replica_watermark_atomic),
            Arc::clone(&oldest_row_age_seconds_atomic),
            shutdown.clone(),
        );

        // Spawn outbox sweep. Unlike heartbeat, sweep does NOT run an
        // unconditional first iteration — there's no urgency to sweep at
        // startup, and a quiet first-`OUTBOX_SWEEP_INTERVAL` warm-up gives
        // operators time to observe the new replica via dashboards before
        // destructive work begins.
        let sweep_handle = spawn_outbox_sweep(
            Arc::clone(&clock),
            pool.clone(),
            outbox_retention,
            Arc::clone(&pg_metrics),
            shutdown,
        );

        // After this point, no fallible operations remain. A future
        // contributor adding a fallible step here MUST cancel and join the
        // already-spawned listener+drain+heartbeat+sweep tasks before
        // returning `Err`.

        let store = Arc::new(Self {
            pool,
            clock,
            broadcast_watermark,
            last_drain_pass_at,
            broadcast_tx,
            metrics: pg_metrics,
            handles: StdMutex::new(Some(PgStoreHandles {
                listener: listener_handle,
                drain: drain_handle,
                heartbeat: heartbeat_handle,
                sweep: Some(sweep_handle),
            })),
            replica_id,
            outbox_retention,
            min_replica_watermark_atomic,
            oldest_row_age_seconds_atomic,
        });

        Ok(store)
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

    fn subscribe(&self) -> broadcast::Receiver<SeqEvent> {
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
            return Err(LivenessError::DbUnreachable(e));
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
#[tracing::instrument(skip_all, fields(run_id = env.run_id.0, job_id = env.job_id.0))]
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
// Outbox retention — heartbeat task
// ---------------------------------------------------------------------------

/// Best-effort hostname read for the `replica_id` prefix. Falls back to
/// `"unknown"` if `HOSTNAME` is unset and `/etc/hostname` cannot be read.
/// Lossy but observable: operators see the replica's actual hostname when
/// available, and a stable `"unknown-<uuid>"` otherwise.
fn hostname_or_unknown() -> String {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.is_empty()
    {
        return h;
    }
    if let Ok(h) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = h.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    "unknown".to_string()
}

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
fn spawn_outbox_heartbeat(
    clock: Arc<dyn Clock>,
    pool: PgPool,
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
                    error = %e,
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
async fn outbox_heartbeat_tick(
    clock: &dyn Clock,
    pool: &PgPool,
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
    sqlx::query!(
        r#"
        INSERT INTO outbox_watermarks (replica_id, broadcast_watermark, updated_at)
        VALUES ($1, $2, $3)
        ON CONFLICT (replica_id) DO UPDATE SET
            broadcast_watermark = EXCLUDED.broadcast_watermark,
            updated_at = EXCLUDED.updated_at
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
fn spawn_outbox_sweep(
    clock: Arc<dyn Clock>,
    pool: PgPool,
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
                tracing::warn!(error = %e, "outbox sweep tick failed");
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
async fn outbox_sweep_tick(
    clock: &dyn Clock,
    pool: &PgPool,
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

impl PgStore {
    /// Run one iteration of the outbox heartbeat synchronously. Exposed so
    /// integration tests can drive the heartbeat path deterministically
    /// without waiting for the task's tick interval. The spawned task uses
    /// the same per-tick body via the free function above; production code
    /// does not call this method.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn outbox_heartbeat_once(&self) -> Result<(), sqlx::Error> {
        outbox_heartbeat_tick(
            self.clock.as_ref(),
            &self.pool,
            self.replica_id.as_ref(),
            self.broadcast_watermark.as_ref(),
            self.min_replica_watermark_atomic.as_ref(),
            self.oldest_row_age_seconds_atomic.as_ref(),
        )
        .await
    }

    /// Per-process replica id (heartbeat row's primary key). Exposed for
    /// integration-test inspection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn replica_id(&self) -> &str {
        self.replica_id.as_ref()
    }

    /// Local broadcast watermark atomic. Exposed for integration-test
    /// inspection; tests use `Release`/`Acquire` to seed and verify the
    /// drain-task contract.
    #[cfg(any(test, feature = "test-support"))]
    pub fn broadcast_watermark(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.broadcast_watermark)
    }

    /// Atomic mirror of the cluster-wide min broadcast watermark. Exposed for
    /// integration-test inspection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn min_replica_watermark_atomic(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.min_replica_watermark_atomic)
    }

    /// Atomic mirror of the oldest outbox row age. Exposed for
    /// integration-test inspection.
    #[cfg(any(test, feature = "test-support"))]
    pub fn oldest_row_age_seconds_atomic(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.oldest_row_age_seconds_atomic)
    }

    /// Run one iteration of the outbox sweep synchronously. Exposed so
    /// integration tests can drive the sweep path deterministically without
    /// waiting for the task's `OUTBOX_SWEEP_INTERVAL` tick. Returns the
    /// number of outbox rows deleted in this tick.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn outbox_sweep_once(&self) -> Result<u64, sqlx::Error> {
        outbox_sweep_tick(
            self.clock.as_ref(),
            &self.pool,
            self.outbox_retention,
            self.metrics.as_ref(),
        )
        .await
    }
}

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
}
