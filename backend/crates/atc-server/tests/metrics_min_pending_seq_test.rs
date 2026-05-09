//! `atc_pg_min_pending_seq` gauge.
//!
//! Asserts that the gauge mirrors the per-replica gap-healing backstop
//! atomic when it holds a real registered seq, and that it returns to its
//! `f64::NAN` sentinel after the drain swap captures the floor.
//!
//! AC8 (success) deliberately removes the drain task from the runtime
//! (`drain_handle.abort()`) so the listener's `fetch_min` mirror is not
//! racing the drain's swap-to-NaN. This trades the microseconds-fragile
//! "scrape during gap-healing" approach for a deterministic
//! manufacture-the-gauge-state pattern.
//!
//! Docker/OrbStack required.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC: &str = "atc_pg_min_pending_seq";
const WATERMARK_METRIC: &str = "atc_pg_broadcast_watermark";

/// AC8 — Drive the broadcast watermark forward, then abort the drain task
/// so its swap-to-NaN can't fire. A subsequent `pg_notify` for an earlier
/// seq drives the listener's `fetch_min` mirror, leaving the gauge at a
/// finite numeric value below `broadcast_watermark`.
#[tokio::test]
#[serial]
async fn metrics_min_pending_seq_mirrors_finite_seq_when_drain_idle() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Step 1: advance the broadcast watermark by one webhook.
    let baseline_passes = fixture.observed_passes.load(Ordering::Relaxed);
    let body = common::fixture_workflow_run_requested();
    let (status, _) =
        common::post_webhook_to_router(fixture.router.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK);

    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > baseline_passes {
                return;
            }
        }
    })
    .await
    .expect("drain did not advance watermark within 5s");

    // Allow the drain's atomic store + gauge mirror to settle.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let watermark_seq: i64 = sqlx::query_scalar::<_, i64>("SELECT MAX(seq) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("MAX(seq) query failed");
    assert!(
        watermark_seq >= 1,
        "watermark must have advanced to at least 1; got {watermark_seq}"
    );

    // Step 2: abort the drain so it cannot swap min_pending_seq back to
    // i64::MAX (gauge → NaN). The listener task stays alive. Wait for the
    // join handle to actually finish — `abort()` schedules cancellation but
    // doesn't synchronously stop the task, and a fixed sleep gives no
    // guarantee that the drain's `notified().await` has been canceled before
    // our pg_notify drives the listener.
    fixture.drain_handle.abort();
    let drain_finished = timeout(Duration::from_secs(5), async {
        while !fixture.drain_handle.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert!(
        drain_finished.is_ok(),
        "drain task did not finish within 5s after abort()"
    );

    // Step 3: drive a NOTIFY for seq=0, which is below the post-webhook
    // watermark (>=1). The listener's `fetch_min` mirrors this finite
    // value into the gauge regardless of whether seq=0 actually exists in
    // the outbox — the listener does not consult the table; the metric
    // only reports "lowest pending seq registered with the backstop."
    let recv_before = fixture.observed_recv.load(Ordering::Relaxed);
    sqlx::query("SELECT pg_notify('atc_outbox', '0')")
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_recv.load(Ordering::Relaxed) > recv_before {
                return;
            }
        }
    })
    .await
    .expect("listener did not receive the manual NOTIFY within 5s");

    // Allow the listener's gauge mirror to settle after the fetch_min.
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body = common::render_metrics();
    let gauge = common::parse_unlabeled_gauge(&body, METRIC)
        .expect("atc_pg_min_pending_seq must be present after fetch_min mirror");
    let watermark_gauge = common::parse_unlabeled_gauge(&body, WATERMARK_METRIC)
        .expect("atc_pg_broadcast_watermark must be present");

    assert!(
        gauge.is_finite(),
        "min_pending_seq gauge must be finite after listener fetch_min; got {gauge}"
    );
    assert!(
        gauge < watermark_gauge,
        "min_pending_seq ({gauge}) must be below broadcast_watermark \
         ({watermark_gauge}) — that's the gap-healing signal"
    );

    fixture.shutdown.cancel();
}

/// AC8b — In a steady-state fixture (no in-flight gap-healing), the gauge
/// is `NaN`: the drain swap-to-NaN runs at the start of every pass, and
/// after the listener has caught up there is no pending fetch_min to
/// mirror.
#[tokio::test]
#[serial]
async fn metrics_min_pending_seq_is_nan_in_steady_state() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let baseline_passes = fixture.observed_passes.load(Ordering::Relaxed);

    for _ in 0..3u8 {
        let body = common::fixture_workflow_run_requested();
        let (status, _) =
            common::post_webhook_to_router(fixture.router.clone(), "workflow_run", &body).await;
        assert_eq!(status, StatusCode::OK);
    }

    // Wait for steady state: the drain has caught up (broadcast_watermark
    // matches MAX(seq) in the outbox) AND the gauge has reached its NaN
    // sentinel. Polling the gauge directly avoids fixed-sleep flake — every
    // drain pass swaps min_pending_seq to MAX and emits NaN, so once the
    // drain has processed all pending rows the gauge MUST settle at NaN.
    let mut last_gauge = None;
    let result = timeout(Duration::from_secs(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) <= baseline_passes {
                continue;
            }
            let body = common::render_metrics();
            let gauge = common::parse_unlabeled_gauge(&body, METRIC);
            last_gauge = gauge;
            if gauge.map(f64::is_nan).unwrap_or(false) {
                return;
            }
        }
    })
    .await;
    assert!(
        result.is_ok(),
        "min_pending_seq gauge did not reach NaN steady-state within 10s; \
         last observed: {last_gauge:?}"
    );

    let body = common::render_metrics();
    let gauge = common::parse_unlabeled_gauge(&body, METRIC)
        .expect("atc_pg_min_pending_seq must be present (sentinel state)");

    assert!(
        gauge.is_nan(),
        "min_pending_seq gauge must be NaN in steady state (drain caught up); got {gauge}"
    );

    fixture.shutdown.cancel();
}
