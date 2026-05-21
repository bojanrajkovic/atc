//! Test seams for the PG store — `start_with_test_hooks`, the
//! `PgStoreTestHooks` / `PgStoreTestHandles` types, and synchronous
//! tick + atomic-accessor methods on `PgStore`.
//!
//! Every public surface in this module is gated behind
//! `#[cfg(any(test, feature = "test-support"))]`. The `test-support` feature
//! is activated by:
//!   - this crate's self-referential dev-dep (for in-crate `#[cfg(test)]`
//!     code that uses these helpers), and
//!   - `atc-server`'s cross-crate dev-dep
//!     (`atc-store-pg = { path = "../atc-store-pg", features = ["test-support"] }`)
//!     for the workspace integration test binary.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64};
use std::time::Duration;

use crate::TracedPool;
use atc_core::Clock;
use sqlx::postgres::PgListener;
use tokio::sync::Notify;
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;

use super::{PgStore, PgStoreStartError, retention};

/// Test hooks for [`PgStore::start_with_test_hooks`]. Mirrors the optional
/// instrumentation params on the existing `spawn_listener_task` /
/// `spawn_drain_task` signatures so existing fixtures can keep observing
/// listener / drain progress.
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
pub struct PgStoreTestHandles {
    pub drain_abort: AbortHandle,
    pub listener_abort: AbortHandle,
    pub last_drain_pass_at: Arc<AtomicI64>,
    pub broadcast_watermark: Arc<AtomicI64>,
}

impl PgStore {
    /// Test variant that mirrors [`PgStore::start`] but accepts optional
    /// instrumentation hooks (`received_counter`, `observed_passes`,
    /// `drain_started`, `drain_delay`) and returns a [`PgStoreTestHandles`]
    /// alongside the store so test fixtures can poll the watermark / abort
    /// the drain mid-pass.
    pub async fn start_with_test_hooks(
        clock: Arc<dyn Clock>,
        pool: TracedPool,
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

    /// Run one iteration of the outbox heartbeat synchronously. Exposed so
    /// integration tests can drive the heartbeat path deterministically
    /// without waiting for the task's tick interval. The spawned task uses
    /// the same per-tick body via the free function above; production code
    /// does not call this method.
    pub async fn outbox_heartbeat_once(&self) -> Result<(), sqlx::Error> {
        retention::outbox_heartbeat_tick(
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
    pub fn replica_id(&self) -> &str {
        self.replica_id.as_ref()
    }

    /// Local broadcast watermark atomic. Exposed for integration-test
    /// inspection; tests use `Release`/`Acquire` to seed and verify the
    /// drain-task contract.
    pub fn broadcast_watermark(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.broadcast_watermark)
    }

    /// Atomic mirror of the cluster-wide min broadcast watermark. Exposed for
    /// integration-test inspection.
    pub fn min_replica_watermark_atomic(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.min_replica_watermark_atomic)
    }

    /// Atomic mirror of the oldest outbox row age. Exposed for
    /// integration-test inspection.
    pub fn oldest_row_age_seconds_atomic(&self) -> Arc<AtomicI64> {
        Arc::clone(&self.oldest_row_age_seconds_atomic)
    }

    /// Run one iteration of the outbox sweep synchronously. Exposed so
    /// integration tests can drive the sweep path deterministically without
    /// waiting for the task's `OUTBOX_SWEEP_INTERVAL` tick. Returns the
    /// number of outbox rows deleted in this tick.
    pub async fn outbox_sweep_once(&self) -> Result<u64, sqlx::Error> {
        retention::outbox_sweep_tick(
            self.clock.as_ref(),
            &self.pool,
            self.outbox_retention,
            self.metrics.as_ref(),
        )
        .await
    }
}
