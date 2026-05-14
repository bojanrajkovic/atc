use std::sync::Arc;

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter};
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;

/// Meter scope name used by every `atc-server` instrument. Matches the scope
/// passed to `MeterProvider::meter` in `otel::init_otel` so production and the
/// test harness share one scope.
pub const METER_SCOPE: &str = "atc";

/// Set the `atc_build_info` gauge with compile-time labels.
///
/// One-shot emit at startup against the global meter installed by
/// `otel::init_otel`. When OTel is disabled the global meter is a no-op meter
/// and the gauge `record` is a cheap no-op. The labels (`version`,
/// `git_describe`, `git_sha`, `rustc_version`, `build_timestamp`,
/// `target_triple`) are baked at compile time via `env!` so this site is
/// allocation-free.
pub fn register_build_info() {
    let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
    let gauge = meter
        .f64_gauge("atc_build_info")
        .with_description("ATC build metadata (always 1; use labels for values)")
        .build();
    gauge.record(
        1.0,
        &[
            KeyValue::new("version", env!("CARGO_PKG_VERSION")),
            // `git_describe` is `git describe --tags`: for ATC's release pipeline
            // (which only fires on `v*` tags) this is always the exact tag (e.g.
            // `v1.0.0`). For local builds it's `<latest-tag>-<offset>-g<sha>` or
            // a vergen-gix fallback string if the build environment has no git
            // history available (e.g. a tarball-sourced build). The startup log
            // line in main.rs surfaces the same value for operator visibility.
            KeyValue::new("git_describe", env!("VERGEN_GIT_DESCRIBE")),
            KeyValue::new("git_sha", env!("VERGEN_GIT_SHA")),
            KeyValue::new("rustc_version", env!("VERGEN_RUSTC_SEMVER")),
            KeyValue::new("build_timestamp", env!("VERGEN_BUILD_TIMESTAMP")),
            KeyValue::new("target_triple", env!("VERGEN_CARGO_TARGET_TRIPLE")),
        ],
    );
}

/// Cached OTel instruments and pre-built attribute slices for every repeat-emit
/// site in PG mode.
///
/// Constructed once per `PgStore` via [`PgMetrics::register`] against the
/// global meter installed by `otel::init_otel`. Cloned cheaply (each
/// instrument is internally `Arc<dyn …>`) into the listener and drain task
/// closures so every emit on a hot path is a field access plus a single
/// pointer-equality compare inside the SDK — no instrument lookup, no
/// `KeyValue` allocation.
///
/// For attribute-bearing metrics (`atc_pg_write_failures_total{kind=…}`,
/// `atc_pg_notify_emitted_total{kind=…}`), the struct stores both the single
/// `Counter<u64>` instrument and a pre-built `[KeyValue; 1]` attribute slice
/// for each `kind`. Emit sites read `counter.add(1, &attrs_parity)` so the
/// `KeyValue` itself is never reallocated on a webhook path.
///
/// TODO(otel-0.32): once `tracing-opentelemetry` and `axum-otel-metrics`
/// publish releases targeting `opentelemetry 0.32` (upstream PRs
/// `tokio-rs/tracing-opentelemetry#258` and `ttys3/axum-otel-metrics#196`),
/// bump our SDK pin, enable `experimental_metrics_bound_instruments`, and
/// swap the `(instrument, [KeyValue; 1])` pairs below for real
/// `BoundCounter<u64>` handles via `Counter::bind(&[…])`. The hot-path shape
/// stays identical from the caller's perspective.
pub struct PgMetrics {
    /// `atc_pg_write_failures_total` — PG-side write failures, emitted with a
    /// `kind` attribute. `kind="parity"` fires when the PG UPSERT matches 0
    /// rows (the WHERE predicate rejected the transition under PG's view of
    /// state); `kind="transient"` fires on sqlx errors at `pool.begin()`,
    /// mid-transaction, or `tx.commit()`. Page-worthy in production.
    write_failures: Counter<u64>,
    pub(crate) attrs_parity: [KeyValue; 1],
    pub(crate) attrs_transient: [KeyValue; 1],

