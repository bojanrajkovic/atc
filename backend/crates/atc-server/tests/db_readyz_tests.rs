//! Integration tests for the PostgreSQL-backed readyz probe and pool initialization.
//!
//! Boots ephemeral PostgreSQL containers via testcontainers, runs migrations via
//! `atc_server::db::init_pool`, and verifies GET /readyz behavior in healthy and
//! unreachable DB states. Requires Docker (or OrbStack) to be running.

use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::state::{AppState, SeqEvent};
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

fn prometheus_layer() -> PrometheusMetricLayer<'static> {
    PROMETHEUS_INIT
        .get_or_init(|| PrometheusMetricLayer::pair().0)
        .clone()
}

async fn build_app_with_pool(pool: sqlx::PgPool) -> axum::Router {
    let layer = prometheus_layer();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: Some(pool),
    });
    atc_server::routes::api_routes(layer).with_state(app_state)
}

#[tokio::test]
#[serial_test::serial]
async fn readyz_returns_ok_with_healthy_db() {
    let container = Postgres::default()
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
