use std::sync::Arc;
use std::time::Duration;

use metrics::{Counter, Gauge, Histogram};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Describe and set the `atc_build_info` gauge with compile-time labels.
///
/// Must be called after the global `metrics::Recorder` is installed (via
/// `otel::init_otel`). When OTel is disabled, the macro resolves through the
/// crate's no-op recorder; the describe/set pair becomes a cheap no-op.
///
/// This is the sole production emit site that retains the inline
/// `metrics::gauge!(…).set(1.0)` form. It is a real emit (not metadata-only)
/// but is fired exactly once at startup with compile-time labels and is never
/// touched again — caching its handle would be pure ceremony. The `describe_*!`
/// macros below have a separate exception rationale: they are genuinely
/// metadata-only and do not need a cached handle.
pub fn register_build_info() {
    metrics::describe_gauge!(
        "atc_build_info",
        "ATC build metadata (always 1; use labels for values)"
    );
    metrics::gauge!(
        "atc_build_info",
        "version" => env!("CARGO_PKG_VERSION"),
        // `git_describe` is `git describe --tags`: for ATC's release pipeline
        // (which only fires on `v*` tags) this is always the exact tag (e.g.
        // `v1.0.0`). For local builds it's `<latest-tag>-<offset>-g<sha>` or
        // a vergen-gix fallback string if the build environment has no git
        // history available (e.g. a tarball-sourced build). The startup log
        // line in main.rs surfaces the same value for operator visibility.
        "git_describe" => env!("VERGEN_GIT_DESCRIBE"),
        "git_sha" => env!("VERGEN_GIT_SHA"),
        "rustc_version" => env!("VERGEN_RUSTC_SEMVER"),
        "build_timestamp" => env!("VERGEN_BUILD_TIMESTAMP"),
        "target_triple" => env!("VERGEN_CARGO_TARGET_TRIPLE"),
    )
    .set(1.0);
}

/// Cached metric handles for every repeat-emit site in PG mode.
///
/// Constructed once per `PgStore` via [`PgMetrics::register`] after the global
/// `metrics::Recorder` is installed. Cloned cheaply (each handle is internally
/// `Arc<dyn …Fn>`) into the listener and drain task closures so every emit on
/// a hot path is a field access plus a single relaxed atomic update — no
/// registry lookup, no `Arc` allocation.
///
/// The struct also stops a recurrence of the `metrics-util` hash-contract bug
/// fixed in PR #153, where the inline `metrics::counter!()` form re-walked the
/// registry on every emit and a faulty `Key::hash` / `KeyHasher` pairing let
/// `metrics-exporter-otel` overwrite observable callbacks on every miss. A
/// cached handle hits the registry exactly once at handle creation, so the
/// entire bug class cannot reach hot-path emits.
pub struct PgMetrics {
    /// `atc_pg_write_failures_total{kind="parity"}` — PG rejected a write that
    /// `in-memory` would have accepted (`0 rows affected` from the predicated
    /// UPSERT). Page-worthy in production.
    pub write_failures_parity: Counter,
    /// `atc_pg_write_failures_total{kind="transient"}` — pool / commit / sqlx
    /// failure inside the webhook handler transaction.
    pub write_failures_transient: Counter,
    /// `atc_pg_notify_emitted_total{kind="run"}` — emitted from
    /// `PgStore::apply_run_event` after commit.
    pub notify_emitted_run: Counter,
    /// `atc_pg_notify_emitted_total{kind="job"}` — emitted from
    /// `PgStore::apply_job_event` after commit.
    pub notify_emitted_job: Counter,

    /// `atc_pg_notify_received_total` — incremented per NOTIFY by the listener.
    pub notify_received: Counter,
    /// `atc_pg_listener_recv_errors_total` — listener `recv()` errors (sqlx
    /// hides successful reconnects; this counts irrecoverable surfacings).
    pub listener_recv_errors: Counter,
    /// `atc_pg_wake_coalesced_total` — NOTIFYs observed while a drain pass
    /// was already in flight.
    pub wake_coalesced: Counter,

    /// `atc_pg_drain_passes_total` — drain task pass count (one per wake-up).
    pub drain_passes: Counter,
    /// `atc_pg_drain_rows_total` — outbox rows fetched across all passes.
    pub drain_rows: Counter,
    /// `atc_pg_drain_duplicate_skipped_total` — broadcasts suppressed by the
    /// ring-buffer dedup.
    pub drain_duplicate_skipped: Counter,
    /// `atc_pg_drain_unknown_kind_total` — outbox rows with an unrecognized
    /// `kind` discriminator.
    pub drain_unknown_kind: Counter,

    /// `atc_pg_outbox_lag_seconds` — event age at broadcast.
    pub outbox_lag: Histogram,
    /// `atc_pg_drain_pass_duration_seconds` — wall time for one drain pass.
    pub drain_pass_duration: Histogram,
    /// `atc_pg_drain_startup_seconds` — startup readiness latency (one
    /// observation per process).
    pub drain_startup: Histogram,
    /// `atc_pg_drain_shutdown_remaining_rows` — outbox rows past the drain's
    /// watermark at drain task exit (one observation per process).
    pub drain_shutdown_remaining_rows: Histogram,