    /// `atc_pg_notify_emitted_total` — emitted from `PgStore::apply_*_event`
    /// after commit, attributed by event kind (`run` or `job`).
    notify_emitted: Counter<u64>,
    pub(crate) attrs_run: [KeyValue; 1],
    pub(crate) attrs_job: [KeyValue; 1],

    /// `atc_pg_notify_received_total` — incremented per NOTIFY by the listener.
    pub notify_received: Counter<u64>,
    /// `atc_pg_listener_recv_errors_total` — listener `recv()` errors (sqlx
    /// hides successful reconnects; this counts irrecoverable surfacings).
    pub listener_recv_errors: Counter<u64>,
    /// `atc_pg_wake_coalesced_total` — NOTIFYs observed while a drain pass
    /// was already in flight.
    pub wake_coalesced: Counter<u64>,

    /// `atc_pg_drain_passes_total` — drain task pass count (one per wake-up).
    pub drain_passes: Counter<u64>,
    /// `atc_pg_drain_rows_total` — outbox rows fetched across all passes.
    pub drain_rows: Counter<u64>,
    /// `atc_pg_drain_duplicate_skipped_total` — broadcasts suppressed by the
    /// ring-buffer dedup.
    pub drain_duplicate_skipped: Counter<u64>,
    /// `atc_pg_drain_unknown_kind_total` — outbox rows with an unrecognized
    /// `kind` discriminator.
    pub drain_unknown_kind: Counter<u64>,

    /// `atc_pg_outbox_lag_seconds` — event age at broadcast.
    pub outbox_lag: Histogram<f64>,
    /// `atc_pg_drain_pass_duration_seconds` — wall time for one drain pass.
    pub drain_pass_duration: Histogram<f64>,
    /// `atc_pg_drain_startup_seconds` — startup readiness latency (one
    /// observation per process).
    pub drain_startup: Histogram<f64>,
    /// `atc_pg_drain_shutdown_remaining_rows` — outbox rows past the drain's
    /// watermark at drain task exit (one observation per process).
    pub drain_shutdown_remaining_rows: Histogram<f64>,

    /// `atc_pg_broadcast_watermark` — highest outbox seq broadcast by this
    /// replica (commit-order cursor; per-replica).
    pub broadcast_watermark: Gauge<f64>,
    /// `atc_pg_min_pending_seq` — lowest pending NOTIFY seq registered with
    /// the gap-healing backstop, or NaN when the drain has caught up.
    pub min_pending_seq: Gauge<f64>,
}

impl PgMetrics {
    /// Build every PG-mode instrument against the global meter.
    ///
    /// MUST be called after `otel::init_otel` has installed the global meter
    /// provider. Production `main.rs` upholds this ordering; the integration
    /// harness installs an in-memory meter provider via the `OnceLock` guard
    /// in `tests/integration/common/mod.rs` before any test constructs a
    /// `PgStore`. When OTel is disabled the global is a no-op meter and every
    /// instrument resolves to a no-op — emits become cheap field accesses
    /// against zero-state instruments.
    ///
    /// Safe to call multiple times — the OTel SDK deduplicates instruments by
    /// `(name, kind, unit, description)` so repeated calls return equivalent
    /// handles against the same underlying meter entries. The integration
    /// test binary constructs multiple `PgStore` instances across tests; each
    /// re-registration is idempotent.
    ///
    /// `atc_pg_in_memory_drift_total` is declared but no handle is cached:
    /// the metric is part of the documented surface but has no production
    /// emit site today. Future writers that add an emit site MUST cache a
    /// handle here.
    pub(crate) fn register() -> Arc<Self> {
        let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
        Self::register_with_meter(&meter)
    }

