//! PostgreSQL-backed [`PersistentStore`](atc_persist::PersistentStore)
//! implementation — type, constructors, helpers, and module wiring.
//!
//! The `impl PersistentStore for PgStore` block plus the free-function
//! transaction helpers (`upsert_run_in_txn`, `upsert_job_in_txn`,
//! `insert_outbox_*_in_txn`, `notify_outbox_seq_in_txn`) live in
//! [`writes`](crate::store::writes). The four retention free functions
//! (`spawn_outbox_heartbeat`, `outbox_heartbeat_tick`, `spawn_outbox_sweep`,
//! `outbox_sweep_tick`) live in [`retention`](crate::store::retention). The
//! `test-support`-feature surface (`PgStoreTestHooks`, `PgStoreTestHandles`,
//! `PgStore::start_with_test_hooks`, the test-only `impl PgStore { … }` block
//! of sync-tick + accessor methods) lives in
//! [`test_hooks`](crate::store::test_hooks).

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::TracedPool;
use atc_core::{
    Clock, JobConclusion, JobStatus, RunConclusion, RunStatus,
    event::{JobEvent, RunEvent},
};
use atc_wire::CommittedEvent;
use sqlx::postgres::PgListener;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::listener;
use crate::metrics::PgMetrics;

pub mod retention;
pub mod staleness;
pub mod writes;

#[cfg(any(test, feature = "test-support"))]
pub mod test_hooks;

#[cfg(any(test, feature = "test-support"))]
pub use test_hooks::{PgStoreTestHandles, PgStoreTestHooks};

/// Production broadcast capacity for the PG-mode event stream.
pub(crate) const BROADCAST_CAPACITY: usize = 256;

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
pub(crate) const OUTBOX_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// Replicas whose `updated_at` is older than this cutoff are excluded from
/// the `MIN(broadcast_watermark)` floor used by the sweep statement. 3×
/// heartbeat tolerates one missed tick before a replica drops out.
pub(crate) const OUTBOX_STALE_THRESHOLD: Duration = Duration::from_secs(90);

/// Cadence the sweep task uses to delete retired outbox rows. Every 5 minutes
/// is sufficient for a 7-day retention default.
pub(crate) const OUTBOX_SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// Per-tick maximum on rows deleted by the sweep statement. Bounds work under
/// traffic spikes; total bounded by `OUTBOX_SWEEP_MAX_ROWS × replicas`.
pub(crate) const OUTBOX_SWEEP_MAX_ROWS: i64 = 10_000;

/// Watermark rows whose `updated_at` is older than this cutoff are removed by
/// the sweep task's piggyback cleanup. 30× stale_threshold leaves a wide
/// margin against false-positive cleanup of live replicas.
pub(crate) const OUTBOX_WATERMARK_CLEANUP_WINDOW: Duration = Duration::from_secs(3600);

/// Per-tick maximum candidate rows the staleness sweep considers, per entity
/// (jobs and runs are capped independently). Bounds per-tick work under a
/// large stale backlog; leftovers wait for the next tick.
pub(crate) const STALENESS_SWEEP_BATCH_CAP: i64 = 500;

// ---------------------------------------------------------------------------
// Per-task shutdown timeouts (moved from atc-server::shutdown)
// ---------------------------------------------------------------------------

/// Join budget for the drain task during [`PgStore::shutdown`]. Drain owns the
/// broadcast write end so this is the largest of the four PG task budgets.
pub const SHUTDOWN_TIMEOUT_DRAIN: Duration = Duration::from_secs(5);
/// Join budget for the listener task during [`PgStore::shutdown`].
pub const SHUTDOWN_TIMEOUT_LISTENER: Duration = Duration::from_secs(1);
/// Join budget for the outbox heartbeat task during [`PgStore::shutdown`].
/// Each tick is bounded by `OUTBOX_HEARTBEAT_INTERVAL`; 2 s is generous for
/// a cooperative exit at the next `select!` boundary.
pub const SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT: Duration = Duration::from_secs(2);
/// Join budget for the outbox sweep task during [`PgStore::shutdown`]. Same
/// cooperative shape as the heartbeat. A sweep can run a multi-second
/// statement under contention, so 2 s is the join budget for cooperative
/// exit, not for the in-flight statement. Also covers the staleness sweep,
/// which now runs inside this same task (see `spawn_outbox_sweep`).
pub const SHUTDOWN_TIMEOUT_OUTBOX_SWEEP: Duration = Duration::from_secs(2);

