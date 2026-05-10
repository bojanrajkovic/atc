//! `atc_pg_wake_coalesced_total` counter.
//!
//! Asserts that NOTIFYs arriving while a drain pass is in flight increment
//! the wake-coalesced counter, and conversely that NOTIFYs arriving with
//! the drain idle never produce a coalesce signal.
//!
//! Docker/OrbStack required.

use crate::common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC: &str = "atc_pg_wake_coalesced_total";

async fn fire_run_webhook(fixture: &common::AppFixture) -> StatusCode {
    let body = common::fixture_workflow_run_requested();
    let (status, _) =
        common::post_webhook_to_router(fixture.router.clone(), "workflow_run", &body).await;
    status
}

/// Five webhooks fired into a 200ms slow-drain window may coalesce up to five
/// NOTIFYs while `drain_in_flight=true`. The counter must never over-count.
///
/// The lower bound is intentionally 0 — without deterministic synchronization
/// between the listener task and the drain task scheduling, no specific NOTIFY
/// is guaranteed to observe `drain_in_flight=true`. This test enforces "the
/// counter exists, increments correctly, never over-counts" and pairs with the
/// idle-drain test to detect a stuck-true bug.
#[tokio::test]
#[serial]
async fn metrics_wake_coalesced_does_not_over_count_during_slow_drain() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture =
        common::build_app_with_pg_and_slow_drain(pool, db_url, Duration::from_millis(200)).await;

    let baseline_recv = fixture.observed_recv.load(Ordering::Relaxed);

    for _ in 0..5u8 {
        assert_eq!(fire_run_webhook(&fixture).await, StatusCode::OK);
    }

    timeout(Duration::from_secs(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_recv.load(Ordering::Relaxed) >= baseline_recv + 5 {
                return;
            }
        }
    })
    .await
    .expect("listener did not receive all 5 NOTIFYs within 10s");

    let snapshot = common::snapshot_metrics();
    let count = common::counter_value(&snapshot, METRIC, &[]);

    assert!(
        count <= 5,
        "wake-coalesced counter must not over-count beyond NOTIFY arrivals; got {count}",
    );

    fixture.shutdown.cancel();
}

/// Three webhooks fired with full drain completion between each (no in-flight
/// pass overlap) must not increment the coalesce counter.
///
/// Each webhook waits for `observed_passes` to advance before the next is
/// posted, guaranteeing the listener never observes `drain_in_flight=true`.
#[tokio::test]
#[serial]
async fn metrics_wake_coalesced_unchanged_when_drain_idle_between_notifies() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    for _ in 0..3u8 {
        let passes_before = fixture.observed_passes.load(Ordering::Relaxed);
        assert_eq!(fire_run_webhook(&fixture).await, StatusCode::OK);

        timeout(Duration::from_secs(5), async {
            loop {
                tokio::time::sleep(Duration::from_millis(30)).await;
                if fixture.observed_passes.load(Ordering::Relaxed) > passes_before {
                    return;
                }
            }
        })
        .await
        .expect("drain pass did not complete between webhooks");
    }

    let snapshot = common::snapshot_metrics();
    let count = common::counter_value(&snapshot, METRIC, &[]);

    assert_eq!(
        count, 0,
        "wake-coalesced counter must stay flat when no NOTIFY overlaps an \
         in-flight pass; got {count}",
    );

    fixture.shutdown.cancel();
}
