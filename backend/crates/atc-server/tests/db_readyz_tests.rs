//! Integration tests for the PostgreSQL-backed readyz probe and pool initialization.
//!
//! Boots ephemeral PostgreSQL containers via testcontainers, runs migrations via
//! `atc_server::db::init_pool`, and verifies GET /readyz behavior in healthy and
//! unreachable DB states. Requires Docker (or OrbStack) to be running.

mod common;

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;

use atc_core::{RunStateMachine, SystemClock};
use atc_server::state::{AppState, SeqEvent};
use axum_prometheus::PrometheusMetricLayer;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

fn prometheus_layer() -> PrometheusMetricLayer<'static> {
    common::PROMETHEUS_INIT
        .get_or_init(common::install_test_recorder)
        .0
        .clone()
}

async fn build_app_with_pool(pool: sqlx::PgPool) -> axum::Router {
    let layer = prometheus_layer();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = Arc::new(atc_server::persist::PgStore::new(pool.clone()))
        as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: Some(pool),
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(now_millis_for_test())),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    atc_server::routes::api_routes(layer).with_state(app_state)
}

#[tokio::test]
#[serial_test::serial]
async fn readyz_returns_ok_with_healthy_db() {
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

    let app = build_app_with_pool(pool).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
#[serial_test::serial]
async fn migrations_create_runs_and_jobs_tables() {
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

    // Verify tables exist by querying them (empty result is fine; error = missing table)
    sqlx::query("SELECT 1 FROM runs LIMIT 0")
        .execute(&pool)
        .await
        .expect("runs table missing or query failed");
    sqlx::query("SELECT 1 FROM jobs LIMIT 0")
        .execute(&pool)
        .await
        .expect("jobs table missing or query failed");

    // Verify migration is idempotent
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("second migration run failed");
}

#[tokio::test]
#[serial_test::serial]
async fn readyz_returns_503_when_db_unreachable() {
    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("failed to start postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");
    let db_url =
        format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres?connect_timeout=2");

    let pool = sqlx::postgres::PgPoolOptions::new()
        .acquire_timeout(Duration::from_secs(2))
        .connect(&db_url)
        .await
        .expect("failed to connect to PG");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrations failed");

    let app = build_app_with_pool(pool).await;

    // Stop the container to make the DB unreachable
    container.stop().await.expect("failed to stop container");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["status"], "db_unreachable");
}
