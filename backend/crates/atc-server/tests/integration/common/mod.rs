#![allow(dead_code)]

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64};
use std::time::Duration;

use atc_core::SystemClock;
use atc_persist::PersistentStore;
use atc_server::listener;
use atc_server::otel::exponential_histogram_view;
use atc_server::persist::PgStore;
use atc_server::persist::pg::PgStoreTestHooks;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::metrics::data::{
    AggregatedMetrics, MetricData, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_sdk::metrics::{
    InMemoryMetricExporter, InMemoryMetricExporterBuilder, PeriodicReader, SdkMeterProvider,
    Temporality,
};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};
use tokio::task::AbortHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

// ---------------------------------------------------------------------------
// OTel test harness — single shared install per test binary
// ---------------------------------------------------------------------------

/// Shared in-memory OTel pipeline for the test binary.
///
/// One install per process: the OTel global tracer/meter providers are
/// process-singletons. Tests that read snapshots must be marked
/// `#[serial_test::serial]` because the buffers are shared across every test.
///
/// Histograms use the same exponential aggregation view as production (via
/// `atc_server::otel::exponential_histogram_view`) so tests observe the same
/// data shape as deployed pods.
pub struct OtelTestHarness {
    pub span_exporter: InMemorySpanExporter,
    pub metric_exporter: InMemoryMetricExporter,
    pub meter_provider: SdkMeterProvider,
    pub tracer_provider: SdkTracerProvider,
}

static OTEL_TEST_INIT: OnceLock<OtelTestHarness> = OnceLock::new();

/// Idempotent installer. First caller installs; subsequent callers receive the
/// existing harness. Safe to call from any test that emits metrics or spans.
pub fn ensure_recorder_installed() -> &'static OtelTestHarness {
    OTEL_TEST_INIT.get_or_init(install_test_otel)
}

fn install_test_otel() -> OtelTestHarness {
    // Spans: simple processor exports synchronously on `on_end` so finished
    // spans are visible immediately. The InMemoryMetricExporter requires a
    // PeriodicReader (it's a PushMetricExporter); we trigger collection in
    // tests via `meter_provider.force_flush()`.
    let span_exporter = InMemorySpanExporter::default();
    let tracer_provider = SdkTracerProvider::builder()
        .with_span_processor(SimpleSpanProcessor::new(span_exporter.clone()))
        .build();

    // Delta temporality so each test's `force_flush()` reports only the
    // emissions made since the last flush. Cumulative would carry every test's
    // emissions forward, defeating per-test reset semantics.
    let metric_exporter = InMemoryMetricExporterBuilder::new()
        .with_temporality(Temporality::Delta)
        .build();
    let reader = PeriodicReader::builder(metric_exporter.clone()).build();
    let meter_provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_view(exponential_histogram_view)
        .build();

    // The tracing-opentelemetry layer captures a tracer from this provider so
    // `tracing::info_span!` / `#[tracing::instrument]` route into the in-memory
    // span exporter. axum-otel-metrics reads from the global meter provider at
    // layer-build time, so the global meter provider must be set before the
    // first call to `routes::api_routes()` in any test.
    let tracer = tracer_provider.tracer("atc-test");
    opentelemetry::global::set_tracer_provider(tracer_provider.clone());
    opentelemetry::global::set_meter_provider(meter_provider.clone());
    opentelemetry::global::set_text_map_propagator(TraceContextPropagator::new());

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let _ = tracing_subscriber::registry()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .try_init();

    // `register_build_info` is the only metric the harness has to register
    // eagerly — `PgMetrics` instruments are constructed transitively by
    // `PgStore::start` when a test builds a store. Tests that never build a
    // `PgStore` (the in-memory routing tests) emit no `atc_pg_*` metrics and
    // don't need the PgMetrics instruments to exist.
    atc_server::metrics::register_build_info();

    OtelTestHarness {
        span_exporter,
        metric_exporter,
        meter_provider,
        tracer_provider,
    }
}

// ---------------------------------------------------------------------------
// Snapshot accessors
// ---------------------------------------------------------------------------

