//! Persistence trait + small shared utilities for the ATC stores.
//!
//! The `PersistentStore` trait is the interface a `PgStore` or `InMemoryStore`
//! implements: write-path application of run/job events, snapshot reads,
//! liveness checks, broadcast subscription, and cooperative shutdown. Both
//! stores depend on this crate; `atc-server` consumes it through the active
//! `Arc<dyn PersistentStore>`.
//!
//! `LivenessError::DbUnreachable` wraps the inner error as a
//! `Box<dyn std::error::Error + Send + Sync + 'static>` so this crate does
//! not need to name a concrete storage-library error type (e.g. `sqlx::Error`).
//! `impl std::error::Error::source()` preserves the underlying error in
//! `/readyz` diagnostics and `error: {:?}` logs.
//!
//! `join::join_with_timeout` is the shared shutdown-join helper used by every
//! store's `shutdown()` impl. Lifted from `atc-server::shutdown` so the two
//! store crates can share a single canonical copy.

use atc_core::event::{JobEventEnvelope, RunEventEnvelope};
use tokio::sync::broadcast;

pub use atc_core::PersistError;

pub mod join;
pub use join::join_with_timeout;

// ---------------------------------------------------------------------------
// LivenessError
// ---------------------------------------------------------------------------

/// Error returned by [`PersistentStore::liveness_check`].
///
/// `DbUnreachable` wraps the underlying error opaquely so this crate does not
/// need to depend on `sqlx` (or any other storage-library) directly. The
/// `std::error::Error::source()` impl below exposes the inner error to log
/// formatters and `/readyz` diagnostics.
#[derive(Debug)]
pub enum LivenessError {
    /// The database (or other backing store) is unreachable.
    DbUnreachable(Box<dyn std::error::Error + Send + Sync + 'static>),
    /// The drain task heartbeat is stale (age exceeds the per-store threshold).
    DrainStale {
        /// Age of the last heartbeat in milliseconds.
        age_ms: i64,
    },
}

impl std::fmt::Display for LivenessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LivenessError::DbUnreachable(e) => write!(f, "db unreachable: {e}"),
            LivenessError::DrainStale { age_ms } => {
                write!(f, "drain heartbeat stale ({age_ms} ms old)")
            }
        }
    }
}

impl std::error::Error for LivenessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LivenessError::DbUnreachable(e) => Some(e.as_ref()),
            LivenessError::DrainStale { .. } => None,
        }
    }
}

// ---------------------------------------------------------------------------
// PersistentStore trait
// ---------------------------------------------------------------------------

/// A store that can durably apply domain events and return the allocated seq.
///
/// - PG-backed: opens its own transaction per call (UPSERT + outbox + notify → commit).
/// - In-memory: locks seq, applies pure functions, broadcasts, returns seq.
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
    /// For the in-memory store: locks seq, reads state under a read lock.
    /// For the PG-backed store: loads broadcast watermark (Acquire), opens
    /// REPEATABLE READ tx, reads runs/jobs, returns snapshot with watermark as
    /// `last_seq`.
    async fn read_snapshot(&self) -> Result<atc_wire::StateSnapshot, PersistError>;

    /// Check whether the store is live and healthy.
    ///
    /// For the in-memory store: always returns `Ok(())`.
    /// For the PG-backed store: runs `SELECT 1` to verify connectivity, then
    /// checks that the drain heartbeat is not stale (> 30 s).
    async fn liveness_check(&self) -> Result<(), LivenessError>;

    /// Subscribe to the store's domain-event broadcast stream.
    ///
    /// Each subscriber receives every event the store has emitted since the
    /// subscribe call, up to the channel's bounded capacity (256). Subscribers
    /// that fall behind see `RecvError::Lagged` and must reconcile via
    /// `/v1/state`.
    fn subscribe(&self) -> broadcast::Receiver<atc_wire::CommittedEvent>;

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// A `LivenessError::DbUnreachable` constructed from a synthetic inner
    /// error must surface that inner error through `std::error::Error::source`
    /// so log formatters and `/readyz` diagnostics can reach it.
    #[test]
    fn db_unreachable_source_preserves_inner_error() {
        let inner = std::io::Error::other("synthetic backend failure");
        let err = LivenessError::DbUnreachable(Box::new(inner));
        let source = std::error::Error::source(&err)
            .expect("DbUnreachable must expose its inner error via source()");
        assert!(source.to_string().contains("synthetic backend failure"));
    }

    /// `DrainStale` carries no inner error, so its `source()` must be `None`.
    #[test]
    fn drain_stale_source_is_none() {
        let err = LivenessError::DrainStale { age_ms: 42_000 };
        assert!(std::error::Error::source(&err).is_none());
    }
}
