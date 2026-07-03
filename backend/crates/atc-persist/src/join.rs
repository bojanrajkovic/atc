//! Shared shutdown-join helper.
//!
//! Both `atc-store-pg` and `atc-store-mem` (future PRs) join the
//! `JoinHandle`s they own through this helper from their `PersistentStore::shutdown`
//! impls; `atc-server::shutdown` calls it for the non-store tasks it still
//! owns (metrics collector, axum graceful drain).

use std::time::Duration;

use tokio::task::JoinHandle;

/// Join a `JoinHandle<()>` within the given timeout. On timeout, log an error
/// and call `AbortHandle::abort()` (best-effort; task may run until its next
/// await point). On task panic, log the error and continue. A task that was
/// externally cancelled (e.g. by a test's `AbortHandle::abort()`) is treated
/// as a clean exit and logged at `warn` rather than `error` — the join itself
/// is the intended outcome.
pub async fn join_with_timeout(handle: JoinHandle<()>, timeout: Duration, name: &'static str) {
    let abort = handle.abort_handle();
    match tokio::time::timeout(timeout, handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) if e.is_cancelled() => {
            tracing::warn!(task = name, "task was cancelled before clean exit");
        }
        Ok(Err(e)) => {
            tracing::error!(task = name, error.message = %e, "task ended with error");
        }
        Err(_elapsed) => {
            tracing::error!(
                task = name,
                "shutdown timeout exceeded; aborting (best-effort)"
            );
            abort.abort();
        }
    }
}