/// Drain the meter provider and return every `ResourceMetrics` batch produced
/// since the previous snapshot or `reset_metrics()` call.
///
/// This call drains the exporter buffer after reading it, so the next
/// `snapshot_metrics()` reflects only deltas produced after this call —
/// matching the natural per-test isolation pattern. To start from a clean
/// slate without consuming a snapshot, call `reset_metrics()`.
pub fn snapshot_metrics() -> Vec<ResourceMetrics> {
    let h = ensure_recorder_installed();
    h.meter_provider
        .force_flush()
        .expect("meter_provider.force_flush()");
    let snapshot = h
        .metric_exporter
        .get_finished_metrics()
        .expect("InMemoryMetricExporter::get_finished_metrics");
    h.metric_exporter.reset();
    snapshot
}

/// Snapshot of every span exported so far in the current test binary.
pub fn read_finished_spans() -> Vec<opentelemetry_sdk::trace::SpanData> {
    let h = ensure_recorder_installed();
    h.span_exporter
        .get_finished_spans()
        .expect("InMemorySpanExporter::get_finished_spans")
}

/// Clear the in-memory metric buffer AND advance the SDK's
/// last-flush watermark so subsequent snapshots only report observations made
/// after this call.
///
/// With `Temporality::Delta`, the SDK reports the delta since the last
/// `force_flush()`. Just clearing the exporter buffer would leave prior
/// observations in the SDK accumulator; the next `snapshot_metrics()` would
/// then report them as new. We force a flush first to advance the watermark,
/// then clear the buffer so the cumulative state since process start is
/// dropped.
pub fn reset_metrics() {
    let h = ensure_recorder_installed();
    let _ = h.meter_provider.force_flush();
    h.metric_exporter.reset();
}

/// Clear the in-memory span buffer.
pub fn reset_spans() {
    let h = ensure_recorder_installed();
    h.span_exporter.reset();
}

/// Backwards-compatible alias for tests that still use the span-only helper.
pub fn ensure_span_exporter_installed() -> InMemorySpanExporter {
    ensure_recorder_installed().span_exporter.clone()
}

// ---------------------------------------------------------------------------
// Typed lookup helpers over `ResourceMetrics` snapshots
// ---------------------------------------------------------------------------

fn attrs_match(actual: &[KeyValue], expected: &[KeyValue]) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    expected.iter().all(|e| {
        actual
            .iter()
            .any(|a| a.key == e.key && a.value.as_str() == e.value.as_str())
    })
}

fn for_each_metric<'a>(
    snapshot: &'a [ResourceMetrics],
    name: &str,
    mut f: impl FnMut(&'a AggregatedMetrics),
) {
    for resource in snapshot {
        for scope in resource.scope_metrics() {
            scope_metrics_for_name(scope, name, &mut f);
        }
    }
}

fn scope_metrics_for_name<'a>(
    scope: &'a ScopeMetrics,
    name: &str,
    f: &mut impl FnMut(&'a AggregatedMetrics),
) {
    for metric in scope.metrics() {
        if metric.name() == name {
            f(metric.data());
        }
    }
}

/// Counter value for `name` with the given attribute set, summed across every
/// `ResourceMetrics` batch in the snapshot. Returns 0 when absent so deltas
/// against a fresh `reset_metrics()` baseline are well-defined.
pub fn counter_value(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> u64 {
    let mut total: u64 = 0;
    for_each_metric(snapshot, name, |data| {
        if let AggregatedMetrics::U64(MetricData::Sum(sum)) = data {
            for dp in sum.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    total = total.saturating_add(dp.value());
                }
            }
        }
    });
    total
}

/// Gauge value for `name`. Returns the most recent observation across batches
/// (the SDK reports one observation per collection cycle for an
/// `ObservableGauge`). `Some(NaN)` when the gauge was set to NaN; `None` when
/// the gauge was never recorded.
pub fn gauge_value(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> Option<f64> {
    let mut last: Option<f64> = None;
    for_each_metric(snapshot, name, |data| {
        if let AggregatedMetrics::F64(MetricData::Gauge(gauge)) = data {
            for dp in gauge.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    last = Some(dp.value());
                }
            }
        }
    });
    last
}

/// Histogram observation count for `name` summed across batches. Returns 0
/// when absent.
pub fn histogram_count(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> u64 {
    let mut total: u64 = 0;
    for_each_metric(snapshot, name, |data| {
        if let AggregatedMetrics::F64(MetricData::ExponentialHistogram(hist)) = data {
            for dp in hist.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    total = total.saturating_add(dp.count() as u64);
                }
            }
        } else if let AggregatedMetrics::F64(MetricData::Histogram(hist)) = data {
            for dp in hist.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    total = total.saturating_add(dp.count());
                }
            }
        }
    });
    total
}

