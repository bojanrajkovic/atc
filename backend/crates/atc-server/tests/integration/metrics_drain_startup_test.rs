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

const METRIC_COUNT: &str = "atc_pg_drain_startup_seconds_count";
const METRIC_SUM: &str = "atc_pg_drain_startup_seconds_sum";

/// A fresh fixture (which spawns a new drain task and runs one startup pass)
/// records exactly one startup observation with a positive duration value.
/// Additional webhooks driven through the same fixture do NOT extend the startup
/// count: the once-per-process contract.
#[tokio::test]
#[serial]
async fn metrics_drain_startup_records_once_per_process() {
    common::ensure_recorder_installed();

    let baseline = common::render_metrics();
    let baseline_count = common::parse_unlabeled_counter(&baseline, METRIC_COUNT);
    let baseline_sum = common::parse_unlabeled_gauge(&baseline, METRIC_SUM).unwrap_or(0.0);

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let after_startup = common::render_metrics();
    let after_startup_count = common::parse_unlabeled_counter(&after_startup, METRIC_COUNT);
    let after_startup_sum =
        common::parse_unlabeled_gauge(&after_startup, METRIC_SUM).unwrap_or(0.0);

    assert_eq!(
        after_startup_count - baseline_count,
        1,
        "expected exactly one startup observation after fixture init; \
         baseline={baseline_count} after={after_startup_count}"
    );
    assert!(
        after_startup_sum > baseline_sum,
        "startup _sum must increase by a positive value; \
         baseline={baseline_sum} after={after_startup_sum}"
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

    let after_nth = common::render_metrics();
    let after_nth_count = common::parse_unlabeled_counter(&after_nth, METRIC_COUNT);

    assert_eq!(
        after_nth_count - after_startup_count,
        0,
        "startup observation must fire once per process; subsequent drain \
         passes added {} more observations",
        after_nth_count - after_startup_count
    );

    fixture.shutdown.cancel();
}