    /// `atc_pg_broadcast_watermark` — highest outbox seq broadcast by this
    /// replica (commit-order cursor; per-replica).
    pub broadcast_watermark: Gauge,
    /// `atc_pg_min_pending_seq` — lowest pending NOTIFY seq registered with
    /// the gap-healing backstop, or NaN when the drain has caught up.
    pub min_pending_seq: Gauge,
}

impl PgMetrics {
    /// Describe every metric and cache its handle.
    ///
    /// MUST be called after the global `metrics::Recorder` is installed —
    /// handles cached before recorder install bind permanently to the no-op
    /// recorder and silently drop every emit. `PgStore::start` satisfies this
    /// precondition: production `main.rs` runs `otel::init_otel` before
    /// `PgStore::start`, and the integration test harness installs the
    /// recorder once per binary via the `OnceLock` guard in
    /// `tests/integration/common/mod.rs` before any test constructs a
    /// `PgStore`.
    ///
    /// Safe to call multiple times — e.g. multiple `PgStore` instances across
    /// integration tests in the same binary. `metrics-exporter-otel` keeps a
    /// single `(KeyName, MetricKind)` entry in its metadata table that is
    /// overwritten on each `describe_*!`, and the `metrics-util` registry
    /// deduplicates handle creation by `Key` — so every call returns
    /// equivalent handles bound to the same underlying registry entries.
    ///
    /// `atc_pg_in_memory_drift_total` is described here but no handle is
    /// cached: the metric is part of the documented surface but has no
    /// production emit site today.
    pub(crate) fn register() -> Arc<Self> {
        // Counters — write path (PgStore::apply_*_event).
        metrics::describe_counter!(
            "atc_pg_write_failures_total",
            "PG write failures by kind (parity or transient)"
        );
        metrics::describe_counter!(
            "atc_pg_in_memory_drift_total",
            "PG committed but in-memory apply diverged"
        );
        metrics::describe_counter!(
            "atc_pg_notify_emitted_total",
            "Notifications emitted from the webhook handler, by event kind"
        );

        // Counters — listener and drain tasks.
        metrics::describe_counter!(
            "atc_pg_notify_received_total",
            "Notifications received by the listener task"
        );
        metrics::describe_counter!(
            "atc_pg_listener_recv_errors_total",
            "Listener task recv() error events (sqlx reconnects internally; this counts irrecoverable surfacings)"
        );
        metrics::describe_counter!(
            "atc_pg_wake_coalesced_total",
            "NOTIFY arrivals observed by the listener while a drain pass was in flight"
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

        // Histograms.
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

        // Gauges.
        metrics::describe_gauge!(
            "atc_pg_broadcast_watermark",
            "Highest outbox seq broadcast by this replica (commit-order cursor; per-replica)"
        );
        metrics::describe_gauge!(
            "atc_pg_min_pending_seq",
            "Lowest pending NOTIFY seq registered with the gap-healing backstop, or NaN when the drain has caught up (sentinel)"
        );

        Arc::new(Self {
            write_failures_parity: metrics::counter!(
                "atc_pg_write_failures_total",
                "kind" => "parity",
            ),
            write_failures_transient: metrics::counter!(
                "atc_pg_write_failures_total",
                "kind" => "transient",
            ),
            notify_emitted_run: metrics::counter!(
                "atc_pg_notify_emitted_total",
                "kind" => "run",
            ),
            notify_emitted_job: metrics::counter!(
                "atc_pg_notify_emitted_total",
                "kind" => "job",
            ),

            notify_received: metrics::counter!("atc_pg_notify_received_total"),
            listener_recv_errors: metrics::counter!("atc_pg_listener_recv_errors_total"),
            wake_coalesced: metrics::counter!("atc_pg_wake_coalesced_total"),

            drain_passes: metrics::counter!("atc_pg_drain_passes_total"),
            drain_rows: metrics::counter!("atc_pg_drain_rows_total"),
            drain_duplicate_skipped: metrics::counter!("atc_pg_drain_duplicate_skipped_total"),
            drain_unknown_kind: metrics::counter!("atc_pg_drain_unknown_kind_total"),

            outbox_lag: metrics::histogram!("atc_pg_outbox_lag_seconds"),
            drain_pass_duration: metrics::histogram!("atc_pg_drain_pass_duration_seconds"),
            drain_startup: metrics::histogram!("atc_pg_drain_startup_seconds"),
            drain_shutdown_remaining_rows: metrics::histogram!(
                "atc_pg_drain_shutdown_remaining_rows"
            ),

            broadcast_watermark: metrics::gauge!("atc_pg_broadcast_watermark"),
            min_pending_seq: metrics::gauge!("atc_pg_min_pending_seq"),
        })
    }
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
