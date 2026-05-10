use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Describe and set the `atc_build_info` gauge with compile-time labels.
///
/// Must be called after the global `metrics::Recorder` is installed (via
/// `otel::init_otel`). When OTel is disabled, the macro resolves through the
/// crate's no-op recorder; the describe/set pair becomes a cheap no-op.
pub fn register_build_info() {
    metrics::describe_gauge!(
        "atc_build_info",
        "ATC build metadata (always 1; use labels for values)"
    );
    metrics::gauge!(
        "atc_build_info",
        "version" => env!("CARGO_PKG_VERSION"),
        "git_sha" => env!("VERGEN_GIT_SHA"),
        "rustc_version" => env!("VERGEN_RUSTC_SEMVER"),
        "build_timestamp" => env!("VERGEN_BUILD_TIMESTAMP"),
        "target_triple" => env!("VERGEN_CARGO_TARGET_TRIPLE"),
    )
    .set(1.0);
}

/// Register the PG write failure and drift counters.
///
/// Two labels distinguish failure kinds:
/// - `kind="parity"` — PG rejected a write that in-memory accepted (`0 rows affected`).
///   Page-worthy in production: the two stores have diverged.
/// - `kind="transient"` — sqlx error (network, pool exhaustion, etc.).
///   Alert on sustained rate.
pub fn register_pg_write_counters() {
    metrics::describe_counter!(
        "atc_pg_write_failures_total",
        "PG write failures by kind (parity or transient)"
    );
    metrics::describe_counter!(
        "atc_pg_in_memory_drift_total",
        "PG committed but in-memory apply diverged"
    );
}

/// Register listener and drain task metrics.
///
/// Counters:
/// - atc_pg_notify_emitted_total{kind} — emitted from `PgStore::apply_*_event` after commit (ADR 0005)
/// - atc_pg_notify_received_total — received by listener task
/// - atc_pg_listener_recv_errors_total — recv() errors (sqlx hides successful reconnects)
/// - atc_pg_drain_passes_total — drain task wake-ups
/// - atc_pg_drain_rows_total — outbox rows fetched across all passes
/// - atc_pg_drain_duplicate_skipped_total — broadcasts suppressed by ring-buffer dedup
/// - atc_pg_drain_unknown_kind_total — outbox rows with an unrecognized kind discriminator
/// - atc_pg_wake_coalesced_total — NOTIFYs observed while a drain pass was in flight
///
/// Histograms:
/// - atc_pg_outbox_lag_seconds — wall time between outbox row insert and broadcast (one per row)
/// - atc_pg_drain_pass_duration_seconds — wall time for one drain pass (including pagination)
/// - atc_pg_drain_startup_seconds — wall time from watermark init through first drain pass exit
/// - atc_pg_drain_shutdown_remaining_rows — outbox rows past this replica's watermark at drain task exit
///
/// Gauges:
/// - atc_pg_broadcast_watermark — highest outbox seq broadcast by this replica
/// - atc_pg_min_pending_seq — lowest pending NOTIFY seq below the watermark, NaN when caught up
pub fn register_listener_metrics() {
    metrics::describe_counter!(
        "atc_pg_notify_emitted_total",
        "Notifications emitted from the webhook handler, by event kind"
    );
    metrics::describe_counter!(
        "atc_pg_notify_received_total",
        "Notifications received by the listener task"
    );
    metrics::describe_counter!(
        "atc_pg_listener_recv_errors_total",
        "Listener task recv() error events (sqlx reconnects internally; this counts irrecoverable surfacings)"
    );
    metrics::describe_counter!(
        "atc_pg_drain_passes_total",
        "Drain task pass count (one per wake-up)"
    );
    metrics::describe_counter!(
        "atc_pg_drain_rows_total",
        "Total outbox rows fetched by the drain task across all passes"
    );
    metrics::describe_counter!(
        "atc_pg_drain_duplicate_skipped_total",
        "Outbox rows whose broadcast was suppressed by the dedup ring buffer"
    );
    metrics::describe_counter!(
        "atc_pg_drain_unknown_kind_total",
        "Outbox rows with an unrecognized kind discriminator"
    );
    metrics::describe_counter!(
        "atc_pg_wake_coalesced_total",
        "NOTIFY arrivals observed by the listener while a drain pass was in flight"
    );
    metrics::describe_histogram!(
        "atc_pg_outbox_lag_seconds",
        "Event age at broadcast: wall time between outbox row inserted_at and broadcast (one observation per broadcast row)"
    );
    metrics::describe_histogram!(
        "atc_pg_drain_pass_duration_seconds",
        "Wall time for one drain pass, including paginated batches; heartbeat-only wakes excluded"
    );
    metrics::describe_histogram!(
        "atc_pg_drain_startup_seconds",
        "Startup readiness latency: wall time from watermark init through first drain pass exit (one observation per process)"
    );
    metrics::describe_histogram!(
        "atc_pg_drain_shutdown_remaining_rows",
        "Outbox rows with seq above this replica's drain watermark at drain task exit (one observation per process; absent when the shutdown count query failed)"
    );
    metrics::describe_gauge!(
        "atc_pg_broadcast_watermark",
        "Highest outbox seq broadcast by this replica (commit-order cursor; per-replica)"
    );
    metrics::describe_gauge!(
        "atc_pg_min_pending_seq",
        "Lowest pending NOTIFY seq registered with the gap-healing backstop, or NaN when the drain has caught up (sentinel)"
    );
}

/// Describe process metrics and spawn a background collector that ticks every
/// 10 seconds.
///
/// Returns a [`JoinHandle`] that resolves when the task exits cooperatively
/// after `cancel` is cancelled. The task exits between ticks, not mid-collect:
/// `Collector::collect()` is a synchronous function that calls OS syscalls or
/// reads from `/proc` (on Linux), so it blocks the tokio worker for its
/// duration. Cancellation is checked only at the `ticker.tick().await` point;
/// the cancel arm fires at the next tick boundary after `cancel` is signalled.
/// This is acceptable: a single collect call typically completes in
/// microseconds, so shutdown latency attributable to mid-collect blocking is
/// negligible in practice.
pub fn spawn_process_collector(cancel: CancellationToken) -> JoinHandle<()> {
    let collector = metrics_process::Collector::default();
    collector.describe();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                _ = ticker.tick() => collector.collect(),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    use super::spawn_process_collector;

    #[tokio::test]
    async fn process_collector_exits_cooperatively_on_cancel() {
        let cancel = CancellationToken::new();
        let handle = spawn_process_collector(cancel.clone());

        cancel.cancel();

        let result = timeout(Duration::from_millis(200), handle).await;
        assert!(
            result.is_ok(),
            "process collector task did not exit within 200 ms after cancellation"
        );
    }
}
