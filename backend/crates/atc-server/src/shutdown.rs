//! Cooperative shutdown orchestration.
//!
//! On trigger (signal handler OR an unexpected serve-task exit), the single
//! `shutdown` token cancels every supervised surface — axum
//! (`with_graceful_shutdown`), listener, drain, eviction, process metrics
//! collector, and every WS handler. The orchestration then waits for tracked
//! WS handlers to flush `Close(1001 "going away")` frames, joins the spawned
//! serve task, and joins the remaining background-task handles, each within
//! a bounded timeout.
//!
//! Catch-up after a client reconnects is handled by `/v1/state` snapshot on a
//! healthy replica (see `docs/architecture/backend-server.md`); the dying
//! replica is not responsible for re-broadcasting unprocessed outbox rows, so
//! shutdown does not need to wait for the drain task to finish before closing
//! WS handlers.

use std::io;
use std::time::Duration;

use tokio::task::{JoinError, JoinHandle};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

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

/// Log an unexpected early exit of a spawned axum serve task. Used by the
/// trigger select arm: if a serve resolves before any signal, we log and
/// fire `shutdown.cancel()` so the rest of the orchestration proceeds.
///
/// Returns `true` if the exit represents a real failure (the task ended with
/// an error or panicked), `false` if it ended cleanly. Caller uses the value
/// to decide whether to propagate a non-zero exit code. A clean exit before
/// any signal is unusual but not a fault — it's most often the harmless
/// "serve task races signal handler" case during SIGTERM where graceful
/// shutdown completes before our `select!` polls the cancel arm.
fn log_early_serve_exit(serve: &'static str, res: Result<io::Result<()>, JoinError>) -> bool {
    match res {
        Ok(Ok(())) => {
            tracing::warn!(
                serve,
                "serve exited cleanly before shutdown signal; triggering shutdown"
            );
            false
        }
        Ok(Err(e)) => {
            tracing::error!(
                serve, error = %e,
                "serve exited with error before shutdown signal; triggering shutdown"
            );
            true
        }
        Err(e) => {
            tracing::error!(
                serve, error = %e,
                "serve task panicked or was cancelled before shutdown signal; triggering shutdown"
            );
            true
        }
    }
}

/// Await a serve task that may or may not still be running. `None` means the
/// serve already resolved in the trigger select; `Some` is awaited and any
/// error is logged.
async fn await_optional_serve(serve: &'static str, handle: Option<JoinHandle<io::Result<()>>>) {
    if let Some(handle) = handle {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(serve, error = %e, "serve ended with error"),
            Err(e) => tracing::error!(serve, error = %e, "serve task ended with error"),
        }
    }
}

