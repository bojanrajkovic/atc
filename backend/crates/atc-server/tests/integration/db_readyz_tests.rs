//! Integration tests for the PostgreSQL-backed readyz probe and pool initialization.
//!
//! Boots ephemeral PostgreSQL containers via testcontainers, runs migrations via
//! `atc_store_pg::db::init_pool`, and verifies GET /readyz behavior in healthy and
//! unreachable DB states. Requires Docker (or OrbStack) to be running.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

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

async fn build_app_with_pool(
    pool: sqlx::PgPool,
    db_url: &str,
) -> (axum::Router, CancellationToken) {
    common::ensure_recorder_installed();
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool, db_url, shutdown.clone()).await;
    let persist = store as Arc<dyn atc_persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: tokio::sync::broadcast::channel(16).0,
        shutdown: shutdown.clone(),
        ws_tracker: TaskTracker::new(),
    });
    (
        atc_server::routes::api_routes().with_state(app_state),
        shutdown,
    )
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

    let pool = atc_store_pg::db::init_pool(&db_url)
        .await
        .expect("init_pool failed");

    let (app, shutdown) = build_app_with_pool(pool, &db_url).await;
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

    shutdown.cancel();
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

    let pool = atc_store_pg::db::init_pool(&db_url)
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
    atc_store_pg::db::MIGRATOR
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
    atc_store_pg::db::MIGRATOR
        .run(&pool)
        .await
        .expect("migrations failed");

    let (app, shutdown) = build_app_with_pool(pool, &db_url).await;

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

    shutdown.cancel();
}
