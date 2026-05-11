use std::sync::Arc;

use atc_core::{Job, WorkflowRun};
use atc_github::WebhookEvent;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::persist::PersistentStore;

/// Shared application state passed to all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// Write-path dispatch for domain events, and read-path snapshot.
    ///
    /// Routes each incoming webhook event to the appropriate backend:
    /// - [`crate::persist::PgStore`] when database is configured.
    /// - [`crate::persist::InMemoryStore`] when running in in-memory mode.
    ///
    /// The trait object is `Arc<dyn PersistentStore>` so it can be cloned
    /// cheaply and used across `async` handler closures.
    pub persist: Arc<dyn PersistentStore>,
    /// Broadcast channel sender for pushing domain events to WebSocket clients.
    ///
    /// In in-memory mode the webhook handler writes directly. In PG mode the
    /// drain task is the sole writer; the handler is silent.
    pub webhook_tx: broadcast::Sender<SeqEvent>,
    /// HMAC-SHA256 secret for verifying GitHub webhook signatures.
    /// `None` means verification is skipped.
    pub webhook_secret: Option<String>,
    /// Cancellation token for cooperative shutdown.
    ///
    /// Cloned from `main`'s `shutdown` token. WS handlers observe it via the
    /// first (biased) arm of their `select!` loop and emit
    /// `Message::Close(CloseFrame { code: 1001, reason: "going away" })`
    /// before returning. Other supervised surfaces (eviction, listener, drain,
    /// process metrics, axum's `with_graceful_shutdown`) observe the same
    /// token directly via their own clones in `main`.
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
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct StateSnapshot {
    pub last_seq: u64,
    pub runs: Vec<WorkflowRun>,
    pub jobs: Vec<Job>,
}
