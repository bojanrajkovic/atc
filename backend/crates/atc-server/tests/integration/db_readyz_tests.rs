//! Integration tests for the PostgreSQL-backed readyz probe and pool initialization.
//!
//! Boots ephemeral PostgreSQL containers via testcontainers, runs migrations via
//! `atc_server::db::init_pool`, and verifies GET /readyz behavior in healthy and
//! unreachable DB states. Requires Docker (or OrbStack) to be running.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;

use atc_server::persist::PgStore;
use atc_server::state::AppState;
use axum::body::Body;
use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::ServiceExt;

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

async fn build_app_with_pool(pool: sqlx::PgPool) -> axum::Router {
    common::ensure_recorder_installed();
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    // Use a fresh timestamp so liveness_check considers the heartbeat as recent.
    let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis_for_test()));
    let broadcast_watermark = Arc::new(AtomicI64::new(0));
    let persist = Arc::new(PgStore::new(
        pool.clone(),
        Arc::clone(&broadcast_watermark),
        Arc::clone(&last_drain_pass_at),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_tx,
        webhook_secret: None,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    atc_server::routes::api_routes().with_state(app_state)
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
