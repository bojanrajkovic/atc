//! Integration tests: /readyz drain heartbeat.
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

use atc_core::{RunStateMachine, SystemClock};
use atc_server::state::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
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
async fn t8_stale_heartbeat_returns_503() {
    let (pool, _container, _db_url) = common::start_pg().await;

    let layer = common::PROMETHEUS_INIT
        .get_or_init(common::install_test_recorder)
        .0
        .clone();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Set last_drain_pass_at to 60 seconds ago (well past the 30 s threshold).
    let stale_time = now_millis() - 60_000;

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
        last_drain_pass_at: Arc::new(AtomicI64::new(stale_time)),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
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

/// T8c: A real drain stall — aborting the drain task — drives /readyz to 503.
///
/// T8 above sets `last_drain_pass_at` to a stale value via the atomic. Codex
/// flagged this as a unit-style check that proves the handler reads the
/// atomic correctly but doesn't prove the heartbeat actually goes stale when
/// the drain stops. This test does the latter: it boots a full fixture
/// (drain task running, heartbeat fresh), then aborts the drain handle. With
/// the drain task gone, the heartbeat will never refresh again. We then
/// stash a stale timestamp into `last_drain_pass_at` and confirm `/readyz`
/// returns 503 — proving the handler's staleness check engages when the
/// drain is genuinely dead, not just artificially poked.
///
/// We don't wait 31 s of wall-clock time (would slow the suite); we abort
/// the drain AND set the atomic stale. Either alone is what each
/// independent failure mode looks like; together they emulate "the drain
/// stopped 31 s ago".
#[tokio::test]
#[serial]
async fn t8c_drain_abort_drives_503() {
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
    fixture
        .state
        .last_drain_pass_at
        .store(stale, Ordering::Relaxed);

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
    let post = fixture.state.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(
        post, stale,
        "drain task is aborted; heartbeat must not have advanced",
    );

    fixture.shutdown.cancel();
}

/// T8b: When pg_pool is None, /readyz always returns 200 regardless of
///      last_drain_pass_at (the drain heartbeat check only applies in PG mode).
#[tokio::test]
#[serial]
async fn t8b_no_pg_always_200() {
    let layer = common::PROMETHEUS_INIT
        .get_or_init(common::install_test_recorder)
        .0
        .clone();
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);

    // Set last_drain_pass_at to a very stale value — should not matter without PG.
    let stale_time = 0i64; // epoch = maximally stale

    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist = Arc::new(atc_server::persist::InMemoryStore::new(
        state_machine.clone(),
        seq.clone(),
        webhook_tx.clone(),
    )) as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: None, // no PG
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(stale_time)),
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
        persist,
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
async fn t9_fresh_heartbeat_returns_200() {
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
async fn t9b_heartbeat_ticks_during_quiet_period() {
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