    /// Variant that builds instruments against a caller-supplied meter. Used
    /// internally by [`PgMetrics::register`]; exposed only for symmetry with
    /// future test seams.
    fn register_with_meter(meter: &Meter) -> Arc<Self> {
        let write_failures = meter
            .u64_counter("atc_pg_write_failures_total")
            .with_description("PG write failures by kind (parity or transient)")
            .build();
        let _in_memory_drift = meter
            .u64_counter("atc_pg_in_memory_drift_total")
            .with_description("PG committed but in-memory apply diverged")
            .build();
        let notify_emitted = meter
            .u64_counter("atc_pg_notify_emitted_total")
            .with_description("Notifications emitted from the webhook handler, by event kind")
            .build();

        let notify_received = meter
            .u64_counter("atc_pg_notify_received_total")
            .with_description("Notifications received by the listener task")
            .build();
        let listener_recv_errors = meter
            .u64_counter("atc_pg_listener_recv_errors_total")
            .with_description(
                "Listener task recv() error events (sqlx reconnects internally; \
                 this counts irrecoverable surfacings)",
            )
            .build();
        let wake_coalesced = meter
            .u64_counter("atc_pg_wake_coalesced_total")
            .with_description(
                "NOTIFY arrivals observed by the listener while a drain pass was in flight",
            )
            .build();
        let drain_passes = meter
            .u64_counter("atc_pg_drain_passes_total")
            .with_description("Drain task pass count (one per wake-up)")
            .build();
        let drain_rows = meter
            .u64_counter("atc_pg_drain_rows_total")
            .with_description("Total outbox rows fetched by the drain task across all passes")
            .build();
        let drain_duplicate_skipped = meter
            .u64_counter("atc_pg_drain_duplicate_skipped_total")
            .with_description("Outbox rows whose broadcast was suppressed by the dedup ring buffer")
            .build();
        let drain_unknown_kind = meter
            .u64_counter("atc_pg_drain_unknown_kind_total")
            .with_description("Outbox rows with an unrecognized kind discriminator")
            .build();

        let outbox_lag = meter
            .f64_histogram("atc_pg_outbox_lag_seconds")
            .with_description(
                "Event age at broadcast: wall time between outbox row inserted_at and broadcast \
                 (one observation per broadcast row)",
            )
            .build();
        let drain_pass_duration = meter
            .f64_histogram("atc_pg_drain_pass_duration_seconds")
            .with_description(
                "Wall time for one drain pass, including paginated batches; \
                 heartbeat-only wakes excluded",
            )
            .build();
        let drain_startup = meter
            .f64_histogram("atc_pg_drain_startup_seconds")
            .with_description(
                "Startup readiness latency: wall time from watermark init through \
                 first drain pass exit (one observation per process)",
            )
            .build();
        let drain_shutdown_remaining_rows = meter
            .f64_histogram("atc_pg_drain_shutdown_remaining_rows")
            .with_description(
                "Outbox rows with seq above this replica's drain watermark at drain task exit \
                 (one observation per process; absent when the shutdown count query failed)",
            )
            .build();

        let broadcast_watermark = meter
            .f64_gauge("atc_pg_broadcast_watermark")
            .with_description(
                "Highest outbox seq broadcast by this replica \
                 (commit-order cursor; per-replica)",
            )
            .build();
        let min_pending_seq = meter
            .f64_gauge("atc_pg_min_pending_seq")
            .with_description(
                "Lowest pending NOTIFY seq registered with the gap-healing backstop, \
                 or NaN when the drain has caught up (sentinel)",
            )
            .build();

        Arc::new(Self {
            write_failures,
            attrs_parity: [KeyValue::new("kind", "parity")],
            attrs_transient: [KeyValue::new("kind", "transient")],
            notify_emitted,
            attrs_run: [KeyValue::new("kind", "run")],
            attrs_job: [KeyValue::new("kind", "job")],
            notify_received,
            listener_recv_errors,
            wake_coalesced,
            drain_passes,
            drain_rows,
            drain_duplicate_skipped,
            drain_unknown_kind,
            outbox_lag,
            drain_pass_duration,
            drain_startup,
            drain_shutdown_remaining_rows,
            broadcast_watermark,
            min_pending_seq,
        })
    }

