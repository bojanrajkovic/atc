use std::sync::Arc;

use atc_core::RunnerPoolCapacity;
use atc_persist::PersistentStore;
use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::config_watcher::ConfigEvent;

/// Shared application state passed to all Axum handlers via `State<Arc<AppState>>`.
pub struct AppState {
    /// Write-path dispatch for domain events, read-path snapshot, and the
    /// subscribe seam for WS clients.
    ///
    /// Routes each incoming webhook event to the appropriate backend
    /// (`PgStore` when a database is configured; `InMemoryStore` otherwise).
    /// The trait object is `Arc<dyn PersistentStore>` so it can be cloned
    /// cheaply and used across `async` handler closures. WS handlers obtain
    /// their broadcast receiver via `persist.subscribe()`.
    pub persist: Arc<dyn PersistentStore>,
    /// HMAC-SHA256 secret for verifying GitHub webhook signatures.
    /// `None` means verification is skipped.
    pub webhook_secret: Option<String>,
    /// Operator-declared runner-pool capacities loaded from the YAML config
    /// at startup and re-loaded by the `config_watcher` task on file changes.
    ///
    /// Wrapped in an async `RwLock` so the watcher can atomically replace the
    /// vector while route handlers and tests hold short read guards. Tokio's
    /// `RwLock` is write-preferring, so a sustained read load cannot starve
    /// the watcher's writes (relevant under heavy `/v1/state` traffic). Empty
    /// when the operator has not declared any pools.
    ///
    /// The watcher takes `write().await`, compares current to new under the
    /// guard, and replaces atomically if different. `routes::state_handler`
    /// takes a short `read().await` and clones into the snapshot response.
    pub runner_pool_capacities: RwLock<Vec<RunnerPoolCapacity>>,
    /// Broadcast sender for config-change events produced by `config_watcher`.
    ///
    /// The WS handler subscribes to this channel alongside `persist.subscribe()`
    /// and wraps each `ConfigEvent` variant in the wire `WireFrame` shape.
    /// Capacity matches the `CommittedEvent` channel (256). Lagged on this
    /// channel closes the WS connection symmetrically with the committed
    /// channel — client reconnects via the existing path and re-fetches
    /// `/v1/state`.
    pub config_events_tx: broadcast::Sender<ConfigEvent>,
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
