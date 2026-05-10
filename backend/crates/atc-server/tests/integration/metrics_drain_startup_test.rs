//! `atc_pg_drain_startup_seconds` histogram.
//!
//! Asserts that startup-init latency (watermark init through first drain
//! pass exit) records exactly one observation per process lifetime, and
//! that subsequent drain passes do not extend the startup count.
//!
//! Docker/OrbStack required.

use crate::common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC: &str = "atc_pg_drain_startup_seconds";

/// A fresh fixture (which spawns a new drain task and runs one startup pass)
/// records exactly one startup observation with a positive duration value.
/// Additional webhooks driven through the same fixture do NOT extend the startup
/// count: the once-per-process contract.
#[tokio::test]
#[serial]
async fn metrics_drain_startup_records_once_per_process() {
    common::ensure_recorder_installed();

    // Reset BEFORE fixture init: the test's contract is that fixture init
    // (which spawns the drain task and runs one startup pass) records exactly
    // one startup observation. Resetting after init would erase the observation
    // before the snapshot can see it.
    common::reset_metrics();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let snapshot = common::snapshot_metrics();
    let startup_count = common::histogram_count(&snapshot, METRIC, &[]);
    let startup_sum = common::histogram_sum(&snapshot, METRIC, &[]);

    assert_eq!(
        startup_count, 1,
        "expected exactly one startup observation after fixture init; got {startup_count}",
    );
    assert!(
        startup_sum > 0.0,
        "startup sum must be a positive duration; got {startup_sum}"
    );

    // Drive a webhook through the same fixture; subsequent drain
    // passes must NOT add to the startup _count.
    let passes_before = fixture.observed_passes.load(Ordering::Relaxed);
    let body = common::fixture_workflow_run_requested();
    let (status, _) =
        common::post_webhook_to_router(fixture.router.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK);

    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > passes_before {
                return;
            }
        }
    })
    .await
    .expect("post-startup drain pass did not complete within 5s");

    let snapshot = common::snapshot_metrics();
    let after_count = common::histogram_count(&snapshot, METRIC, &[]);
    assert_eq!(
        after_count, 0,
        "startup observation must fire once per process; \
         second snapshot recorded {after_count} additional observations (must be 0)",
    );

    fixture.shutdown.cancel();
}
