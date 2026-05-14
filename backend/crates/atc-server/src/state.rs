use std::sync::Arc;

use atc_core::{Job, RunnerPoolCapacity, WorkflowRun};
use atc_github::WebhookEvent;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::persist::PersistentStore;

/// Shared application state passed to all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// Write-path dispatch for domain events, read-path snapshot, and the
    /// subscribe seam for WS clients.
    ///
    /// Routes each incoming webhook event to the appropriate backend:
    /// - [`crate::persist::PgStore`] when database is configured.
    /// - [`crate::persist::InMemoryStore`] when running in in-memory mode.
    ///
    /// The trait object is `Arc<dyn PersistentStore>` so it can be cloned
    /// cheaply and used across `async` handler closures. WS handlers obtain
    /// their broadcast receiver via `persist.subscribe()`.
    pub persist: Arc<dyn PersistentStore>,
    /// HMAC-SHA256 secret for verifying GitHub webhook signatures.
    /// `None` means verification is skipped.
    pub webhook_secret: Option<String>,
    /// Operator-declared runner-pool capacities loaded from the YAML config
    /// at startup. Single source of truth — composed into every
    /// `StateSnapshot` response by `routes::state_handler`. Empty when the
    /// operator has not declared any pools.
    pub runner_pool_capacities: Vec<RunnerPoolCapacity>,
    /// Cancellation token for cooperative shutdown.
    ///
    /// Cloned from `main`'s `shutdown` token. WS handlers observe it via the
    /// first (biased) arm of their `select!` loop and emit
    /// `Message::Close(CloseFrame { code: 1001, reason: "going away" })`
    /// before returning. The active `PersistentStore` holds its own clone for
    /// its background tasks (listener + drain in PG mode, eviction in
    /// in-memory mode); axum's `with_graceful_shutdown` and the process
    /// metrics collector observe the same token from their spawn sites.
    pub shutdown: CancellationToken,
    /// Task tracker for spawned WS handler futures.
    ///
    /// Each WS handler future is wrapped via `ws_tracker.track_future(...)` in
    /// `ws_handler` before being passed to `ws.on_upgrade(...)`. `main` calls
    /// `ws_tracker.close()` followed by `ws_tracker.wait()` (bounded by
    /// `SHUTDOWN_TIMEOUT_WS`) during shutdown so connected clients have time
    /// to emit their Close frames before process exit.
    pub ws_tracker: TaskTracker,
}

/// A domain event annotated with a monotonic sequence number.
///
/// Carried over the broadcast channel and sent to WebSocket clients as JSON.
/// Clients use `seq` to reconcile the REST snapshot with the live event stream.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SeqEvent {
    /// Monotonic sequence number assigned at ingestion time.
    pub seq: u64,
    /// The domain event that was ingested.
    pub event: WebhookEvent,
}

/// REST state snapshot for client backfill.
///
/// Returned by `GET /v1/state`. `last_seq` is the highest committed sequence
/// number — clients discard buffered WS events with `seq <= last_seq`.
/// A snapshot at `last_seq: N` reflects all committed events with event seq <= N.
///
/// `runner_pool_capacities` carries operator-declared pool ceilings (loaded
/// from the YAML config and composed into the response by
/// `routes::state_handler`, **not** by the persistent store). It is annotated
/// `#[serde(default)]` so a snapshot from an older replica that does not
/// emit the field still deserializes — the field defaults to `Vec::new()`
/// and the frontend behaves as if no capacities were declared.
#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StateSnapshot {
    pub last_seq: u64,
    pub runs: Vec<WorkflowRun>,
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub runner_pool_capacities: Vec<RunnerPoolCapacity>,
}