/// Histogram sum-of-observations for `name` across batches. Returns 0.0 when
/// absent.
pub fn histogram_sum(snapshot: &[ResourceMetrics], name: &str, attrs: &[KeyValue]) -> f64 {
    let mut total: f64 = 0.0;
    for_each_metric(snapshot, name, |data| {
        if let AggregatedMetrics::F64(MetricData::ExponentialHistogram(hist)) = data {
            for dp in hist.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    total += dp.sum();
                }
            }
        } else if let AggregatedMetrics::F64(MetricData::Histogram(hist)) = data {
            for dp in hist.data_points() {
                let dp_attrs: Vec<KeyValue> = dp.attributes().cloned().collect();
                if attrs_match(&dp_attrs, attrs) {
                    total += dp.sum();
                }
            }
        }
    });
    total
}

/// Whether any data point exists for `name` in the snapshot, regardless of
/// kind. Useful for "the metric was registered and emitted" assertions
/// without committing to a specific aggregation type.
pub fn metric_present(snapshot: &[ResourceMetrics], name: &str) -> bool {
    let mut found = false;
    for_each_metric(snapshot, name, |_| found = true);
    found
}

// ---------------------------------------------------------------------------
// HTTP webhook signature helper (unchanged from the prior recorder)
// ---------------------------------------------------------------------------

/// Compute HMAC-SHA256 signature in the format GitHub expects: sha256=<hex>
pub fn compute_signature(secret: &[u8], body: &[u8]) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).unwrap();
    mac.update(body);
    let digest = mac.finalize();
    format!("sha256={}", const_hex::encode(digest.into_bytes()))
}

/// Build app with a specific webhook secret (in-memory mode).
///
/// Uses `InMemoryStore::new_for_test`, which constructs the store without
/// spawning the eviction task. These helpers back tests that cover routing,
/// auth, WS, and ingestion — not TTL eviction — so the eviction task would
/// be dead weight here, and the shared-binary test environment means a
/// leaked detached task would accumulate across tests until process exit.
/// Tests that explicitly exercise eviction (`in_memory_store_tests.rs`)
/// use their own helpers.
pub fn build_app_with_secret(secret: &str) -> (axum::Router, Arc<AppState>) {
    ensure_recorder_installed();
    let persist = InMemoryStore::new_for_test(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        IN_MEMORY_TEST_BROADCAST_CAPACITY,
    ) as Arc<dyn atc_persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: Some(secret.to_string()),
        runner_pool_capacities: Vec::new(),
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    let app = atc_server::routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

/// Build app with no webhook secret (verification bypassed, in-memory mode).
///
/// Same `new_for_test` shape as [`build_app_with_secret`]; see that doc for
/// why eviction isn't spawned here.
pub fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
    ensure_recorder_installed();
    let persist = InMemoryStore::new_for_test(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        IN_MEMORY_TEST_BROADCAST_CAPACITY,
    ) as Arc<dyn atc_persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: Vec::new(),
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    let app = atc_server::routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

/// Broadcast capacity used by the shared in-memory app builders. Matches the
/// production capacity (`InMemoryStore::start` constant) so `RecvError::Lagged`
/// semantics in the shared helpers mirror production. Lagging-client coverage
/// uses a smaller capacity via `InMemoryStore::new_for_test` directly.
const IN_MEMORY_TEST_BROADCAST_CAPACITY: usize = 256;

// Fixture: workflow_run_requested.json
pub fn fixture_workflow_run_requested() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_run_requested.json").to_vec()
}

// Fixture: workflow_job_queued.json
pub fn fixture_workflow_job_queued() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_job_queued.json").to_vec()
}

// Fixture: workflow_run_completed.json
pub fn fixture_workflow_run_completed() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_run_completed.json").to_vec()
}

// Fixture: workflow_run_in_progress.json
pub fn fixture_workflow_run_in_progress() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_run_in_progress.json").to_vec()
}

// Fixture: workflow_job_in_progress.json
pub fn fixture_workflow_job_in_progress() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_job_in_progress.json").to_vec()
}

