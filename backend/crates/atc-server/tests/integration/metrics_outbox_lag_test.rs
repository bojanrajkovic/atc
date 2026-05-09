//! `atc_pg_outbox_lag_seconds` histogram.
//!
//! Asserts that every outbox row broadcast by the drain task records one
//! observation into the `atc_pg_outbox_lag_seconds` histogram, and that the
//! observation reaches the histogram even when the lag value is unusual
//! (e.g., negative because the row's `inserted_at` is in the future).
//!
//! Docker/OrbStack required.

use crate::common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC_COUNT: &str = "atc_pg_outbox_lag_seconds_count";
const METRIC_SUM: &str = "atc_pg_outbox_lag_seconds_sum";

/// A normal webhook drives one observation into the lag histogram.
///
/// One webhook, one outbox row, one drain pass, one broadcast → one
/// histogram observation. The healthy-path lag is sub-millisecond, so the
/// `_sum` delta is bounded loosely at 5 seconds to absorb CI scheduling
/// slop.
#[tokio::test]
#[serial]
async fn metrics_outbox_lag_records_one_observation_per_broadcast() {
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
    .expect("drain did not process the webhook within 5s");

    let after = common::render_metrics();
    let after_count = common::parse_unlabeled_counter(&after, METRIC_COUNT);
    let after_sum = common::parse_unlabeled_gauge(&after, METRIC_SUM).unwrap_or(0.0);

    assert_eq!(
        after_count - baseline_count,
        1,
        "expected exactly one outbox-lag observation per broadcast row; \
         baseline={baseline_count} after={after_count}"
    );

    let sum_delta = after_sum - baseline_sum;
    assert!(
        (0.0..=5.0).contains(&sum_delta),
        "outbox-lag _sum delta should be in [0.0, 5.0]s; got {sum_delta}"
    );

    fixture.shutdown.cancel();
}

/// A future-dated `inserted_at` produces a negative lag observation,
/// but the histogram still records it (no panic, no input-side clamping).
///
/// We do not assert on `_sum` because exporter handling of negative
/// histogram observations is exporter-version-specific. The behavioral
/// contract is: the lag computation reaches `histogram!().record()` and the
/// histogram remains usable after a sentinel-class observation.
#[tokio::test]
#[serial]
async fn metrics_outbox_lag_records_observation_for_future_inserted_at() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    let baseline_passes = fixture.observed_passes.load(Ordering::Relaxed);
    let baseline_count = common::parse_unlabeled_counter(&common::render_metrics(), METRIC_COUNT);

    // Stub run row to satisfy the outbox FK pattern used in the PG drain integration tests.
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (60000000001, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run insert failed");

    // Build a valid envelope so the drain decode path succeeds and the row
    // reaches the broadcast site (where the lag observation is recorded).
    let fixture_bytes = common::fixture_workflow_run_requested();
    let base_env = match atc_github::parse_webhook("workflow_run", &fixture_bytes)
        .expect("fixture must parse")
    {
        atc_github::ParseResult::Parsed(ev) => match *ev {
            atc_github::WebhookEvent::Run(e) => e,
            _ => panic!("expected Run variant"),
        },
        atc_github::ParseResult::Skipped { .. } => panic!("fixture must not be skipped"),
    };

    let mut env = base_env.clone();
    env.run_id = atc_core::types::RunId(60_000_000_001);
    let payload = serde_json::to_value(&env).expect("env serialization failed");

    let inserted_seq: i64 = sqlx::query_scalar(
        "INSERT INTO outbox (kind, run_id, payload, inserted_at) \
         VALUES ('run', 60000000001, $1, NOW() + INTERVAL '10 minutes') RETURNING seq",
    )
    .bind(&payload)
    .fetch_one(&pool)
    .await
    .expect("future-dated outbox INSERT failed");

    sqlx::query("SELECT pg_notify('atc_outbox', $1::text)")
        .bind(inserted_seq)
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            if fixture.observed_passes.load(Ordering::Relaxed) > baseline_passes {
                return;
            }
        }
    })
    .await
    .expect("drain did not process the future-dated row within 5s");

    let after_count = common::parse_unlabeled_counter(&common::render_metrics(), METRIC_COUNT);
    assert_eq!(
        after_count - baseline_count,
        1,
        "future-dated row must still record one lag observation; \
         baseline={baseline_count} after={after_count}"
    );

    fixture.shutdown.cancel();
}
