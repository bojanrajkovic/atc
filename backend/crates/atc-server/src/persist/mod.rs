//! Persistence abstraction and backends for domain events.
//!
//! Defines [`PersistentStore`], a common interface over any backend that can
//! durably apply domain events. Two implementations live here:
//!
//! - [`PgStore`] — PostgreSQL-backed. Opens its own transaction per event:
//!   UPSERT + outbox INSERT + `pg_notify` → commit → returns allocated seq.
//! - [`InMemoryStore`] — In-memory-only (dev/test). Owns `StateData` with
//!   HashMaps + secondary indexes, seq counter, clock, and broadcast sender.
//!   Uses pure `atc_core::apply_*_event` functions for transitions.
//!
//! Each store owns the background tasks that read its internal state and emit
//! domain events. `PgStore::start` constructs and spawns the listener and
//! drain tasks; `InMemoryStore::start` constructs and spawns the eviction
//! task. The trait surfaces this as `subscribe()` (event stream) and
//! `shutdown()` (cooperative join of all owned tasks).
//!
//! The `reads` submodule provides `read_all_runs` / `read_all_jobs` free
//! functions used by the PG read path.

pub mod eviction;
pub mod in_memory;
pub mod pg;
pub(crate) mod reads;

pub use atc_core::PersistError;
pub use in_memory::InMemoryStore;
pub use pg::PgStore;

use atc_core::event::{JobEventEnvelope, RunEventEnvelope};
use tokio::sync::broadcast;

use crate::state::{SeqEvent, StateSnapshot};

// ---------------------------------------------------------------------------
// LivenessError
// ---------------------------------------------------------------------------

/// Error returned by [`PersistentStore::liveness_check`].
#[derive(Debug)]
pub enum LivenessError {
    /// The database is unreachable.
    DbUnreachable(sqlx::Error),
    /// The drain task heartbeat is stale (age exceeds 30 s threshold).
    DrainStale {
        /// Age of the last heartbeat in milliseconds.
        age_ms: i64,
    },
}

// ---------------------------------------------------------------------------
// PersistentStore trait
// ---------------------------------------------------------------------------

/// A store that can durably apply domain events and return the allocated seq.
///
/// - [`PgStore`]: opens its own transaction per call (UPSERT + outbox + notify → commit).
/// - [`InMemoryStore`]: locks seq, applies pure functions, broadcasts, returns seq.
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

    /// Return a consistent snapshot of all state with the current seq cursor.
    ///
    /// For `InMemoryStore`: locks seq, reads state under a read lock.
    /// For `PgStore`: loads broadcast watermark (Acquire), opens REPEATABLE READ tx,
    /// reads runs/jobs, returns snapshot with watermark as `last_seq`.
    async fn read_snapshot(&self) -> Result<StateSnapshot, PersistError>;

    /// Check whether the store is live and healthy.
    ///
    /// For `InMemoryStore`: always returns `Ok(())`.
    /// For `PgStore`: runs `SELECT 1` to verify connectivity, then checks
    /// that the drain heartbeat is not stale (> 30 s).
    async fn liveness_check(&self) -> Result<(), LivenessError>;

    /// Subscribe to the store's domain-event broadcast stream.
    ///
    /// Each subscriber receives every event the store has emitted since the
    /// subscribe call, up to the channel's bounded capacity (256). Subscribers
    /// that fall behind see `RecvError::Lagged` and must reconcile via
    /// `/v1/state`.
    fn subscribe(&self) -> broadcast::Receiver<SeqEvent>;

    /// Cancel and join every background task the store owns.
    ///
    /// Returns once all owned tasks have observed cancellation and exited (or
    /// the per-task timeout fires and the task is aborted best-effort).
    /// Failures are logged internally and not propagated — the process is
    /// exiting and there is no actionable recovery for the caller. Calling
    /// `shutdown` more than once is safe: the second and later calls observe
    /// the consumed handles and return immediately.
    async fn shutdown(&self);
}