// Fixture: workflow_job_completed.json
pub fn fixture_workflow_job_completed() -> Vec<u8> {
    include_bytes!("../../../../atc-github/tests/fixtures/workflow_job_completed.json").to_vec()
}

// ---------------------------------------------------------------------------
// Ephemeral PG container helpers
// ---------------------------------------------------------------------------

/// Boot (or reuse) a Postgres container and return pool + guard + URL.
///
/// The container is shared across nextest test processes via testcontainers'
/// `ReuseDirective::Always` (named `atc-test-pg`). Each test gets its own
/// freshly-created database within the shared container — `CREATE DATABASE
/// test_<nanos>_<counter>` — so tests stay isolated despite the shared
/// container. Migrations run on each per-test DB.
///
/// The container persists after `cargo nextest run` finishes; clean up with
/// `docker rm -f atc-test-pg` (or wait for OrbStack/Docker GC). Per-test
/// databases accumulate inside the container but are tiny; if they pile up
/// beyond comfort, drop the container.
pub async fn start_pg() -> (sqlx::PgPool, impl Drop, String) {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    use testcontainers::ImageExt;
    use testcontainers::ReuseDirective;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    static DB_COUNTER: AtomicU64 = AtomicU64::new(0);

    // Retry on container-creation race: testcontainers' reuse logic
    // (inspect-then-create) is not atomic, so concurrent test processes
    // can both pass the inspect, then one wins `docker create` while the
    // others get a 409 Conflict. On retry, the existence check passes
    // and we attach to the now-created container.
    let mut container_delay_ms: u64 = 50;
    let container = loop {
        match Postgres::default()
            .with_tag("17-alpine")
            .with_container_name("atc-test-pg")
            .with_reuse(ReuseDirective::Always)
            .start()
            .await
        {
            Ok(c) => break c,
            Err(e) if container_delay_ms < 4_000 => {
                tokio::time::sleep(Duration::from_millis(container_delay_ms)).await;
                container_delay_ms *= 2;
                eprintln!(
                    "[start_pg] container start retry after {container_delay_ms}ms (last error: {e})"
                );
            }
            Err(e) => panic!("failed to start postgres container after retries: {e}"),
        }
    };
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");

    // Open a single admin connection (NOT a pool — pools default to 10
    // connections, and N parallel tests × 10 connections each blows past
    // Postgres' default max_connections=100) just long enough to issue
    // `CREATE DATABASE`, then drop it. The test's own pool (returned
    // below) connects to the new DB.
    //
    // Retries: with `ReuseDirective::Always`, the *first* test process
    // that reaches the container creation race wins; concurrent siblings
    // see the container exists but Postgres inside may still be
    // starting up. The retry loop with exponential backoff absorbs
    // "Connection reset by peer" and "database system is starting up"
    // errors during that startup window (typically <1s).
    use sqlx::Connection;
    let admin_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    // Cross-process DB-name uniqueness, not testability. We need the live
    // wall-clock nanos here precisely so each test process picks a different
    // suffix; routing through a `TestClock` would defeat the purpose.
    #[allow(clippy::disallowed_methods)]
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let counter = DB_COUNTER.fetch_add(1, Ordering::Relaxed);
    // PID guarantees uniqueness across nextest's parallel test processes
    // even when two processes happen to call this fn at the same nanosecond
    // with the same process-local counter value.
    let pid = std::process::id();
    let db_name = format!("test_{pid}_{nanos}_{counter}");
    let mut delay_ms: u64 = 50;
    let admin_conn = loop {
        match sqlx::PgConnection::connect(&admin_url).await {
            Ok(conn) => break conn,
            Err(e) if delay_ms < 4_000 => {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms *= 2;
                eprintln!("[start_pg] admin connect retry after {delay_ms}ms (last error: {e})");
            }
            Err(e) => panic!("admin connect failed after retries: {e}"),
        }
    };
    {
        let mut admin_conn = admin_conn;
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\""))
            .execute(&mut admin_conn)
            .await
            .expect("CREATE DATABASE failed");
    }

    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/{db_name}");
    let pool = atc_server::db::init_pool(&db_url)
        .await
        .expect("init_pool failed");
    (pool, container, db_url)
}

// ---------------------------------------------------------------------------
// Full-stack fixture with listener + drain tasks
// ---------------------------------------------------------------------------

