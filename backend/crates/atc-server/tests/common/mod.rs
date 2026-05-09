#![allow(dead_code)]

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64};
use std::time::Duration;

use atc_core::{RunStateMachine, SystemClock};
use atc_server::listener;
use atc_server::persist::{InMemoryStore, PgStore};
use atc_server::state::AppState;
use axum_prometheus::PrometheusMetricLayer;
use axum_prometheus::metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use axum_prometheus::utils::SECONDS_DURATION_BUCKETS;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Mirror of the production `metrics::build()` install path so test fixtures
/// validate the same custom-bucket configuration as production. Without this
/// mirror, tests would exercise the default-recorder path while production
/// uses the install-recorder path, masking real divergence.
///
/// All test binaries that share [`PROMETHEUS_INIT`] must use this initializer
/// (instead of `PrometheusMetricLayer::pair`) so the global recorder for the
/// binary has consistent bucket configuration regardless of which test fires
/// first.
pub fn install_test_recorder() -> (PrometheusMetricLayer<'static>, PrometheusHandle) {
    let handle = PrometheusBuilder::new()
        .set_buckets_for_metric(
            Matcher::Full("atc_pg_drain_startup_seconds".to_string()),
            &[0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
        )
        .expect("valid drain-startup bucket spec")
        .set_buckets_for_metric(
            Matcher::Suffix("_seconds".to_string()),
            SECONDS_DURATION_BUCKETS,
        )
        .expect("valid _seconds suffix bucket spec")
        .install_recorder()
        .expect("install global Prometheus recorder");

    let upkeep_handle = handle.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            interval.tick().await;
            upkeep_handle.run_upkeep();
        }
    });

    (PrometheusMetricLayer::new(), handle)
}

// Guard: install_test_recorder() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
// Stores both the layer (for routing) and the handle (for metric assertions via render_metrics()).
pub static PROMETHEUS_INIT: OnceLock<(PrometheusMetricLayer<'static>, PrometheusHandle)> =
    OnceLock::new();

/// Render current Prometheus metrics as text.
///
/// Panics if `PROMETHEUS_INIT` has not been initialized yet. Call after any
/// call to `build_app_with_secret`, `build_app_no_secret`,
/// `build_app_with_pg_and_listener`, or [`ensure_recorder_installed`].
pub fn render_metrics() -> String {
    PROMETHEUS_INIT
        .get()
        .expect("PROMETHEUS_INIT not yet initialized — call a build_app_* helper first")
        .1
        .render()
}

/// Ensure the global Prometheus recorder is installed without constructing a
/// full `TestApp`. Useful for tests that need to capture a baseline scrape
/// before they exercise the code under test.
pub fn ensure_recorder_installed() {
    PROMETHEUS_INIT.get_or_init(install_test_recorder);
}

/// Locate an unlabeled metric line in a Prometheus exposition body and return
/// the value as a string slice.
///
/// The Prometheus text format is one metric per line: `<name>[{labels}] <value>`.
/// This helper scans for a line whose name part is exactly `name` (no labels
/// allowed), skipping `# HELP`/`# TYPE` comments. Returns `None` if no such
/// line is found.
fn unlabeled_metric_value<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix(name) else {
            continue;
        };
        // The next character must be whitespace (no labels, no extra suffix).
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        if let Some(value) = line.split_whitespace().last() {
            return Some(value);
        }
    }
    None
}

/// Parse an unlabeled counter or histogram `_count` value. Returns 0 if the
/// metric is absent — convenient for delta computations against a baseline
/// scrape captured before the metric had any observations.
pub fn parse_unlabeled_counter(body: &str, name: &str) -> u64 {
    unlabeled_metric_value(body, name)
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0)
}

/// Parse an unlabeled gauge or histogram `_sum` value. Returns `None` if the
/// metric is absent so callers can distinguish "missing" from "present and
/// equal to zero". Returned `f64` may be `NaN` (gauges that emit `f64::NAN`
/// — e.g., `atc_pg_min_pending_seq` at its sentinel state — render as `NaN`
/// in the exposition body and parse back into `f64::NAN`).
pub fn parse_unlabeled_gauge(body: &str, name: &str) -> Option<f64> {
    unlabeled_metric_value(body, name).and_then(|v| v.parse::<f64>().ok())
}

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

/// Build app with a specific webhook secret
pub fn build_app_with_secret(secret: &str) -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(install_test_recorder).0.clone();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = Arc::new(InMemoryStore::new(
        state_machine.clone(),
        seq.clone(),
        webhook_tx.clone(),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: Some(secret.to_string()),
        seq,
        pg_pool: None,
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(now_millis_for_test())),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
        ws_close: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

/// Build app with no webhook secret (verification bypassed)
pub fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(install_test_recorder).0.clone();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = Arc::new(InMemoryStore::new(
        state_machine.clone(),
        seq.clone(),
        webhook_tx.clone(),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: None,
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(now_millis_for_test())),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
        ws_close: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

// Fixture: workflow_run_requested.json
pub fn fixture_workflow_run_requested() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_requested.json").to_vec()
}

// Fixture: workflow_job_queued.json
pub fn fixture_workflow_job_queued() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_job_queued.json").to_vec()
}

