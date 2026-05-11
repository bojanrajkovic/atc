//! Integration tests: /readyz drain heartbeat.
//!
//! Covers the drain-staleness gate on /readyz: when `last_drain_pass_at` is
//! older than 30 seconds (`READYZ_HEARTBEAT_STALENESS_MS`), /readyz returns 503
//! with `{"status":"drain_stale"}`; when the drain is running and the
//! heartbeat is recent, /readyz returns 200 with `{"status":"ok"}`. Also
//! covers the no-PG fallback (always 200) and a real drain stall via task
//! abort.
//!
//! Stale-heartbeat tests do NOT require Docker — they manipulate the atomic
//! directly. Fresh-heartbeat tests require Docker for the real PG pool
//! (needed for the DB check to pass).

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use atc_server::persist::PgStore;
use atc_server::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serial_test::serial;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::ServiceExt;

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

// ─── stale heartbeat → 503 ────────────────────────────────────────────────

/// When PgStore is configured and last_drain_pass_at is older than 30 seconds,
///     GET /readyz returns 503 with `{"status":"drain_stale"}`.
///
/// Uses a testcontainers PG instance (so the DB check passes), but sets
/// `last_drain_pass_at` to an artificially stale timestamp without a real
/// drain task running.
#[tokio::test]
#[serial]
async fn stale_heartbeat_returns_503() {
    let (pool, _container, _db_url) = common::start_pg().await;

    common::ensure_recorder_installed();
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Set last_drain_pass_at to 60 seconds ago (well past the 30 s threshold).
    let stale_time = now_millis() - 60_000;
    let last_drain_pass_at = Arc::new(AtomicI64::new(stale_time));
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

    let app = atc_server::routes::api_routes()
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

/// A real drain stall — aborting the drain task — drives /readyz to 503.
///
/// The unit-style stale-heartbeat test sets `last_drain_pass_at` to a stale
/// value via the atomic and proves the handler reads it correctly, but
/// doesn't prove the heartbeat actually goes stale when the drain stops.
/// This test does the latter: it boots a full fixture (drain task running,
/// heartbeat fresh), then aborts the drain handle. With the drain task gone,
/// the heartbeat will never refresh again. We then stash a stale timestamp
/// into `last_drain_pass_at` and confirm `/readyz` returns 503 — proving the
/// handler's staleness check engages when the drain is genuinely dead, not
/// just artificially poked.
///
/// We don't wait 31 s of wall-clock time (would slow the suite); we abort
/// the drain AND set the atomic stale. Either alone is what each independent
/// failure mode looks like; together they emulate "the drain stopped 31 s
/// ago".
#[tokio::test]
#[serial]
async fn drain_abort_drives_503() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Sanity: heartbeat is fresh and /readyz is 200 right now.
    let pre_req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();
    let pre_resp = fixture.router.clone().oneshot(pre_req).await.unwrap();
    assert_eq!(
        pre_resp.status(),
        StatusCode::OK,
        "pre-abort: heartbeat fresh, /readyz must be 200"
    );

    // Abort the drain task — the heartbeat will not refresh from now on.
    fixture.drain_handle.abort();
    // Give tokio time to actually unschedule it. This is brief because abort
    // is synchronous from the caller's perspective.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        fixture.drain_handle.is_finished(),
        "drain task should have stopped after abort"
    );

    // Stash a stale timestamp. Were the drain still running, this would race
    // a heartbeat refresh — but the drain is gone, so the value sticks.
    let stale = now_millis() - 60_000;
    fixture.last_drain_pass_at.store(stale, Ordering::Relaxed);

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();
    let resp = fixture.router.clone().oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "drain dead + stale heartbeat must drive /readyz to 503"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["status"], "drain_stale",
        "body should report drain_stale"
    );

    // Confirm the heartbeat was NOT refreshed during the test — proves
    // the abort actually killed the heartbeat producer.
    let post = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(
        post, stale,
        "drain task is aborted; heartbeat must not have advanced",
    );

    fixture.shutdown.cancel();
}

