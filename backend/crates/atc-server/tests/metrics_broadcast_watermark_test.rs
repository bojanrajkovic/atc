//! `atc_pg_broadcast_watermark` gauge.
//!
//! Asserts that the gauge mirrors the per-replica `broadcast_watermark`
//! atomic after each successful drain pass and that it is initialized at
//! startup (not at first-broadcast).
//!
//! Docker/OrbStack required.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

const METRIC: &str = "atc_pg_broadcast_watermark";

/// After three webhooks have been processed and drained, the
/// `atc_pg_broadcast_watermark` gauge equals `MAX(seq)` from the outbox.
#[tokio::test]
#[serial]
async fn metrics_broadcast_watermark_tracks_max_outbox_seq() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    let baseline_passes = fixture.observed_passes.load(Ordering::Relaxed);

    for _ in 0..3u8 {
        let body = common::fixture_workflow_run_requested();
        let (status, _) =
            common::post_webhook_to_router(fixture.router.clone(), "workflow_run", &body).await;
        assert_eq!(status, StatusCode::OK);
    }

    timeout(Duration::from_secs(10), async {
        loop {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let row_count: i64 =
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::bigint FROM outbox")
                    .fetch_one(&pool)
                    .await
                    .unwrap_or(0);
            let passes = fixture.observed_passes.load(Ordering::Relaxed);
            if row_count == 3 && passes > baseline_passes {
                return;
            }
        }
    })
    .await
    .expect("drain did not process all 3 webhooks within 10s");

    // Allow the drain a moment to complete its watermark store after the
    // last broadcast (the gauge mirror runs alongside the atomic store).
    tokio::time::sleep(Duration::from_millis(100)).await;

    let body = common::render_metrics();
    let gauge = common::parse_unlabeled_gauge(&body, METRIC)
        .expect("atc_pg_broadcast_watermark must be present after 3 webhooks");

    let max_seq: i64 = sqlx::query_scalar::<_, i64>("SELECT MAX(seq) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("MAX(seq) query failed");

    #[allow(clippy::cast_precision_loss)]
    let expected = max_seq as f64;
    assert!(
        (gauge - expected).abs() < f64::EPSILON,
        "broadcast_watermark gauge ({gauge}) must equal MAX(outbox.seq) ({expected})"
    );

    fixture.shutdown.cancel();
}

/// In a fresh fixture before any webhook is POSTed, the
/// `atc_pg_broadcast_watermark` gauge equals 0 (the seed value mirrored
/// after the COALESCE(MAX(seq),0) initialization in main.rs / the test
/// fixture builder).
#[tokio::test]
#[serial]
async fn metrics_broadcast_watermark_seeded_at_startup() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;

    let body = common::render_metrics();
    let gauge = common::parse_unlabeled_gauge(&body, METRIC)
        .expect("atc_pg_broadcast_watermark must be seeded at startup");

    assert!(
        (gauge - 0.0).abs() < f64::EPSILON,
        "broadcast_watermark gauge must equal 0 at startup with empty outbox; got {gauge}"
    );

    fixture.shutdown.cancel();
}