// Fixture: workflow_run_completed.json
pub fn fixture_workflow_run_completed() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_completed.json").to_vec()
}

// Fixture: workflow_run_in_progress.json
pub fn fixture_workflow_run_in_progress() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_in_progress.json").to_vec()
}

// Fixture: workflow_job_in_progress.json
pub fn fixture_workflow_job_in_progress() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_job_in_progress.json").to_vec()
}

// Fixture: workflow_job_completed.json
pub fn fixture_workflow_job_completed() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_job_completed.json").to_vec()
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

/// Full-stack test fixture with a real PG pool and running listener+drain tasks.
pub struct AppFixture {
    pub pool: sqlx::PgPool,
    pub router: axum::Router,
    pub state: Arc<atc_server::state::AppState>,
    pub broadcast_rx: tokio::sync::broadcast::Receiver<atc_server::state::SeqEvent>,
    pub listener_handle: JoinHandle<()>,
    pub drain_handle: JoinHandle<()>,
    pub observed_recv: Arc<AtomicU64>,
    pub observed_passes: Arc<AtomicU64>,
    pub drain_started: Arc<tokio::sync::Notify>,
    pub shutdown: CancellationToken,
    pub db_url: String,
}

/// Build a full fixture with PG pool, listener task, and drain task.
///
/// Waits for the first drain pass to complete (drain_started fires once)
/// before returning — this guarantees the watermark is initialized and
/// the first unconditional pass has run, so tests can capture a stable
/// baseline.
pub async fn build_app_with_pg_and_listener(pool: sqlx::PgPool, db_url: String) -> AppFixture {
    build_app_inner(pool, db_url, None).await
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
    build_app_inner(pool, db_url, Some(drain_delay)).await
}

/// Shared implementation for both fixture builders.
async fn build_app_inner(
    pool: sqlx::PgPool,
    db_url: String,
    drain_delay: Option<Duration>,
) -> AppFixture {
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::Ordering;
    use std::time::Instant;

    let layer = PROMETHEUS_INIT.get_or_init(install_test_recorder).0.clone();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, broadcast_rx) =
        tokio::sync::broadcast::channel::<atc_server::state::SeqEvent>(256);
    let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));
    let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis_for_test()));
    // Mirror of main.rs: drain_in_flight bracket for wake-coalesce
    // observation, and startup_at captured before the watermark query.
    let drain_in_flight = Arc::new(AtomicBool::new(false));
    let startup_at = Instant::now();

    // Initialize watermark (same logic as main.rs) using untyped API to avoid sqlx macro caching.
    let initial_watermark: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(seq), 0) FROM outbox")
            .fetch_one(&pool)
            .await
            .expect("watermark query failed");

    // Seed broadcast_watermark from initial_watermark so /v1/state returns a
    // sensible lastSeq before the first post-startup drain pass completes.
    let broadcast_watermark = Arc::new(AtomicI64::new(initial_watermark));
    #[allow(clippy::cast_precision_loss)]
    ::metrics::gauge!("atc_pg_broadcast_watermark").set(initial_watermark as f64);
    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist =
        Arc::new(PgStore::new(pool.clone())) as Arc<dyn atc_server::persist::PersistentStore>;
    let state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: Some(pool.clone()),
        min_pending_seq: min_pending_seq.clone(),
        last_drain_pass_at: last_drain_pass_at.clone(),
        broadcast_watermark: broadcast_watermark.clone(),
        persist,
        ws_close: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });

    let router = atc_server::routes::api_routes(layer)
        .with_state(state.clone())
        .fallback(atc_server::assets::fallback_handler());

    // Connect the PgListener for the listener task.
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener failed");

    let observed_recv = Arc::new(AtomicU64::new(0));
    let observed_passes = Arc::new(AtomicU64::new(0));
    let drain_started = Arc::new(tokio::sync::Notify::new());
    let shutdown = CancellationToken::new();
    let drain_notify = Arc::new(tokio::sync::Notify::new());

    let listener_handle = listener::spawn_listener_task(
        pg_listener,
        drain_notify.clone(),
        min_pending_seq.clone(),
        drain_in_flight.clone(),
        shutdown.clone(),
        Some(observed_recv.clone()),
    );

    let drain_handle = listener::spawn_drain_task(
        pool.clone(),
        initial_watermark,
        startup_at,
        drain_notify,
        min_pending_seq,
        last_drain_pass_at,
        broadcast_watermark,
        drain_in_flight,
        state.webhook_tx.clone(),
        shutdown.clone(),
        Some(observed_passes.clone()),
        Some(drain_started.clone()),
        drain_delay,
    );

    // Wait for the first drain pass to complete so the fixture is stable.
    tokio::time::timeout(Duration::from_secs(5), drain_started.notified())
        .await
        .expect("drain task did not start within 5s");

    // Suppress unused import warning in non-test builds.
    let _ = Ordering::Relaxed;

    AppFixture {
        pool,
        router,
        state,
        broadcast_rx,
        listener_handle,
        drain_handle,
        observed_recv,
        observed_passes,
        drain_started,
        shutdown,
        db_url,
    }
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