/// When InMemoryStore is used (no PG), /readyz always returns 200.
///
/// InMemoryStore.liveness_check() always returns Ok(()). The drain-staleness
/// check only applies to PgStore, which has the heartbeat atomic.
#[tokio::test]
#[serial]
async fn no_pg_always_200() {
    // build_app_no_secret() wires InMemoryStore → liveness_check() = Ok(())
    let (app, _state) = common::build_app_no_secret();

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "InMemoryStore liveness is always ok, readyz must return 200"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok");
}

// ─── shutdown token cancelled ─────────────────────────────────────────────

/// With PG configured AND a fresh heartbeat — i.e. the path that would
/// otherwise return 200 — a cancelled shutdown token must still drive
/// `/readyz` to 503 `{"status":"shutting_down"}`. This is what locks the
/// "check shutdown before doing PG work" invariant: were the shutdown
/// short-circuit ever moved below the PG `SELECT 1`, this test would
/// observe a successful PG query and fall through to 200.
#[tokio::test]
#[serial]
async fn shutdown_cancelled_returns_503_with_pg() {
    let (pool, _container, _db_url) = common::start_pg().await;

    common::ensure_recorder_installed();
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Fresh heartbeat so the drain-staleness path would otherwise return 200.
    let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()));
    let broadcast_watermark = Arc::new(AtomicI64::new(0));

    let persist = Arc::new(PgStore::new(
        pool.clone(),
        Arc::clone(&broadcast_watermark),
        Arc::clone(&last_drain_pass_at),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let shutdown = CancellationToken::new();
    let app_state = Arc::new(AppState {
        persist,
        webhook_tx,
        webhook_secret: None,
        shutdown: shutdown.clone(),
        ws_tracker: TaskTracker::new(),
    });

    shutdown.cancel();

    let app = atc_server::routes::api_routes()
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
        "cancelled shutdown must beat PG check + fresh heartbeat"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["status"], "shutting_down",
        "body must indicate shutting_down, not ok or drain_stale"
    );
}

/// When state.shutdown is cancelled, GET /readyz returns 503 with
///     `{"status":"shutting_down"}` — even without PG.
#[tokio::test]
#[serial]
async fn shutdown_cancelled_returns_503() {
    let (app, state) = common::build_app_no_secret();
    // Extract the shutdown token from state and cancel it.
    state.shutdown.cancel();

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "cancelled shutdown token must cause /readyz to return 503"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        json["status"], "shutting_down",
        "body should indicate shutting_down"
    );
}

/// When state.shutdown is cancelled, GET /healthz still returns 200 — liveness
///     must not restart the pod mid-drain.
#[tokio::test]
#[serial]
async fn healthz_returns_200_after_shutdown() {
    let (app, state) = common::build_app_no_secret();
    state.shutdown.cancel();

    let req = Request::builder()
        .method("GET")
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "/healthz must return 200 even after shutdown is cancelled"
    );

    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["status"], "ok", "body should be ok");
}

// ─── fresh heartbeat → 200 ────────────────────────────────────────────────

/// When the drain task is running and healthy, GET /readyz returns 200.
///
/// Uses `build_app_with_pg_and_listener` which starts a real drain task.
/// After the fixture initializes (drain_started fires), the heartbeat should
/// be fresh. Assert /readyz returns 200.
#[tokio::test]
#[serial]
async fn fresh_heartbeat_returns_200() {
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
    let last = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    let age_ms = now_millis().saturating_sub(last);
    assert!(
        age_ms < 10_000,
        "last_drain_pass_at should be recent; age_ms = {age_ms}"
    );

    fixture.shutdown.cancel();
}

/// The drain heartbeat ticks even during quiet periods (no events).
///
/// Sleeps past the 5 s HEARTBEAT_TICK and asserts the heartbeat has been
/// refreshed — proving the tick fires even when no NOTIFY arrives.
#[tokio::test]
#[serial]
async fn heartbeat_ticks_during_quiet_period() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Capture timestamp before the quiet tick.
    let before_ms = now_millis();

    // Wait 6 s (> HEARTBEAT_TICK = 5 s) during which no events are fired.
    tokio::time::sleep(Duration::from_secs(6)).await;

    // The heartbeat should have been refreshed after the tick.
    let last = fixture.last_drain_pass_at.load(Ordering::Relaxed);

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
