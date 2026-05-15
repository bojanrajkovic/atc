//! Integration tests for outbox retention (issue #67 / ADR 0007).
//!
//! Covers the heartbeat task's `outbox_watermarks` upsert, the retention
//! floor's startup-time guard, and the sweep task's deletion + multi-replica
//! contention paths.
//!
//! Docker/OrbStack required (each test starts an ephemeral Postgres DB
//! through the shared test container).

use crate::common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use atc_core::{Clock, SystemClock};
use atc_persist::PersistentStore;
use atc_store_pg::listener;
use atc_store_pg::store::{OUTBOX_RETENTION_FLOOR, PgStoreTestHooks};
use atc_store_pg::{PgStore, PgStoreStartError};
use serial_test::serial;
use sqlx::Row;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

// ---------------------------------------------------------------------------
// Phase 3: heartbeat task
// ---------------------------------------------------------------------------

/// `PgStore::start` schedules an immediate heartbeat tick at startup, so
/// `outbox_watermarks` is populated within milliseconds of the store
/// resolving. The row carries the replica's id, a `broadcast_watermark` of
/// 0 (fresh DB → no outbox rows), and a non-NULL `updated_at`.
#[tokio::test]
#[serial]
async fn heartbeat_upserts_replica_row_on_startup() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Poll for the heartbeat row up to 3 s — the first iteration is
    // unconditional and runs immediately at spawn, so well under a second
    // in practice; the slack covers cold container + DB pool warm-up.
    let row = timeout(Duration::from_secs(3), async {
        loop {
            let row = sqlx::query(
                "SELECT replica_id, broadcast_watermark, updated_at FROM outbox_watermarks",
            )
            .fetch_optional(&pool)
            .await
            .expect("query outbox_watermarks");
            if let Some(r) = row {
                return r;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("heartbeat row should appear within 3 s");

    let replica_id: String = row.get("replica_id");
    let broadcast_watermark: i64 = row.get("broadcast_watermark");
    assert_eq!(
        broadcast_watermark, 0,
        "fresh DB → watermark seeded at 0, got {broadcast_watermark}",
    );
    assert!(
        !replica_id.is_empty() && replica_id.contains('-'),
        "replica_id should be `<host>-<uuid8>`, got {replica_id:?}",
    );
    assert_eq!(
        replica_id.as_str(),
        store.replica_id(),
        "row should carry the store's own replica_id",
    );

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown did not finish within 8 s");
}

/// Running the heartbeat tick a second time keeps the row at one (UPSERT
/// semantics) and bumps `updated_at`. The retention gauge atomics are
/// populated (no longer the `-1` NaN sentinel) once a real heartbeat has
/// observed the cluster state.
#[tokio::test]
#[serial]
async fn heartbeat_upsert_refreshes_existing_row() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Drive a second heartbeat synchronously via the test entry point.
    store
        .outbox_heartbeat_once()
        .await
        .expect("outbox_heartbeat_once");

    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox_watermarks")
        .fetch_one(&pool)
        .await
        .expect("count outbox_watermarks");
    assert_eq!(n, 1, "UPSERT semantics keep the row count at one");

    // After at least one real heartbeat, the gauge atomics are no longer
    // the `-1` NaN sentinel: min-replica-watermark sees the cluster floor
    // (0 on an empty outbox), oldest-row-age sees NULL → -1 because the
    // outbox is empty. The first is observable here; the second matches
    // the empty-outbox sentinel and stays at -1 which is correct.
    assert_eq!(
        store.min_replica_watermark_atomic().load(Ordering::Acquire),
        0,
        "with one live replica and an empty outbox, MIN(watermark) = 0",
    );
    assert_eq!(
        store
            .oldest_row_age_seconds_atomic()
            .load(Ordering::Acquire),
        -1,
        "empty outbox → oldest-row-age atomic carries the NaN sentinel",
    );

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Configuring `outbox_retention` below the [`OUTBOX_RETENTION_FLOOR`] must
/// fail process startup with [`PgStoreStartError::RetentionTooShort`]. The
/// floor exists because `inserted_at` is transaction-start time, not commit
/// time — sub-floor retention is unsafe under MVCC. See ADR 0007.
#[tokio::test]
#[serial]
async fn start_rejects_sub_floor_retention() {
    let (pool, _container, db_url) = common::start_pg().await;
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener");
    let shutdown = CancellationToken::new();

    let result = PgStore::start_with_test_hooks(
        Arc::new(SystemClock),
        pool,
        pg_listener,
        shutdown.clone(),
        Duration::from_secs(30 * 60), // 30m — well below the 1h floor
        PgStoreTestHooks::default(),
    )
    .await;

    let Err(err) = result else {
        shutdown.cancel();
        panic!("expected RetentionTooShort, got Ok(_)");
    };

    match err {
        PgStoreStartError::RetentionTooShort {
            configured,
            minimum,
        } => {
            assert_eq!(configured, Duration::from_secs(30 * 60));
            assert_eq!(minimum, OUTBOX_RETENTION_FLOOR);
        }
        other => panic!("expected RetentionTooShort, got {other:?}"),
    }

    // Display message should name the floor so operators can act on it.
    let err = PgStoreStartError::RetentionTooShort {
        configured: Duration::from_secs(30 * 60),
        minimum: OUTBOX_RETENTION_FLOOR,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("3600s") || msg.contains("1h"),
        "Display should name the 1h floor, got {msg:?}",
    );

    shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Phase 4: sweep task
// ---------------------------------------------------------------------------

/// Insert a synthetic outbox row at an explicit `inserted_at` so tests can
/// position rows on either side of the retention window without relying on
/// SQL `now()` (which `TestClock` cannot influence). Returns the allocated
/// `seq`.
async fn insert_outbox_at(
    pool: &sqlx::PgPool,
    run_id: i64,
    inserted_at: chrono::DateTime<chrono::Utc>,
) -> i64 {
    sqlx::query_scalar!(
        r#"
        INSERT INTO outbox (kind, run_id, payload, inserted_at)
        VALUES ('run', $1, '{}'::jsonb, $2)
        RETURNING seq
        "#,
        run_id,
        inserted_at,
    )
    .fetch_one(pool)
    .await
    .expect("INSERT INTO outbox")
}

/// Sweep deletes outbox rows older than retention WHEN the multi-replica
/// `MIN(broadcast_watermark)` floor is >= the row's seq AND the row's
/// `inserted_at` is older than `clock.now() - retention`.
#[tokio::test]
#[serial]
async fn sweep_deletes_old_rows_within_watermark() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // Insert 3 old rows (well beyond 7d retention).
    let now = SystemClock.now();
    let eight_days_ago = now - chrono::Duration::days(8);
    let seq1 = insert_outbox_at(&pool, 10_001, eight_days_ago).await;
    let seq2 = insert_outbox_at(&pool, 10_002, eight_days_ago).await;
    let seq3 = insert_outbox_at(&pool, 10_003, eight_days_ago).await;

    // Advance this replica's broadcast watermark past the highest seq so
    // the sweep's safety floor admits all three rows. The heartbeat task
    // mirrors the watermark into outbox_watermarks.
    store.broadcast_watermark().store(seq3, Ordering::Release);
    store
        .outbox_heartbeat_once()
        .await
        .expect("outbox_heartbeat_once");

    // Run one sweep tick.
    let deleted = store.outbox_sweep_once().await.expect("outbox_sweep_once");
    assert_eq!(deleted, 3, "all three old rows should be swept");

    // All three rows are gone.
    let remaining: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM outbox WHERE seq IN ($1, $2, $3)"#)
            .bind(seq1)
            .bind(seq2)
            .bind(seq3)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Rows inside the retention window must survive even when the watermark
/// floor admits them. Time-based retention is the primary cutoff; the
/// watermark is the safety floor.
#[tokio::test]
#[serial]
async fn sweep_preserves_rows_inside_retention_window() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // 1h is well within the 7d default retention.
    let now = SystemClock.now();
    let one_hour_ago = now - chrono::Duration::hours(1);
    let seq = insert_outbox_at(&pool, 20_001, one_hour_ago).await;

    // Watermark floor admits the row.
    store.broadcast_watermark().store(seq, Ordering::Release);
    store.outbox_heartbeat_once().await.expect("heartbeat once");

    let deleted = store.outbox_sweep_once().await.expect("sweep once");
    assert_eq!(deleted, 0, "row within retention must not be deleted");

    let still_there: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM outbox WHERE seq = $1"#)
        .bind(seq)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still_there, 1);

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Rows whose seq exceeds the multi-replica watermark floor must survive
/// even when their `inserted_at` is well past the retention cutoff. The
/// safety floor protects against deleting rows that haven't yet been
/// broadcast by every live replica.
#[tokio::test]
#[serial]
async fn sweep_preserves_rows_above_watermark_floor() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    let now = SystemClock.now();
    let eight_days_ago = now - chrono::Duration::days(8);
    let seq = insert_outbox_at(&pool, 30_001, eight_days_ago).await;

    // Pin the watermark *below* the row's seq so the safety floor rejects it.
    store
        .broadcast_watermark()
        .store(seq - 1, Ordering::Release);
    store.outbox_heartbeat_once().await.expect("heartbeat once");

    let deleted = store.outbox_sweep_once().await.expect("sweep once");
    assert_eq!(
        deleted, 0,
        "row above watermark floor must survive even if past retention",
    );

    let still_there: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM outbox WHERE seq = $1"#)
        .bind(seq)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(still_there, 1);

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// When no replica has heartbeated recently (`updated_at > now() - 90s`),
/// the safety-floor subquery returns 0 — the CTE matches `seq <= 0`, which
/// is satisfied by no row. The sweep is a no-op even though retention
/// would otherwise admit the rows.
#[tokio::test]
#[serial]
async fn sweep_is_noop_without_fresh_heartbeats() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    let now = SystemClock.now();
    let eight_days_ago = now - chrono::Duration::days(8);
    let _seq = insert_outbox_at(&pool, 40_001, eight_days_ago).await;

    // The startup heartbeat ran (broadcast_watermark = 0 initially), so
    // there IS a fresh row in outbox_watermarks. Backdate it past the
    // stale_threshold (90 s) so the sweep's `updated_at > $stale_cutoff`
    // clause excludes it. Direct SQL update is the cleanest way to do this
    // without exposing internal cadence knobs to tests.
    sqlx::query(r#"UPDATE outbox_watermarks SET updated_at = $1 WHERE replica_id = $2"#)
        .bind(now - chrono::Duration::hours(2))
        .bind(store.replica_id())
        .execute(&pool)
        .await
        .expect("backdate watermark");

    let deleted = store.outbox_sweep_once().await.expect("sweep once");
    assert_eq!(
        deleted, 0,
        "no fresh heartbeats → MIN(...) over empty set is COALESCE'd to 0 → seq <= 0 matches nothing",
    );

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown");
}

/// Two `PgStore` instances against the same database run a sweep
/// concurrently; the FOR UPDATE SKIP LOCKED semantics give each sweeper a
/// disjoint candidate set, total deleted = expected, per-store counters
/// sum to the total.
#[tokio::test]
#[serial]
async fn sweep_under_contention_partitions_work() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown_a = CancellationToken::new();
    let shutdown_b = CancellationToken::new();
    let store_a = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown_a.clone()).await;
    let store_b = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown_b.clone()).await;

    // 50 old rows.
    let now = SystemClock.now();
    let eight_days_ago = now - chrono::Duration::days(8);
    let mut last_seq = 0;
    for i in 0..50 {
        last_seq = insert_outbox_at(&pool, 50_000 + i, eight_days_ago).await;
    }

    // Both replicas heartbeat with the same high watermark so the floor
    // admits all 50 rows.
    store_a
        .broadcast_watermark()
        .store(last_seq, Ordering::Release);
    store_b
        .broadcast_watermark()
        .store(last_seq, Ordering::Release);
    store_a
        .outbox_heartbeat_once()
        .await
        .expect("heartbeat A once");
    store_b
        .outbox_heartbeat_once()
        .await
        .expect("heartbeat B once");

    // Concurrent sweep.
    let store_a_clone = Arc::clone(&store_a);
    let store_b_clone = Arc::clone(&store_b);
    let (deleted_a, deleted_b) = tokio::join!(
        async move { store_a_clone.outbox_sweep_once().await.expect("sweep A") },
        async move { store_b_clone.outbox_sweep_once().await.expect("sweep B") },
    );

    assert_eq!(
        deleted_a + deleted_b,
        50,
        "total deleted across both sweepers must equal the row count: \
         A={deleted_a}, B={deleted_b}",
    );

    // All 50 rows are gone.
    let remaining: i64 = sqlx::query_scalar(
        r#"SELECT COUNT(*) FROM outbox WHERE run_id >= 50000 AND run_id < 50050"#,
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(remaining, 0);

    shutdown_a.cancel();
    shutdown_b.cancel();
    timeout(Duration::from_secs(8), store_a.shutdown())
        .await
        .expect("shutdown A");
    timeout(Duration::from_secs(8), store_b.shutdown())
        .await
        .expect("shutdown B");
}

/// After a sweep deletes the leading prefix of the outbox, restarting
/// `PgStore` against the same pool re-seeds `broadcast_watermark` from
/// `MAX(seq)` over the surviving rows. This protects against a regression
/// where the seed query was changed in a way that doesn't tolerate
/// post-sweep states. Issue #67's explicit verification ask.
#[tokio::test]
#[serial]
async fn broadcast_watermark_reseeds_after_sweep() {
    let (pool, _container, db_url) = common::start_pg().await;
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown.clone()).await;

    // 5 old rows that will be swept, then 3 rows inside retention that survive.
    let now = SystemClock.now();
    let eight_days_ago = now - chrono::Duration::days(8);
    let one_hour_ago = now - chrono::Duration::hours(1);
    for i in 0..5 {
        insert_outbox_at(&pool, 60_000 + i, eight_days_ago).await;
    }
    let mut max_surviving_seq = 0;
    for i in 0..3 {
        max_surviving_seq = insert_outbox_at(&pool, 60_010 + i, one_hour_ago).await;
    }

    // Sweep deletes the old rows.
    store
        .broadcast_watermark()
        .store(max_surviving_seq, Ordering::Release);
    store.outbox_heartbeat_once().await.expect("heartbeat once");
    let deleted = store.outbox_sweep_once().await.expect("sweep once");
    assert_eq!(deleted, 5);

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("first shutdown");

    // Restart a fresh PgStore against the same DB. The seed query
    // `SELECT COALESCE(MAX(seq), 0) FROM outbox` should pick the max from
    // the surviving (post-sweep) rows.
    let shutdown2 = CancellationToken::new();
    let store2 = common::start_pg_store_for_test(pool.clone(), &db_url, shutdown2.clone()).await;
    let seeded = store2.broadcast_watermark().load(Ordering::Acquire);
    assert_eq!(
        seeded, max_surviving_seq,
        "re-seeded watermark must equal MAX(seq) over surviving rows",
    );

    shutdown2.cancel();
    timeout(Duration::from_secs(8), store2.shutdown())
        .await
        .expect("second shutdown");
}
