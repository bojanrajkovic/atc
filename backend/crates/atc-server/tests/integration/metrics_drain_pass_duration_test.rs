//! `atc_pg_drain_pass_duration_seconds` histogram.
//!
//! Asserts that NOTIFY-driven drain passes record a wall-time observation,
//! and that heartbeat-only wakes (the 5s tick branch of the drain loop)
//! do NOT record a pass — the metric is bound to NOTIFY-driven passes only.
//!
//! Docker/OrbStack required.

use crate::common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC: &str = "atc_pg_drain_pass_duration_seconds";

/// A NOTIFY-driven drain pass adds one observation, with a positive duration value.
#[tokio::test]
#[serial]
async fn metrics_drain_pass_duration_records_one_observation_per_pass() {
    common::ensure_recorder_installed();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    // Reset AFTER fixture init so the snapshot only reflects passes that ran
    // for the webhook fired below — fixture init itself runs an unconditional
    // first pass which would otherwise inflate the count.
    common::reset_metrics();
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
    .expect("drain pass did not complete within 5s");

    let snapshot = common::snapshot_metrics();
    let count = common::histogram_count(&snapshot, METRIC, &[]);
    let sum = common::histogram_sum(&snapshot, METRIC, &[]);

    assert_eq!(
        count, 1,
        "expected exactly one drain-pass duration observation per pass; got {count}",
    );

    assert!(
        (0.0001..=1.0).contains(&sum),
        "drain-pass duration sum should be in [0.0001, 1.0]s; got {sum}"
    );

    fixture.shutdown.cancel();
}

/// Heartbeat-only wakes do NOT execute a drain pass and therefore
/// do NOT add observations to the duration histogram.
///
/// `HEARTBEAT_TICK = 5s`. We wait `2 * HEARTBEAT_TICK` after fixture init
/// (which already ran one startup pass) to give two heartbeat ticks a
/// chance to fire. No webhooks are POSTed, so no NOTIFY arrives.
#[tokio::test]
#[serial]
async fn metrics_drain_pass_duration_unchanged_for_heartbeat_only_wakes() {
    common::ensure_recorder_installed();

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    common::reset_metrics();
    tokio::time::sleep(Duration::from_secs(11)).await;

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::histogram_count(&snapshot, METRIC, &[]),
        0,
        "heartbeat-only wakes must not record drain-pass observations",
    );

    fixture.shutdown.cancel();
}
