//! Graceful shutdown orchestration for ATC's two-phase cooperative shutdown.
//!
//! Phase 1: `shutdown` token cancels axum serves, listener, drain, eviction,
//! and process-metrics tasks. The drain handle is awaited (bounded) to ensure
//! the current outbox pass finishes before proceeding to phase 2.
//!
//! Phase 2: `ws_close` token fires, causing WS handlers to send
//! `Close(1001 "going away")`. The TaskTracker drains (bounded). Then the
//! spawned serve tasks and remaining background handles are joined.

use std::io;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::state::SeqEvent;

/// Per-task timeout budgets. Aggregate worst-case shutdown: ~13 seconds.
/// K8s `terminationGracePeriodSeconds` defaults to 30; well within budget.
pub const SHUTDOWN_TIMEOUT_DRAIN: Duration = Duration::from_secs(5);
pub const SHUTDOWN_TIMEOUT_WS: Duration = Duration::from_secs(2);
pub const SHUTDOWN_TIMEOUT_SERVES: Duration = Duration::from_secs(3);
pub const SHUTDOWN_TIMEOUT_LISTENER: Duration = Duration::from_secs(1);
pub const SHUTDOWN_TIMEOUT_EVICTION: Duration = Duration::from_secs(1);
pub const SHUTDOWN_TIMEOUT_METRICS: Duration = Duration::from_secs(1);

