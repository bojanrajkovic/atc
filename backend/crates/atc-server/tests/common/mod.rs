#![allow(dead_code)]

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::listener;
use atc_server::state::AppState;
use axum_prometheus::PrometheusMetricLayer;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
pub static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

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
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: Some(secret.to_string()),
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

/// Build app with no webhook secret (verification bypassed)
pub fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
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

/// Boot a fresh ephemeral Postgres container and return pool + guard + URL.
///
/// The container lives until the guard (impl Drop) is dropped. The URL is
/// needed by tests that open additional connections (e.g., PgListener).
pub async fn start_pg() -> (sqlx::PgPool, impl Drop, String) {
    use testcontainers::ImageExt;
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres;

    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("failed to start postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
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
    use std::sync::atomic::Ordering;

    let layer = PROMETHEUS_INIT
        .get_or_init(|| PrometheusMetricLayer::pair().0)
        .clone();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, broadcast_rx) =
        tokio::sync::broadcast::channel::<atc_server::state::SeqEvent>(256);
    let state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: Some(pool.clone()),
    });

    let router = atc_server::routes::api_routes(layer)
        .with_state(state.clone())
        .fallback(atc_server::assets::fallback_handler());

    // Initialize watermark (same logic as main.rs) using untyped API to avoid sqlx macro caching.
    let initial_watermark: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(seq), 0) FROM outbox")
            .fetch_one(&pool)
            .await
            .expect("watermark query failed");

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
        shutdown.clone(),
        Some(observed_recv.clone()),
    );

    let drain_handle = listener::spawn_drain_task(
        pool.clone(),
        initial_watermark,
        drain_notify,
        shutdown.clone(),
        Some(observed_passes.clone()),
        Some(drain_started.clone()),
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
