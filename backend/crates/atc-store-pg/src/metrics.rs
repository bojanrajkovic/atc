//! PG-mode OTel metric registration and cached instrument handles.
//!
//! [`PgMetrics`] holds every [`opentelemetry::metrics::Counter`] /
//! [`opentelemetry::metrics::Histogram`] handle PG-mode code emits to, plus
//! pre-built `[KeyValue; 1]` slices for the two attribute-bearing counters
//! (`atc_pg_write_failures_total{kind=…}`, `atc_pg_notify_emitted_total{kind=…}`).
//! Constructed once per [`PgStore`](crate::store::PgStore) inside
//! `PgStore::start_inner`, then cloned into the listener/drain/heartbeat/sweep
//! task closures so every emit on a hot path is a field access plus a single
//! pointer-equality compare inside the SDK — no instrument lookup, no
//! `KeyValue` allocation.
//!
//! Observable gauges (`atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq`,
//! `atc_pg_outbox_min_replica_watermark`, `atc_pg_outbox_oldest_row_age_seconds`)
//! have no field on the struct because their callbacks close over
//! `Weak<AtomicI64>` references — production code does not call `record()` on
//! them; updating the underlying atomic IS the metric update.
//!
//! `register_build_info` and `spawn_process_collector` stay in
//! `atc-server::metrics` — they are server-lifecycle helpers, not PG-mode
//! emitters, and migrating them would cycle the `atc-store-pg → atc-server`
//! dep direction.

use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::{AtomicI64, Ordering};

use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram, Meter};

/// Meter scope name used by every `atc-server` instrument. Matches the scope
/// passed to `MeterProvider::meter` in `atc_server::otel::init_otel` so
/// production and the test harness share one scope.
pub const METER_SCOPE: &str = "atc";

/// Cached OTel instruments and pre-built attribute slices for every repeat-emit
/// site in PG mode.
///
/// Constructed once per `PgStore` via [`PgMetrics::register`] against the
/// global meter installed by `atc_server::otel::init_otel`. Cloned cheaply
/// (each instrument is internally `Arc<dyn …>`) into the listener and drain
/// task closures so every emit on a hot path is a field access plus a single
/// pointer-equality compare inside the SDK — no instrument lookup, no
/// `KeyValue` allocation.
///
/// For attribute-bearing metrics (`atc_pg_write_failures_total{kind=…}`,
/// `atc_pg_notify_emitted_total{kind=…}`), the struct stores both the single
/// `Counter<u64>` instrument and a pre-built `[KeyValue; 1]` attribute slice
/// for each `kind`. Emit sites read `counter.add(1, &attrs_parity)` so the
/// `KeyValue` itself is never reallocated on a webhook path.
///
/// `atc_pg_broadcast_watermark` and `atc_pg_min_pending_seq` are
/// `ObservableGauge<f64>` instruments whose callbacks close over the
/// `Arc<AtomicI64>` atomics that the listener and drain manipulate. The
/// SDK invokes the callbacks on every collection cycle, so the value
/// re-surfaces on every scrape even under delta-temporality readers.
/// Production code does not call `record()` on these gauges directly —
/// updating the underlying atomic is sufficient.
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

    /// `atc_pg_outbox_rows_deleted_total` — outbox rows deleted by this
    /// replica's sweep task. Counter; no attributes. See `metrics.md`
    /// § Outbox retention.
    pub outbox_rows_deleted: Counter<u64>,
    // The two observable gauges below (`atc_pg_outbox_min_replica_watermark`,
    // `atc_pg_outbox_oldest_row_age_seconds`) have no field here because their
    // callbacks close over `Weak<AtomicI64>` references registered with the
    // meter — production code does not call `record()` on them; updating the
    // heartbeat-owned atomics IS the metric update.
}

impl PgMetrics {
    /// Build every PG-mode instrument against the global meter, backing the
    /// two observable gauges with the supplied atomics.
    ///
    /// MUST be called after `atc_server::otel::init_otel` has installed the
    /// global meter provider. Production `main.rs` upholds this ordering; the
    /// integration harness installs an in-memory meter provider via the
    /// `OnceLock` guard in `tests/integration/common/mod.rs` before any test
    /// constructs a `PgStore`. When OTel is disabled the global is a no-op
    /// meter and every instrument resolves to a no-op — emits become cheap
    /// field accesses against zero-state instruments.
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
    pub fn register(
        broadcast_watermark: &Arc<AtomicI64>,
        min_pending_seq: &Arc<AtomicI64>,
        min_replica_watermark: &Arc<AtomicI64>,
        oldest_row_age_seconds: &Arc<AtomicI64>,
    ) -> Arc<Self> {
        let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
        Self::register_with_meter(
            &meter,
            Arc::downgrade(broadcast_watermark),
            Arc::downgrade(min_pending_seq),
            Arc::downgrade(min_replica_watermark),
            Arc::downgrade(oldest_row_age_seconds),
        )
    }