/// Full-stack test fixture with a real PG pool and a `PgStore` whose listener
/// and drain tasks are owned by the store itself. The fixture exposes abort
/// handles (extracted before the store stored the `JoinHandle`s) plus the
/// watermark and heartbeat atomics for tests that poll them.
pub struct AppFixture {
    pub pool: sqlx::PgPool,
    pub router: axum::Router,
    pub state: Arc<atc_server::state::AppState>,
    pub broadcast_rx: tokio::sync::broadcast::Receiver<atc_wire::CommittedEvent>,
    pub observed_recv: Arc<AtomicU64>,
    pub observed_passes: Arc<AtomicU64>,
    pub drain_started: Arc<tokio::sync::Notify>,
    pub shutdown: CancellationToken,
    pub db_url: String,
    /// Heartbeat atomic owned by the store. Tests that need to manipulate or
    /// read the drain staleness timestamp access it here rather than through
    /// AppState (which never held it).
    pub last_drain_pass_at: Arc<AtomicI64>,
    /// Commit-order cursor advanced by the drain after each successful pass.
    /// Tests that need to poll or read the broadcast watermark (e.g. `state_pg_read`)
    /// access it here rather than through AppState (which never held it).
    pub broadcast_watermark: Arc<AtomicI64>,
    /// Abort handles for the store's listener and drain tasks. Tests use these
    /// to simulate task death (e.g. `readyz` stale-drain coverage); calling
    /// `abort()` consumes neither the store nor the join capability, which the
    /// store retains and joins via `persist.shutdown()`.
    pub drain_abort: AbortHandle,
    pub listener_abort: AbortHandle,
}

/// Build a full fixture with PG pool, listener task, and drain task.
///
/// Waits for the first drain pass to complete (drain_started fires once)
/// before returning — this guarantees the watermark is initialized and
/// the first unconditional pass has run, so tests can capture a stable
/// baseline.
pub async fn build_app_with_pg_and_listener(pool: sqlx::PgPool, db_url: String) -> AppFixture {
    build_app_inner(Arc::new(SystemClock), pool, db_url, None).await
}

/// Build a full fixture identical to [`build_app_with_pg_and_listener`] but
/// with an artificial per-pass sleep injected into the drain task.
///
/// Passing a `drain_delay` makes each drain pass sleep for that duration before
/// querying the outbox, ensuring that NOTIFYs fired during an in-flight pass
/// arrive while the drain is still sleeping. This forces coalescing to be
/// observable in the coalescing test.
pub async fn build_app_with_pg_and_slow_drain(
    pool: sqlx::PgPool,
    db_url: String,
    drain_delay: Duration,
) -> AppFixture {
    build_app_inner(Arc::new(SystemClock), pool, db_url, Some(drain_delay)).await
}

/// Build a full fixture wired to a caller-supplied `Clock`. Tests that need to
/// advance time deterministically (e.g. `liveness_check` staleness, outbox-lag
/// observation) use this entry point with a `TestClock`.
pub async fn build_app_with_pg_clock(
    clock: Arc<dyn atc_core::Clock>,
    pool: sqlx::PgPool,
    db_url: String,
) -> AppFixture {
    build_app_inner(clock, pool, db_url, None).await
}