// ---------------------------------------------------------------------------
// PgStoreStartError
// ---------------------------------------------------------------------------

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
pub(crate) trait SqlRepr {
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

pub(crate) fn derive_run_target(action: &RunEvent) -> RunStatus {
    match action {
        RunEvent::Requested => RunStatus::Queued,
        RunEvent::InProgress => RunStatus::InProgress,
        RunEvent::Completed { .. } => RunStatus::Completed,
    }
}

pub(crate) fn derive_job_target(action: &JobEvent) -> JobStatus {
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

/// PostgreSQL-backed implementation of [`PersistentStore`](atc_persist::PersistentStore).
///
/// Owns the connection pool, the broadcast sender that fans events out to WS
/// subscribers, the drain task's watermark + heartbeat atomics, and the
/// JoinHandles for the listener and drain tasks themselves. Constructed via
/// [`PgStore::start`] (production) or [`PgStore::start_with_test_hooks`]
/// (tests; behind the `test-support` feature).
pub struct PgStore {
    pub(crate) pool: TracedPool,
    /// Wall-clock source for the drain heartbeat, outbox-lag observation, and
    /// retention path. Owning this here (rather than reading `Utc::now()`
    /// directly at each call site) lets tests advance time deterministically
    /// via `TestClock`. See `atc-core/src/clock.rs` for the wall-vs-monotonic
    /// split.
    pub(crate) clock: Arc<dyn Clock>,
    /// Highest outbox `seq` the drain has fetched and broadcast.
    /// Used as `lastSeq` in `read_snapshot`. Memory ordering: drain writes
    /// via `Release`; we read via `Acquire` before opening the REPEATABLE READ tx.
    pub(crate) broadcast_watermark: Arc<AtomicI64>,
    /// Drain-task heartbeat (epoch milliseconds). Refreshed by drain each loop
    /// iteration. Used by `readyz` to detect stalled drain tasks.
    pub(crate) last_drain_pass_at: Arc<AtomicI64>,
    /// Broadcast sender for fanning `CommittedEvent`s out to WS subscribers.
    /// Cloned into the drain task at spawn time so the drain is the sole
    /// writer in PG mode.
    pub(crate) broadcast_tx: broadcast::Sender<CommittedEvent>,
    /// Cached metric handles for every `atc_pg_*` emit site. Constructed once
    /// per store in `start_inner` (which requires the global recorder to be
    /// installed first — see [`PgMetrics::register`]) and cloned into the
    /// listener and drain task closures.
    pub(crate) metrics: Arc<PgMetrics>,
    /// JoinHandles for the spawned listener, drain, heartbeat, and sweep
    /// tasks. Consumed by the first `shutdown()` call; subsequent calls
    /// observe `None` and return immediately.
    pub(crate) handles: StdMutex<Option<PgStoreHandles>>,
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
    /// Configured staleness sweep threshold. `None` disables the sweep (no
    /// task spawned). Production code reads this only at spawn time; the
    /// `#[cfg(any(test, feature = "test-support"))] staleness_sweep_once()`
    /// entry point reads it back off `self`.
    #[cfg_attr(not(any(test, feature = "test-support")), allow(dead_code))]
    pub(crate) staleness_threshold: Option<Duration>,
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

pub(crate) struct PgStoreHandles {
    pub(crate) listener: JoinHandle<()>,
    pub(crate) drain: JoinHandle<()>,
    pub(crate) heartbeat: JoinHandle<()>,
    /// Also drives the staleness sweep on the same tick — see
    /// `retention::spawn_outbox_sweep`.
    pub(crate) sweep: Option<JoinHandle<()>>,
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
        pool: TracedPool,
        listener_conn: PgListener,
        shutdown: CancellationToken,
        outbox_retention: Duration,
        staleness_threshold: Option<Duration>,
    ) -> Result<Arc<Self>, PgStoreStartError> {
        Self::start_inner(
            clock,
            pool,
            listener_conn,
            shutdown,
            outbox_retention,
            staleness_threshold,
            None,
            None,
            None,
            None,
        )
        .await
    }

    /// Shared core of `start` and `start_with_test_hooks`. The four trailing
    /// `Option<_>` parameters are always `None` in production and used by the
    /// test-hooks variant to inject counters / notifies / per-pass delays.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start_inner(
        clock: Arc<dyn Clock>,
        pool: TracedPool,
        listener_conn: PgListener,
        shutdown: CancellationToken,
        outbox_retention: Duration,
        staleness_threshold: Option<Duration>,
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
        let (broadcast_tx, _sentinel) = broadcast::channel::<CommittedEvent>(BROADCAST_CAPACITY);

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
        let heartbeat_handle = retention::spawn_outbox_heartbeat(
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
        // destructive work begins. Also drives the staleness sweep
        // (`staleness_threshold: None` skips just that pass each tick — see
        // `spawn_outbox_sweep`'s doc comment).
        let sweep_handle = retention::spawn_outbox_sweep(
            Arc::clone(&clock),
            pool.clone(),
            outbox_retention,
            staleness_threshold,
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
            staleness_threshold,
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

// ---------------------------------------------------------------------------
// Hostname helper used by start_inner to seed replica_id
// ---------------------------------------------------------------------------

/// Best-effort hostname read for the `replica_id` prefix. Falls back to
/// `"unknown"` if `HOSTNAME` is unset and `/etc/hostname` cannot be read.
/// Lossy but observable: operators see the replica's actual hostname when
/// available, and a stable `"unknown-<uuid>"` otherwise.
pub(crate) fn hostname_or_unknown() -> String {
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