/// Orchestrates the cooperative shutdown sequence.
///
/// **Trigger** (begins on either of two events):
/// - The signal handler cancels `shutdown` (SIGTERM / SIGINT path), OR
/// - The spawned serve task exits unexpectedly before any signal arrives
///   (e.g., an accept-loop failure). In that case we cancel `shutdown`
///   ourselves so the remaining tasks shut down cooperatively rather than
///   getting orphaned.
///
/// Once `shutdown` is cancelled, every supervised surface — axum, listener,
/// drain, eviction, process metrics collector, and every WS handler —
/// observes the same token and exits at its own next opportunity. The
/// orchestration:
///
/// 1. Wait for the trigger.
/// 2. Wait for tracked WS handlers to drain (bounded by `SHUTDOWN_TIMEOUT_WS`).
/// 3. Join the spawned serve task (bounded by `SHUTDOWN_TIMEOUT_SERVES`).
/// 4. Join drain / listener / eviction / metrics handles (each bounded).
///
/// # Returns
/// `true` if shutdown was triggered by an early serve-task exit (i.e., the
/// HTTP service went down before any signal arrived). The caller should map
/// this to a non-zero process exit code so failure-oriented supervisors
/// (Kubernetes, systemd) restart the pod and alert.
///
/// `false` on the normal signal-driven path.
///
/// # Parameters
/// - `shutdown`: The shared cancellation token. Awaited here for the trigger;
///   cloned into AppState and into every background-task spawn site by the caller.
/// - `ws_tracker`: TaskTracker wrapping WS handler futures; `close()` +
///   `wait()` called by this function.
/// - `main_serve_task`: Spawned `JoinHandle<io::Result<()>>` from main axum serve.
/// - `drain_handle`: `Some` in PG mode; `None` in in-memory mode.
/// - `listener_handle`: `Some` in PG mode; `None` in in-memory mode.
/// - `eviction_handle`: Always `Some`.
/// - `metrics_handle`: Always `Some`.
#[allow(clippy::too_many_arguments)]
pub async fn run_shutdown_orchestration(
    shutdown: CancellationToken,
    ws_tracker: TaskTracker,
    main_serve_task: JoinHandle<io::Result<()>>,
    drain_handle: Option<JoinHandle<()>>,
    listener_handle: Option<JoinHandle<()>>,
    eviction_handle: JoinHandle<()>,
    metrics_handle: JoinHandle<()>,
) -> bool {
    // Step 1: Wait for the shutdown trigger — signal handler OR an early
    // serve-task exit. Wrap the serve in `Option` so step 3 doesn't
    // double-await it if it already resolved here.
    let mut main_serve_task = Some(main_serve_task);
    let mut serve_failure = false;

    // `biased;` prefers `shutdown.cancelled()` over the serve-task arm when
    // both are ready, so a SIGTERM-driven shutdown that races serve-task
    // resolution is consistently classified as a normal shutdown rather than
    // an early serve exit. Even with biased ordering, if the serve arm wins the
    // select, `log_early_serve_exit` returns `false` for clean Ok(Ok(()))
    // exits (no false-positive failure exits).
    tokio::select! {
        biased;
        () = shutdown.cancelled() => {
            // Normal path: signal handler fired shutdown.cancel().
        }
        res = main_serve_task.as_mut().expect("just constructed as Some") => {
            serve_failure = log_early_serve_exit("main", res);
            main_serve_task = None;
            shutdown.cancel();
        }
    }

    // Step 2: Wait for tracked WS handler tasks to flush Close(1001) frames
    // and exit. Bounded so a stalled client can't delay shutdown indefinitely;
    // anything still tracked when the timeout fires is reaped by runtime drop.
    ws_tracker.close();
    if tokio::time::timeout(SHUTDOWN_TIMEOUT_WS, ws_tracker.wait())
        .await
        .is_err()
    {
        tracing::error!(
            task = "ws_tracker",
            "shutdown timeout exceeded; runtime drop will kill remaining handlers"
        );
    }

    // Step 3: Join the spawned serve task. If it already resolved at step 1
    // it's `None` and skipped. Otherwise it should already be resolving since
    // shutdown was cancelled. Bounded by SHUTDOWN_TIMEOUT_SERVES.
    if tokio::time::timeout(
        SHUTDOWN_TIMEOUT_SERVES,
        await_optional_serve("main", main_serve_task.take()),
    )
    .await
    .is_err()
    {
        tracing::error!("axum serve did not resolve within timeout; runtime drop will reap");
    }

    // Step 4: Join the remaining background-task handles. The drain task
    // observes the same `shutdown` token and exits between passes, so it
    // typically completes after step 1 fires. Listener / eviction / metrics
    // are simple ticker loops that exit at their next cancel poll.
    if let Some(handle) = drain_handle {
        join_with_timeout(handle, SHUTDOWN_TIMEOUT_DRAIN, "drain").await;
    }
    if let Some(handle) = listener_handle {
        join_with_timeout(handle, SHUTDOWN_TIMEOUT_LISTENER, "listener").await;
    }
    join_with_timeout(eviction_handle, SHUTDOWN_TIMEOUT_EVICTION, "eviction").await;
    join_with_timeout(metrics_handle, SHUTDOWN_TIMEOUT_METRICS, "metrics").await;

    serve_failure
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

    /// If the spawned serve task exits unexpectedly before any signal arrives,
    /// the orchestration must trigger `shutdown.cancel()` itself rather than
    /// hanging on the bare `shutdown.cancelled()` await — otherwise the
    /// process sits indefinitely while the HTTP service is already down.
    #[tokio::test]
    async fn serve_failure_without_signal_triggers_shutdown() {
        let shutdown = CancellationToken::new();
        let ws_tracker = TaskTracker::new();

        // Main serve fails immediately with an io::Error (simulating an
        // accept-loop failure shortly after startup).
        let main_serve_task: JoinHandle<io::Result<()>> =
            tokio::spawn(async { Err(io::Error::other("simulated accept-loop failure")) });

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
                ws_tracker,
                main_serve_task,
                None,
                None,
                eviction_handle,
                metrics_handle,
            ),
        )
        .await;

        let serve_failure =
            result.expect("orchestration must not hang when a serve task fails before any signal");
        assert!(
            shutdown.is_cancelled(),
            "orchestration must call shutdown.cancel() when a serve task fails early"
        );
        assert!(
            serve_failure,
            "orchestration must return true (serve_failure) so main exits with non-zero status"
        );
    }

    /// A serve task that exits cleanly with `Ok(())` before any signal must
    /// NOT mark `serve_failure = true`. This is the SIGTERM-race case where
    /// `axum::serve(...).with_graceful_shutdown(...)` resolves cleanly the
    /// instant the signal handler cancels the shutdown token; the trigger
    /// select might pick the serve arm before the cancel arm, but the exit
    /// is not a failure.
    #[tokio::test]
    async fn clean_serve_exit_before_signal_does_not_mark_failure() {
        let shutdown = CancellationToken::new();
        let ws_tracker = TaskTracker::new();

        // Main serve completes cleanly with Ok(()) immediately.
        let main_serve_task: JoinHandle<io::Result<()>> = tokio::spawn(async { Ok(()) });

        let eviction_handle: JoinHandle<()> = tokio::spawn(async {});
        let metrics_handle: JoinHandle<()> = tokio::spawn(async {});

        let result = tokio::time::timeout(
            Duration::from_secs(15),
            run_shutdown_orchestration(
                shutdown.clone(),
                ws_tracker,
                main_serve_task,
                None,
                None,
                eviction_handle,
                metrics_handle,
            ),
        )
        .await;

        let serve_failure = result.expect("orchestration must not hang");
        assert!(
            !serve_failure,
            "clean Ok(()) serve exit before signal must NOT mark serve_failure"
        );
        assert!(
            shutdown.is_cancelled(),
            "orchestration still cancels shutdown to drain remaining tasks"
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
