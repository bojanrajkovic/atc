//! `atc_pg_drain_shutdown_remaining_rows` histogram.
//!
//! Asserts that exiting the drain task records exactly one observation per
//! drain task lifetime, that the recorded value reflects rows committed past
//! the replica's watermark at exit time, and that the metric emits histogram
//! `_bucket` lines (i.e. the custom bucket override is wired so that
//! `metrics-exporter-prometheus` does not fall back to Summary representation).
//!
//! Docker/OrbStack required.

mod common;

use serial_test::serial;

const METRIC_COUNT: &str = "atc_pg_drain_shutdown_remaining_rows_count";
const METRIC_SUM: &str = "atc_pg_drain_shutdown_remaining_rows_sum";
const METRIC_BUCKET_PREFIX: &str = "atc_pg_drain_shutdown_remaining_rows_bucket";

/// Insert a minimal stub runs row to satisfy the outbox FK constraint. Uses
/// the untyped sqlx API so the new query does not require regenerating
/// `.sqlx/`.
async fn insert_stub_run(pool: &sqlx::PgPool, run_id: i64) {
    sqlx::query(
        r"
        INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at)
        VALUES ($1, 'test-org', 'test-repo', '', '', '', '', 'Queued', now(), now())
        ON CONFLICT (id) DO NOTHING
        ",
    )
    .bind(run_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Insert an outbox row directly. Critically, this does NOT call
/// `pg_notify('atc_outbox', ...)`, so the drain task's NOTIFY-driven select
/// arm never fires for these rows. Combined with the heartbeat-only arm not
/// scanning the outbox, the drain leaves them undelivered until shutdown — at
/// which point the shutdown count query observes them.
async fn insert_outbox_row_silent(pool: &sqlx::PgPool, run_id: i64) {
    sqlx::query("INSERT INTO outbox (kind, run_id, payload) VALUES ('run', $1, '{}'::jsonb)")
        .bind(run_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Drain task records exactly one shutdown observation per lifetime, the
/// observation reflects the lag at exit time, and the histogram emits
/// `_bucket` lines (proves the bucket override is wired).
#[tokio::test]
#[serial]
async fn drain_shutdown_records_remaining_rows_at_task_exit() {
    common::ensure_recorder_installed();

    let baseline = common::render_metrics();
    let baseline_count = common::parse_unlabeled_counter(&baseline, METRIC_COUNT);
    let baseline_sum = common::parse_unlabeled_gauge(&baseline, METRIC_SUM).unwrap_or(0.0);

    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Insert three outbox rows without firing NOTIFY. The drain task's loop
    // wakes only on NOTIFY (or 5 s heartbeat for `/readyz`, which does not
    // scan), so these rows sit undrained until shutdown.
    let run_ids: [i64; 3] = [80_001, 80_002, 80_003];
    for &run_id in &run_ids {
        insert_stub_run(&pool, run_id).await;
        insert_outbox_row_silent(&pool, run_id).await;
    }

    // Trigger shutdown and join the drain handle. The shutdown observation is
    // recorded after the loop exits and before the spawned task returns;
    // joining the handle guarantees the recorder has seen it before we
    // scrape.
    fixture.shutdown.cancel();
    fixture
        .drain_handle
        .await
        .expect("drain task should join cleanly");

    let after = common::render_metrics();
    let after_count = common::parse_unlabeled_counter(&after, METRIC_COUNT);
    let after_sum = common::parse_unlabeled_gauge(&after, METRIC_SUM).unwrap_or(0.0);

    assert_eq!(
        after_count - baseline_count,
        1,
        "expected exactly one shutdown observation; baseline={baseline_count} after={after_count}",
    );

    let observed_value = after_sum - baseline_sum;
    let expected = run_ids.len() as f64;
    let epsilon = 1e-6;
    assert!(
        (observed_value - expected).abs() < epsilon,
        "shutdown observation should record {expected} rows past watermark; got {observed_value} \
         (after_sum={after_sum} baseline_sum={baseline_sum})",
    );

    // Confirm the histogram emits `_bucket` lines. Without the
    // `Matcher::Full` bucket override in `install_recorder`, an unmatched
    // histogram would render as Summary and this assertion would fail —
    // catching a silent regression in the bucket configuration.
    assert!(
        after.lines().any(|l| l.starts_with(METRIC_BUCKET_PREFIX)),
        "expected histogram `_bucket` lines for {METRIC_BUCKET_PREFIX}; \
         metric likely fell back to Summary representation",
    );
}