/// Shared implementation for both fixture builders.
async fn build_app_inner(
    clock: Arc<dyn atc_core::Clock>,
    pool: sqlx::PgPool,
    db_url: String,
    drain_delay: Option<Duration>,
) -> AppFixture {
    ensure_recorder_installed();

    // Connect the PgListener for the store's listener task.
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener failed");

    let observed_recv = Arc::new(AtomicU64::new(0));
    let observed_passes = Arc::new(AtomicU64::new(0));
    let drain_started = Arc::new(tokio::sync::Notify::new());
    let shutdown = CancellationToken::new();

    let hooks = PgStoreTestHooks {
        received_counter: Some(Arc::clone(&observed_recv)),
        observed_passes: Some(Arc::clone(&observed_passes)),
        drain_started: Some(Arc::clone(&drain_started)),
        drain_delay,
    };

    let (pg_store, handles) = PgStore::start_with_test_hooks(
        clock,
        pool.clone(),
        pg_listener,
        shutdown.clone(),
        Duration::from_secs(7 * 24 * 60 * 60),
        hooks,
    )
    .await
    .expect("PgStore::start_with_test_hooks");

    let broadcast_rx = pg_store.subscribe();
    let persist = pg_store as Arc<dyn atc_persist::PersistentStore>;

    let state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: Vec::new(),
        shutdown: shutdown.clone(),
        ws_tracker: TaskTracker::new(),
    });

    let router = atc_server::routes::api_routes()
        .with_state(state.clone())
        .fallback(atc_server::assets::fallback_handler());

    // Wait for the first drain pass to complete so the fixture is stable.
    tokio::time::timeout(Duration::from_secs(5), drain_started.notified())
        .await
        .expect("drain task did not start within 5s");

    AppFixture {
        pool,
        router,
        state,
        broadcast_rx,
        observed_recv,
        observed_passes,
        drain_started,
        shutdown,
        db_url,
        last_drain_pass_at: handles.last_drain_pass_at,
        broadcast_watermark: handles.broadcast_watermark,
        drain_abort: handles.drain_abort,
        listener_abort: handles.listener_abort,
    }
}

// ---------------------------------------------------------------------------
// Bare PgStore test fixture (no Axum, no AppState)
// ---------------------------------------------------------------------------

/// Boot a `PgStore` directly for tests that only exercise `apply_*_event` /
/// `read_snapshot`. Spawns the listener + drain tasks against the test DB so
/// the store is realistic; tests that need to peek at watermark or abort the
/// drain should use the full `AppFixture` instead.
///
/// The caller owns the cancellation token. Tests MUST call
/// `shutdown.cancel()` at end-of-test so the listener and drain exit before
/// the test process moves on; otherwise these tasks linger inside the
/// shared nextest integration-test binary, accumulating DB connections
/// across the run. `cancel()` alone is sufficient — the tasks observe
/// cancellation at their next `select!` boundary and exit within
/// milliseconds. Tests that want to synchronously confirm the join can
/// additionally `store.shutdown().await` afterward.
pub async fn start_pg_store_for_test(
    pool: sqlx::PgPool,
    db_url: &str,
    shutdown: CancellationToken,
) -> Arc<atc_server::persist::PgStore> {
    start_pg_store_for_test_with_clock(Arc::new(SystemClock), pool, db_url, shutdown).await
}

/// Like [`start_pg_store_for_test`] but with a caller-supplied clock for tests
/// that need to drive heartbeat / outbox-lag observations deterministically.
pub async fn start_pg_store_for_test_with_clock(
    clock: Arc<dyn atc_core::Clock>,
    pool: sqlx::PgPool,
    db_url: &str,
    shutdown: CancellationToken,
) -> Arc<atc_server::persist::PgStore> {
    start_pg_store_for_test_with_clock_and_retention(
        clock,
        pool,
        db_url,
        shutdown,
        Duration::from_secs(7 * 24 * 60 * 60),
    )
    .await
}

/// Like [`start_pg_store_for_test_with_clock`] but with a caller-supplied
/// outbox retention. Used by retention-floor and sweep tests that need to
/// drive the retention path with a non-default value.
pub async fn start_pg_store_for_test_with_clock_and_retention(
    clock: Arc<dyn atc_core::Clock>,
    pool: sqlx::PgPool,
    db_url: &str,
    shutdown: CancellationToken,
    outbox_retention: Duration,
) -> Arc<atc_server::persist::PgStore> {
    let pg_listener = listener::connect_listener(db_url)
        .await
        .expect("connect_listener failed");
    let (store, _handles) = atc_server::persist::PgStore::start_with_test_hooks(
        clock,
        pool,
        pg_listener,
        shutdown,
        outbox_retention,
        atc_server::persist::pg::PgStoreTestHooks::default(),
    )
    .await
    .expect("PgStore::start_with_test_hooks");
    store
}

// ---------------------------------------------------------------------------
// Webhook posting helper
// ---------------------------------------------------------------------------

/// POST a webhook through the given router and return (status, json_body).
pub async fn post_webhook_to_router(
    router: axum::Router,
    event_type: &str,
    body: &[u8],
) -> (axum::http::StatusCode, serde_json::Value) {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt;

    let req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", event_type)
        .body(Body::from(body.to_vec()))
        .unwrap();
    let resp = router.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}
