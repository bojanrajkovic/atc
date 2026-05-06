//! Phase 3c integration tests: /readyz drain heartbeat.
//!
//! T8 — Stale heartbeat returns 503: when `last_drain_pass_at` is older than
//!      30 seconds (READYZ_HEARTBEAT_STALENESS_MS), /readyz returns 503 with
//!      `{"status":"drain_stale"}`.
//! T9 — Fresh heartbeat returns 200: when the drain task is running and
//!      last_drain_pass_at is recent, /readyz returns 200 with `{"status":"ok"}`.
//!
//! T8 does NOT require Docker — it manipulates the atomic directly.
//! T9 requires Docker for the real PG pool (needed for the DB check to pass).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum_prometheus::PrometheusMetricLayer;
use serial_test::serial;
use tower::ServiceExt;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ─── T8: stale heartbeat → 503 ──────────────────────────────────────────────

/// T8: When pg_pool is Some and last_drain_pass_at is older than 30 seconds,
///     GET /readyz returns 503 with `{"status":"drain_stale"}`.
///
/// Uses a testcontainers PG instance (so the DB check passes), but sets
/// `last_drain_pass_at` to an artificially stale timestamp without a real
/// drain task running.
#[tokio::test]
#[serial]
async fn phase_3c_readyz_t8_stale_heartbeat_returns_503() {
    let (pool, _container, _db_url) = common::start_pg().await;

    let layer = common::PROMETHEUS_INIT
        .get_or_init(PrometheusMetricLayer::pair)
        .0
        .clone();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Set last_drain_pass_at to 60 seconds ago (well past the 30 s threshold).
    let stale_time = now_millis() - 60_000;

    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: Some(pool),
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(stale_time)),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
    });

    let app = atc_server::routes::api_routes(layer)
        .with_state(app_state)
        .fallback(atc_server::assets::fallback_handler());

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "stale heartbeat must cause 503"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["status"], "drain_stale",
        "body should indicate drain_stale"
    );
}

/// T8b: When pg_pool is None, /readyz always returns 200 regardless of
///      last_drain_pass_at (the drain heartbeat check only applies in PG mode).
#[tokio::test]
#[serial]
async fn phase_3c_readyz_t8b_no_pg_always_200() {
    let layer = common::PROMETHEUS_INIT
        .get_or_init(PrometheusMetricLayer::pair)
        .0
        .clone();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Set last_drain_pass_at to a very stale value — should not matter without PG.
    let stale_time = 0i64; // epoch = maximally stale

    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None, // no PG
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(stale_time)),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
    });

    let app = atc_server::routes::api_routes(layer)
        .with_state(app_state)
        .fallback(atc_server::assets::fallback_handler());

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "without PG pool, readyz must always return 200"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok");
}

// ─── T9: fresh heartbeat → 200 ──────────────────────────────────────────────

/// T9: When the drain task is running and healthy, GET /readyz returns 200.
///
/// Uses `build_app_with_pg_and_listener` which starts a real drain task.
/// After the fixture initializes (drain_started fires), the heartbeat should
/// be fresh. Assert /readyz returns 200.
#[tokio::test]
#[serial]
async fn phase_3c_readyz_t9_fresh_heartbeat_returns_200() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // The drain task has run its first pass (drain_started fired in
    // build_app_with_pg_and_listener). The heartbeat is fresh.
    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = fixture.router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "running drain task with fresh heartbeat must give 200"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok", "body should be ok");

    // Also assert last_drain_pass_at is indeed recent (within 10 seconds).
    let last = fixture.state.last_drain_pass_at.load(Ordering::Relaxed);
    let age_ms = now_millis().saturating_sub(last);
    assert!(
        age_ms < 10_000,
        "last_drain_pass_at should be recent; age_ms = {age_ms}"
    );

    fixture.shutdown.cancel();
}

/// T9b: The drain heartbeat ticks even during quiet periods (no events).
///
/// Sleeps past the 5 s HEARTBEAT_TICK and asserts the heartbeat has been
/// refreshed — proving the tick fires even when no NOTIFY arrives.
#[tokio::test]
#[serial]
async fn phase_3c_readyz_t9b_heartbeat_ticks_during_quiet_period() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Capture timestamp before the quiet tick.
    let before_ms = now_millis();

    // Wait 6 s (> HEARTBEAT_TICK = 5 s) during which no events are fired.
    tokio::time::sleep(Duration::from_secs(6)).await;

    // The heartbeat should have been refreshed after the tick.
    let last = fixture.state.last_drain_pass_at.load(Ordering::Relaxed);

    assert!(
        last >= before_ms,
        "last_drain_pass_at must have advanced after a heartbeat tick; \
         before={before_ms}, last={last}"
    );

    // /readyz should still return 200.
    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();
    let resp = fixture.router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "readyz must be OK after tick"
    );

    fixture.shutdown.cancel();
}