/// Join a `JoinHandle<()>` within the given timeout. On timeout, log an error
/// and call `AbortHandle::abort()` (best-effort; task may run until its next
/// await point). On task panic/cancellation error, log the error and continue.
pub async fn join_with_timeout(handle: JoinHandle<()>, timeout: Duration, name: &'static str) {
    let abort = handle.abort_handle();
    match tokio::time::timeout(timeout, handle).await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!(task = name, error = %e, "task ended with error");
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

/// Orchestrates the two-phase cooperative shutdown sequence.
///
/// **Phase 1** (begins on either of two triggers):
/// - The signal handler cancels `shutdown` (SIGTERM / SIGINT path), OR
/// - One of the spawned serve tasks exits unexpectedly before any signal
///   arrives (e.g., an accept-loop failure). In that case we cancel
///   `shutdown` ourselves so the remaining listener / drain / eviction /
///   metrics tasks shut down cooperatively rather than getting orphaned.
///
/// Once phase 1 is triggered:
/// - The drain handle is awaited (bounded by `SHUTDOWN_TIMEOUT_DRAIN`) to let
///   the current outbox pass finish before WS handlers close.
///
/// **Phase 2:**
/// - `ws_close.cancel()` fires, causing WS handlers to send `Close(1001)`.
/// - `ws_tracker.wait()` (bounded) drains the in-flight WS task count.
/// - Spawned serve tasks are joined (bounded).
/// - Remaining background handles are joined (bounded).
///
/// # Parameters
/// - `shutdown`: The phase-1 token. Awaited here so the caller can run this
///   as a spawned task or directly in main.
/// - `ws_close`: The phase-2 token. Cancelled by this function after drain.
/// - `ws_tracker`: TaskTracker wrapping WS handler futures; `close()` +
///   `wait()` called by this function.
/// - `webhook_tx_keepalive`: A broadcast sender clone held alive through step 5.
///   When the axum serve future completes (at step 2), it drops `AppState` and
///   its embedded `webhook_tx` sender. If the drain task's sender is also dropped
///   at step 3, the broadcast channel closes and WS handlers exit via
///   `RecvError::Closed` before `ws_close.cancel()` can fire at step 4. Keeping
///   one sender alive through step 5 prevents premature channel closure.
/// - `main_serve_task`: Spawned `JoinHandle<io::Result<()>>` from main axum serve.
/// - `metrics_serve_task`: Spawned `JoinHandle<io::Result<()>>` from metrics serve.
/// - `drain_handle`: `Some` in PG mode; `None` in in-memory mode.
/// - `listener_handle`: `Some` in PG mode; `None` in in-memory mode.
/// - `eviction_handle`: Always `Some`.
/// - `metrics_handle`: Always `Some`.
#[allow(clippy::too_many_arguments)]
pub async fn run_shutdown_orchestration(
    shutdown: CancellationToken,
    ws_close: CancellationToken,
    ws_tracker: TaskTracker,
    webhook_tx_keepalive: broadcast::Sender<SeqEvent>,
    main_serve_task: JoinHandle<io::Result<()>>,
    metrics_serve_task: JoinHandle<io::Result<()>>,
    drain_handle: Option<JoinHandle<()>>,
    listener_handle: Option<JoinHandle<()>>,
    eviction_handle: JoinHandle<()>,
    metrics_handle: JoinHandle<()>,
) {
    // Step 2: Wait for the phase-1 shutdown signal, OR for either spawned
    // serve task to exit unexpectedly (which would mean the HTTP service is
    // already down). Without this select, the function would hang on
    // `shutdown.cancelled()` indefinitely if a serve task failed early and no
    // signal ever arrived. Wrap each serve in `Option` so step 6 doesn't
    // double-await whichever one already resolved here.
    let mut main_serve_task = Some(main_serve_task);
    let mut metrics_serve_task = Some(metrics_serve_task);

    tokio::select! {
        () = shutdown.cancelled() => {
            // Normal path: signal handler fired shutdown.cancel().
        }
        res = main_serve_task.as_mut().expect("just constructed as Some") => {
            match res {
                Ok(Ok(())) => tracing::warn!(
                    "main serve exited cleanly before shutdown signal; triggering shutdown"
                ),
                Ok(Err(e)) => tracing::error!(
                    error = %e,
                    "main serve exited with error before shutdown signal; triggering shutdown"
                ),
                Err(e) => tracing::error!(
                    error = %e,
                    "main serve task panicked or was cancelled before shutdown signal; triggering shutdown"
                ),
            }
            main_serve_task = None;
            shutdown.cancel();
        }
        res = metrics_serve_task.as_mut().expect("just constructed as Some") => {
            match res {
                Ok(Ok(())) => tracing::warn!(
                    "metrics serve exited cleanly before shutdown signal; triggering shutdown"
                ),
                Ok(Err(e)) => tracing::error!(
                    error = %e,
                    "metrics serve exited with error before shutdown signal; triggering shutdown"
                ),
                Err(e) => tracing::error!(
                    error = %e,
                    "metrics serve task panicked or was cancelled before shutdown signal; triggering shutdown"
                ),
            }
            metrics_serve_task = None;
            shutdown.cancel();
        }
    }

    // Step 3: Await the drain handle (PG mode only). The drain exits
    // cooperatively after its current pass finishes. We bound the wait to
    // SHUTDOWN_TIMEOUT_DRAIN; if it exceeds that, we abort and continue.
    if let Some(handle) = drain_handle {
        let abort = handle.abort_handle();
        match tokio::time::timeout(SHUTDOWN_TIMEOUT_DRAIN, handle).await {
            Ok(Ok(())) => tracing::info!("drain task exited cleanly"),
            Ok(Err(e)) => tracing::error!(error = %e, "drain task ended with error"),
            Err(_elapsed) => {
                tracing::error!(
                    task = "drain",
                    "shutdown timeout exceeded; aborting (best-effort)"
                );
                abort.abort();
            }
        }
    }

    // Step 4: Fire phase 2 — WS handlers send Close(1001 "going away").
    ws_close.cancel();

    // Step 5: Wait for tracked WS tasks to drain. Drop the keepalive sender
    // only after wait() returns — this ensures the broadcast channel stays open
    // long enough for WS handlers to receive ws_close.cancel() rather than
    // exiting via RecvError::Closed (which produces a TCP reset, not Close(1001)).
    ws_tracker.close();
    match tokio::time::timeout(SHUTDOWN_TIMEOUT_WS, ws_tracker.wait()).await {
        Ok(()) => tracing::info!("ws tracker drained cleanly"),
        Err(_elapsed) => tracing::error!(
            task = "ws_tracker",
            "shutdown timeout exceeded; runtime drop will kill remaining handlers"
        ),
    }
    // Explicitly drop the keepalive here (after ws_tracker.wait()) to document
    // the intent. Without this, it would be dropped at function end (step 7),
    // which is also fine, but explicit is clearer.
    drop(webhook_tx_keepalive);

    // Step 6: Wait for the spawned serve tasks to complete. Whichever serve
    // already resolved in step 2 is `None`; the other (or both, on the normal
    // signal-driven path) is `Some` and should already be resolving (its
    // graceful_shutdown future fired when shutdown was cancelled). We bound
    // the wait with SHUTDOWN_TIMEOUT_SERVES in case a connection stalled.
    let main_pending = main_serve_task.take();
    let metrics_pending = metrics_serve_task.take();
    let serves_future = async {
        let main_res = match main_pending {
            Some(h) => Some(h.await),
            None => None,
        };
        let metrics_res = match metrics_pending {
            Some(h) => Some(h.await),
            None => None,
        };
        (main_res, metrics_res)
    };
    match tokio::time::timeout(SHUTDOWN_TIMEOUT_SERVES, serves_future).await {
        Ok((main_res, metrics_res)) => {
            if let Some(Ok(Err(e))) = main_res {
                tracing::error!(error = %e, "main serve ended with error");
            }
            if let Some(Ok(Err(e))) = metrics_res {
                tracing::error!(error = %e, "metrics serve ended with error");
            }
        }
        Err(_elapsed) => {
            tracing::error!("axum serves did not resolve within timeout; runtime drop will reap");
        }
    }

    // Step 7: Join remaining background-task handles.
    // Listener is None in in-memory mode.
    if let Some(handle) = listener_handle {
        join_with_timeout(handle, SHUTDOWN_TIMEOUT_LISTENER, "listener").await;
    }
    join_with_timeout(eviction_handle, SHUTDOWN_TIMEOUT_EVICTION, "eviction").await;
    join_with_timeout(metrics_handle, SHUTDOWN_TIMEOUT_METRICS, "metrics").await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deliberately-stuck future exceeds its timeout budget: `join_with_timeout`
    /// logs an error (observable via the tracing subscriber) and returns within
    /// bounded time, and the abort handle fires.
    #[tokio::test]
    async fn stuck_handle_aborted_after_timeout() {
        // Spawn a task that will never complete.
        let handle: JoinHandle<()> = tokio::spawn(async {
            std::future::pending::<()>().await;
        });

        // The join helper should return well within 1 second (timeout = 50 ms).
        let test_timeout = Duration::from_secs(1);
        let task_timeout = Duration::from_millis(50);

        tokio::time::timeout(
            test_timeout,
            join_with_timeout(handle, task_timeout, "stuck_test_task"),
        )
        .await
        .expect("join_with_timeout should return within test_timeout even for stuck tasks");

        // If we reach here, the function returned — abort was called and we
        // didn't deadlock. The task itself may still be running briefly
        // (abort is asynchronous), but the orchestration function proceeded.
    }

    /// If a spawned serve task exits unexpectedly before any signal arrives,
    /// the orchestration must trigger `shutdown.cancel()` itself rather than
    /// hanging on the bare `shutdown.cancelled()` await — otherwise the
    /// process sits indefinitely while the HTTP service is already down.
    #[tokio::test]
    async fn serve_failure_without_signal_triggers_shutdown() {
        let shutdown = CancellationToken::new();
        let ws_close = CancellationToken::new();
        let ws_tracker = TaskTracker::new();
        let (webhook_tx, _rx) = broadcast::channel::<SeqEvent>(16);

        // Main serve fails immediately with an io::Error (simulating an
        // accept-loop failure shortly after startup).
        let main_serve_task: JoinHandle<io::Result<()>> =
            tokio::spawn(async { Err(io::Error::other("simulated accept-loop failure")) });

        // Metrics serve hangs forever — the SHUTDOWN_TIMEOUT_SERVES backstop
        // catches it; this verifies we don't deadlock waiting for it.
        let metrics_serve_task: JoinHandle<io::Result<()>> =
            tokio::spawn(async { std::future::pending::<io::Result<()>>().await });

        // Stub eviction/metrics handles that exit immediately.
        let eviction_handle: JoinHandle<()> = tokio::spawn(async {});
        let metrics_handle: JoinHandle<()> = tokio::spawn(async {});

        // Bound the whole orchestration with a generous test timeout. The
        // expected wall time is dominated by SHUTDOWN_TIMEOUT_SERVES (3 s)
        // plus a little overhead.
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            run_shutdown_orchestration(
                shutdown.clone(),
                ws_close,
                ws_tracker,
                webhook_tx,
                main_serve_task,
                metrics_serve_task,
                None,
                None,
                eviction_handle,
                metrics_handle,
            ),
        )
        .await;

        assert!(
            result.is_ok(),
            "orchestration must not hang when a serve task fails before any signal"
        );
        assert!(
            shutdown.is_cancelled(),
            "orchestration must call shutdown.cancel() when a serve task fails early"
        );
    }

    /// A normally-completing handle resolves cleanly within its timeout.
    #[tokio::test]
    async fn clean_handle_resolves_normally() {
        let handle: JoinHandle<()> = tokio::spawn(async {
            // completes immediately
        });

        let result = tokio::time::timeout(
            Duration::from_secs(1),
            join_with_timeout(handle, Duration::from_secs(1), "clean_test_task"),
        )
        .await;

        assert!(
            result.is_ok(),
            "join_with_timeout should complete for a task that exits promptly"
        );
    }
}
