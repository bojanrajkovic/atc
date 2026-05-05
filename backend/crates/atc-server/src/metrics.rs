use std::time::Duration;

use axum::http::header;
use axum::{Router, routing::get};
use axum_prometheus::PrometheusMetricLayer;

/// Prometheus text exposition format Content-Type.
///
/// Axum's default `IntoResponse` for a bare `String` emits
/// `text/plain; charset=utf-8`, which is missing the `version` parameter that
/// the Prometheus exposition format v0.0.4 spec requires. Real scrapers fall
/// back to defaults when the header is missing, but we set it explicitly so
/// the `/metrics` response matches the spec and the design plan's AC2.2.
const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// Build the metrics layer (for the main router) and the metrics side-port router.
///
/// Returns `(layer, side_router)` where:
/// - `layer` is applied to the main API router via `.layer()`
/// - `side_router` exposes `GET /metrics` in Prometheus text format with the
///   canonical `text/plain; version=0.0.4; charset=utf-8` Content-Type.
///
/// # Panics
///
/// Panics if a global metrics recorder has already been installed. Do not call
/// `PrometheusBuilder::install()` separately — axum-prometheus installs the
/// recorder internally.
pub fn build() -> (PrometheusMetricLayer<'static>, Router) {
    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    let metrics_router = Router::new().route(
        "/metrics",
        get(move || async move {
            (
                [(header::CONTENT_TYPE, PROMETHEUS_CONTENT_TYPE)],
                metric_handle.render(),
            )
        }),
    );

    (prometheus_layer, metrics_router)
}

/// Describe and set the `atc_build_info` gauge with compile-time labels.
///
/// Must be called after `build()` (which installs the global recorder).
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
///
/// Must be called after `build()` (which installs the global recorder).
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
/// - atc_pg_notify_emitted_total{kind} — emitted from webhook handler inside txn
/// - atc_pg_notify_received_total — received by listener task
/// - atc_pg_listener_recv_errors_total — recv() errors (sqlx hides successful reconnects)
/// - atc_pg_drain_passes_total — drain task wake-ups
/// - atc_pg_drain_rows_total — outbox rows fetched across all passes
///
/// Must be called after build() (which installs the global recorder).
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
}

/// Describe process metrics and spawn a background collector that ticks every
/// 10 seconds.
///
/// Must be called after `build()` (which installs the global recorder).
pub fn spawn_process_collector() {
    let collector = metrics_process::Collector::default();
    collector.describe();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            collector.collect();
        }
    });
}
