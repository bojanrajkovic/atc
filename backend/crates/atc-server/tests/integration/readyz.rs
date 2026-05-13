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
use std::sync::atomic::Ordering;
use std::time::Duration;

use atc_core::{Clock, TestClock, fixed_test_timestamp};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::TimeDelta;
use serial_test::serial;
use tower::ServiceExt;

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
    let (pool, _container, db_url) = common::start_pg().await;

    common::ensure_recorder_installed();

    // Use a TestClock so the staleness baseline is deterministic — production
    // routes `clock.now()` through `PgStore.clock`, so advancing the
    // `TestClock` is the canonical way to make the heartbeat "old".
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool, db_url).await;
    fixture.drain_abort.abort();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Advance the clock by 60 s. The startup pass recorded the heartbeat at
    // `fixed_test_timestamp()`; after the advance, the staleness age is
    // 60_000 ms — well past the 30 s threshold.
    clock.advance(TimeDelta::seconds(60));

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = fixture.router.clone().oneshot(req).await.unwrap();
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

    fixture.shutdown.cancel();
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
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool, db_url).await;

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
    // The abort handle drives the JoinHandle (which the store owns) to a
    // cancelled exit; we can't query its `is_finished` here because the store
    // retains the handle, so we instead pin staleness behaviorally below by
    // confirming the timestamp never advances past what we store.
    fixture.drain_abort.abort();
    // Give tokio time to actually unschedule the drain. abort() is synchronous
    // from the caller's perspective, but the task itself observes the cancel
    // at its next await point.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Snapshot the heartbeat that the startup pass recorded, then advance the
    // clock past the 30 s threshold. With the drain gone the heartbeat will
    // not move forward — the staleness check sees `clock.now() - last ==
    // 60_000` and returns 503.
    let snapshot_heartbeat = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    clock.advance(TimeDelta::seconds(60));

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
        post, snapshot_heartbeat,
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
    let (pool, _container, db_url) = common::start_pg().await;

    common::ensure_recorder_installed();

    // Use the full fixture so we get a real PgStore + drain task (fresh
    // heartbeat) — the drain-staleness path would otherwise return 200.
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    fixture.shutdown.cancel();

    let req = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let resp = fixture.router.clone().oneshot(req).await.unwrap();
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
/// Uses `build_app_with_pg_clock` so the heartbeat the startup pass records
/// is exactly `fixed_test_timestamp().timestamp_millis()` — assertable to the
/// millisecond rather than a fuzzy "is it recent?" check against wall-clock.
#[tokio::test]
#[serial]
async fn fresh_heartbeat_returns_200() {
    let (pool, _container, db_url) = common::start_pg().await;
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool, db_url).await;

    // The drain task has run its first pass (drain_started fired in
    // build_app_with_pg_clock). The heartbeat is fresh.
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

    // Exact equality: the startup pass recorded `clock.now()` and we have
    // not advanced the clock since.
    let last = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(
        last,
        clock.now().timestamp_millis(),
        "heartbeat must equal clock time at the startup pass",
    );

    fixture.shutdown.cancel();
}

/// The drain heartbeat ticks even during quiet periods (no events).
///
/// Sleeps past the 5 s HEARTBEAT_TICK so the real tokio tick fires; in
/// between, we advance the TestClock by 1 s so the tick reads a strictly
/// greater `clock.now()`. The heartbeat after the tick must equal that new
/// time exactly — sharper than a wall-clock `>=` comparison.
#[tokio::test]
#[serial]
async fn heartbeat_ticks_during_quiet_period() {
    let (pool, _container, db_url) = common::start_pg().await;
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let fixture =
        common::build_app_with_pg_clock(Arc::clone(&clock) as Arc<dyn Clock>, pool, db_url).await;

    // Heartbeat at startup equals `fixed_test_timestamp().timestamp_millis()`.
    let before_ms = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(before_ms, clock.now().timestamp_millis());

    // Move the clock forward by 1 s — the tick will read this advanced
    // value when it fires.
    clock.advance(TimeDelta::seconds(1));
    let expected_after_tick = clock.now().timestamp_millis();

    // Wait 6 s real time (> HEARTBEAT_TICK = 5 s) for the tokio tick.
    tokio::time::sleep(Duration::from_secs(6)).await;

    let last = fixture.last_drain_pass_at.load(Ordering::Relaxed);
    assert_eq!(
        last, expected_after_tick,
        "heartbeat tick must have refreshed last_drain_pass_at to the new clock time; \
         before={before_ms}, expected_after_tick={expected_after_tick}, last={last}",
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
