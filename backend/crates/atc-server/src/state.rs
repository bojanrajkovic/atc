use std::sync::Arc;

use atc_core::RunnerPoolCapacity;
use atc_persist::PersistentStore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

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
