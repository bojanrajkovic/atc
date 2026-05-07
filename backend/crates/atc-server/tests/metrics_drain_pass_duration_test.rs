//! Phase 5 — `atc_pg_drain_pass_duration_seconds` histogram.
//!
//! Asserts that NOTIFY-driven drain passes record a wall-time observation,
//! and that heartbeat-only wakes (the 5s tick branch of the drain loop)
//! do NOT record a pass — the metric is bound to NOTIFY-driven passes only.
//!
//! Docker/OrbStack required.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC_COUNT: &str = "atc_pg_drain_pass_duration_seconds_count";
const METRIC_SUM: &str = "atc_pg_drain_pass_duration_seconds_sum";

/// AC3 — A NOTIFY-driven drain pass adds one observation, with a positive
/// duration value.
#[tokio::test]
#[serial]
async fn metrics_drain_pass_duration_records_one_observation_per_pass() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let baseline = common::render_metrics();
    let baseline_count = common::parse_unlabeled_counter(&baseline, METRIC_COUNT);
    let baseline_sum = common::parse_unlabeled_gauge(&baseline, METRIC_SUM).unwrap_or(0.0);

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

    let after = common::render_metrics();
    let after_count = common::parse_unlabeled_counter(&after, METRIC_COUNT);
    let after_sum = common::parse_unlabeled_gauge(&after, METRIC_SUM).unwrap_or(0.0);

    let count_delta = after_count - baseline_count;
    assert_eq!(
        count_delta, 1,
        "expected exactly one drain-pass duration observation per pass; \
         baseline={baseline_count} after={after_count}"
    );

    let sum_delta = after_sum - baseline_sum;
    assert!(
        (0.0001..=1.0).contains(&sum_delta),
        "drain-pass duration _sum delta should be in [0.0001, 1.0]s; got {sum_delta}"
    );

    fixture.shutdown.cancel();
}

/// AC3b — Heartbeat-only wakes do NOT execute a drain pass and therefore
/// do NOT add observations to the duration histogram.
///
/// `HEARTBEAT_TICK = 5s`. We wait `2 * HEARTBEAT_TICK` after fixture init
/// (which already ran one startup pass) to give two heartbeat ticks a
/// chance to fire. No webhooks are POSTed, so no NOTIFY arrives.
#[tokio::test]
#[serial]
async fn metrics_drain_pass_duration_unchanged_for_heartbeat_only_wakes() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let baseline_count = common::parse_unlabeled_counter(&common::render_metrics(), METRIC_COUNT);

    tokio::time::sleep(Duration::from_secs(11)).await;

    let after_count = common::parse_unlabeled_counter(&common::render_metrics(), METRIC_COUNT);

    assert_eq!(
        after_count, baseline_count,
        "heartbeat-only wakes must not record drain-pass observations; \
         baseline={baseline_count} after={after_count}"
    );

    fixture.shutdown.cancel();
}