    /// Variant that builds instruments against a caller-supplied meter. Used
    /// internally by [`PgMetrics::register`]; exposed only for symmetry with
    /// future test seams.
    ///
    /// The observable gauges take `Weak<AtomicI64>` instead of `Arc<…>` so
    /// callbacks for prior `PgStore` instances (e.g. earlier integration
    /// tests sharing the same process-global meter) become no-ops once the
    /// store and its listener/drain tasks finish dropping their strong
    /// references. OTel SDK 0.31's `pipeline::register_callback` appends to
    /// a `Vec<GenericCallback>` with no dedup, so multiple registrations
    /// against the same meter would otherwise produce one observation per
    /// historical store on every collection cycle, and the LAST_VALUE
    /// aggregator would expose a value from whichever callback the SDK
    /// happens to invoke last — non-deterministic and likely sourced from a
    /// dead store. The Weak upgrade fails the moment the active store's
    /// listener and drain tasks join, so only the most recently constructed
    /// store ever observes a value.
    fn register_with_meter(
        meter: &Meter,
        broadcast_watermark: Weak<AtomicI64>,
        min_pending_seq: Weak<AtomicI64>,
        min_replica_watermark: Weak<AtomicI64>,
        oldest_row_age_seconds: Weak<AtomicI64>,
    ) -> Arc<Self> {
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

        let outbox_rows_deleted = meter
            .u64_counter("atc_pg_outbox_rows_deleted_total")
            .with_description(
                "Outbox rows deleted by this replica's retention sweep task \
                 (counted via the sweep CTE's RETURNING seq)",
            )
            .build();

        // Observable gauges read from the listener/drain-owned atomics on
        // every collection cycle. No call site needs to record() — updating
        // the AtomicI64 IS the metric update. The Weak upgrade short-circuits
        // the callback to a no-op once the store and its tasks finish
        // dropping their strong refs, so only the active store ever observes
        // a value (see the `register_with_meter` doc for the multi-store
        // accumulation story).
        let watermark_observer = broadcast_watermark;
        let _broadcast_watermark = meter
            .f64_observable_gauge("atc_pg_broadcast_watermark")
            .with_description(
                "Highest outbox seq broadcast by this replica \
                 (commit-order cursor; per-replica)",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = watermark_observer.upgrade() {
                    #[allow(clippy::cast_precision_loss)]
                    let value = atomic.load(Ordering::Acquire) as f64;
                    observer.observe(value, &[]);
                }
            })
            .build();

        let min_pending_observer = min_pending_seq;
        let _min_pending_seq = meter
            .f64_observable_gauge("atc_pg_min_pending_seq")
            .with_description(
                "Lowest pending NOTIFY seq registered with the gap-healing backstop, \
                 or NaN when the drain has caught up (sentinel)",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = min_pending_observer.upgrade() {
                    let raw = atomic.load(Ordering::Acquire);
                    #[allow(clippy::cast_precision_loss)]
                    let value = if raw == i64::MAX {
                        f64::NAN
                    } else {
                        raw as f64
                    };
                    observer.observe(value, &[]);
                }
            })
            .build();

        // Outbox retention gauges. Atomic-mirrored — the heartbeat task is
        // the writer; the observable-gauge callbacks read synchronously from
        // `Weak<AtomicI64>` so callbacks belonging to dropped stores
        // short-circuit to no-ops once the active store's heartbeat task
        // joins. -1 in the underlying atomic is the NaN sentinel; rendered
        // as NaN on dashboards.
        let min_replica_observer = min_replica_watermark;
        let _min_replica_watermark = meter
            .f64_observable_gauge("atc_pg_outbox_min_replica_watermark")
            .with_description(
                "MIN(broadcast_watermark) across non-stale replicas (cluster-wide outbox \
                 retention floor). NaN when no live replicas have heartbeated yet. \
                 Refreshed every 30 s by the outbox heartbeat task.",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = min_replica_observer.upgrade() {
                    let raw = atomic.load(Ordering::Acquire);
                    #[allow(clippy::cast_precision_loss)]
                    let value = if raw < 0 { f64::NAN } else { raw as f64 };
                    observer.observe(value, &[]);
                }
            })
            .build();

        let oldest_row_age_observer = oldest_row_age_seconds;
        let _oldest_row_age = meter
            .f64_observable_gauge("atc_pg_outbox_oldest_row_age_seconds")
            .with_description(
                "Age in seconds of the oldest outbox row (clock-bound). NaN when the \
                 outbox is empty. Refreshed every 30 s by the outbox heartbeat task.",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = oldest_row_age_observer.upgrade() {
                    let raw = atomic.load(Ordering::Acquire);
                    #[allow(clippy::cast_precision_loss)]
                    let value = if raw < 0 { f64::NAN } else { raw as f64 };
                    observer.observe(value, &[]);
                }
            })
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
            outbox_rows_deleted,
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