    /// Increment `atc_pg_write_failures_total{kind="parity"}`.
    pub fn write_failure_parity(&self) {
        self.write_failures.add(1, &self.attrs_parity);
    }

    /// Increment `atc_pg_write_failures_total{kind="transient"}`.
    pub fn write_failure_transient(&self) {
        self.write_failures.add(1, &self.attrs_transient);
    }

    /// Increment `atc_pg_notify_emitted_total{kind="run"}`.
    pub fn notify_emitted_run(&self) {
        self.notify_emitted.add(1, &self.attrs_run);
    }

    /// Increment `atc_pg_notify_emitted_total{kind="job"}`.
    pub fn notify_emitted_job(&self) {
        self.notify_emitted.add(1, &self.attrs_job);
    }
}

/// Handle for the background process metrics observer spawned by
/// [`spawn_process_collector`].
///
/// The observer's spawned task is supervised by `tokio` and aborted at
/// shutdown via [`ProcessCollectorHandle::shutdown`]. The graceful-shutdown
/// orchestration in `shutdown.rs` awaits the join handle with a bounded
/// timeout after invoking shutdown, so a stuck observer cannot stall process
/// exit.
pub struct ProcessCollectorHandle {
    abort: AbortHandle,
    join: JoinHandle<()>,
}

impl ProcessCollectorHandle {
    /// Abort the underlying observer task and return the join handle so the
    /// shutdown orchestration can await it under a timeout.
    pub fn shutdown(self) -> JoinHandle<()> {
        self.abort.abort();
        self.join
    }

    /// Wrap an arbitrary spawned task in a `ProcessCollectorHandle`. Lets
    /// tests stub the process observer with a trivial future without paying
    /// the cost of spawning the real `sysinfo`-driven loop.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_join_handle(join: JoinHandle<()>) -> Self {
        let abort = join.abort_handle();
        Self { abort, join }
    }
}

/// Spawn the `opentelemetry-system-metrics` process observer and return a
/// handle that the shutdown orchestration can abort and join.
///
/// The observer ticks on a self-managed interval (driven by the standard
/// `OTEL_METRIC_EXPORT_INTERVAL` env var, default 30 seconds) and emits
/// `process.cpu.usage`, `process.cpu.utilization`, `process.memory.usage`,
/// `process.memory.virtual`, and `process.disk.io` against the supplied
/// meter. The emitter loops forever — there is no cooperative shutdown
/// surface on the upstream crate — so the wrapper uses `AbortHandle::abort()`
/// to terminate it. Aborting between ticks is the common case; aborting
/// mid-tick is safe because `init_process_observer` does no DB / network
/// work that would surface a panic on cancellation.
pub fn spawn_process_collector(_cancel: CancellationToken) -> ProcessCollectorHandle {
    let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
    let join = tokio::spawn(async move {
        if let Err(err) = opentelemetry_system_metrics::init_process_observer(meter).await {
            tracing::warn!(%err, "opentelemetry-system-metrics observer exited with error");
        }
    });
    let abort = join.abort_handle();
    ProcessCollectorHandle { abort, join }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    use super::spawn_process_collector;

    /// The handle's `shutdown()` aborts the spawned observer and returns a
    /// join handle that resolves promptly.
    #[tokio::test]
    async fn process_collector_aborts_on_shutdown() {
        let handle = spawn_process_collector(CancellationToken::new());
        let join = handle.shutdown();
        let result = timeout(Duration::from_millis(200), join).await;
        assert!(
            result.is_ok(),
            "process collector task did not exit within 200 ms after abort",
        );
    }
}
