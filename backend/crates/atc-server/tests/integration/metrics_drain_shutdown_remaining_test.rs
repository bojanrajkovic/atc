//! `atc_pg_drain_shutdown_remaining_rows` histogram.
//!
//! Asserts that exiting the drain task records exactly one observation per
//! drain task lifetime and that the recorded value reflects rows committed
//! past the replica's watermark at exit time.
//!
//! Docker/OrbStack required.

use crate::common;

use serial_test::serial;

const METRIC: &str = "atc_pg_drain_shutdown_remaining_rows";

/// Insert a minimal stub runs row to satisfy the outbox FK constraint. Uses
/// the untyped sqlx API so the new query does not require regenerating
/// `.sqlx/`.
async fn insert_stub_run(pool: &atc_store_pg::TracedPool, run_id: i64) {
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
async fn insert_outbox_row_silent(pool: &atc_store_pg::TracedPool, run_id: i64) {
    sqlx::query("INSERT INTO outbox (kind, run_id, payload) VALUES ('run', $1, '{}'::jsonb)")
        .bind(run_id)
        .execute(pool)
        .await
        .unwrap();
}

/// Drain task records exactly one shutdown observation per lifetime, and the
/// observation reflects the lag at exit time.
#[tokio::test]
#[serial]
async fn drain_shutdown_records_remaining_rows_at_task_exit() {
    common::ensure_recorder_installed();
    common::reset_metrics();

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
    // snapshot.
    fixture.shutdown.cancel();
    // `persist.shutdown()` joins the drain (and listener). The shutdown
    // observation is recorded inside the drain task before it returns, so
    // waiting on the join guarantees the recorder has seen it before we
    // snapshot.
    tokio::time::timeout(
        std::time::Duration::from_secs(8),
        fixture.state.persist.shutdown(),
    )
    .await
    .expect("persist.shutdown should complete within 8s");

    let snapshot = common::snapshot_metrics();
    let count = common::histogram_count(&snapshot, METRIC, &[]);
    let sum = common::histogram_sum(&snapshot, METRIC, &[]);

    assert_eq!(
        count, 1,
        "expected exactly one shutdown observation; got {count}",
    );

    let expected = run_ids.len() as f64;
    let epsilon = 1e-6;
    assert!(
        (sum - expected).abs() < epsilon,
        "shutdown observation should record {expected} rows past watermark; got {sum}",
    );
}
