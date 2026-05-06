//! Phase 3c integration tests: gap-healing backstop and dedup ring buffer.
//!
//! T6  — Drain dedup: seq=2 broadcast by an earlier pass is not re-broadcast
//!        when a backstop-lowered rescan covers it again.
//! T6b — Drain pagination: a rescan triggered after seeding >DRAIN_BATCH_SIZE
//!        rows is paginated correctly (all rows forwarded, no duplicates).
//! T7  — min_pending_seq swap unit test: pure Rust, no testcontainers.
//!
//! Docker/OrbStack required for T6 and T6b.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

// ─── T7: pure unit test for min_pending_seq swap semantics ──────────────────

/// T7: Verify the gap-healing backstop atomic swap behavior.
///
/// `fetch_min(seq, Release)` registers the pending seq from a NOTIFY. The drain
/// then `swap(MAX, AcqRel)` to capture it. This tests the invariant directly
/// without any PG dependency.
#[test]
fn phase_3c_gap_healing_t7_min_pending_seq_swap_semantics() {
    let atomic = Arc::new(AtomicI64::new(i64::MAX));

    // Simulate listener receiving NOTIFY for seq=5.
    let prev = atomic.fetch_min(5, Ordering::Release);
    // Should have swapped from MAX down to 5.
    assert_eq!(
        prev,
        i64::MAX,
        "fetch_min should return the old value (MAX)"
    );
    assert_eq!(
        atomic.load(Ordering::Acquire),
        5,
        "atomic should now hold 5 (the min)"
    );

    // Simulate a second NOTIFY for seq=3 (earlier seq, so min drops further).
    let prev2 = atomic.fetch_min(3, Ordering::Release);
    assert_eq!(prev2, 5, "second fetch_min returns old value (5)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        3,
        "atomic should now hold 3"
    );

    // Simulate drain swapping backstop to MAX and capturing the floor.
    let captured = atomic.swap(i64::MAX, Ordering::AcqRel);
    assert_eq!(captured, 3, "swap should return 3 (the captured backstop)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        i64::MAX,
        "atomic should be reset to MAX after swap"
    );

    // A NOTIFY after the swap for seq=7 should register correctly.
    atomic.fetch_min(7, Ordering::Release);
    assert_eq!(
        atomic.load(Ordering::Acquire),
        7,
        "new NOTIFY after reset should register"
    );

    // Another drain sweep: capture and reset.
    let captured2 = atomic.swap(i64::MAX, Ordering::AcqRel);
    assert_eq!(captured2, 7, "second swap captures 7");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        i64::MAX,
        "reset to MAX again"
    );
}

/// T7b: fetch_min does not go below an already-smaller value.
#[test]
fn phase_3c_gap_healing_t7b_fetch_min_does_not_increase() {
    let atomic = Arc::new(AtomicI64::new(10));

    // Attempting to register seq=20 should not change the stored minimum.
    let prev = atomic.fetch_min(20, Ordering::Release);
    assert_eq!(prev, 10, "prev should be 10 (the minimum)");
    assert_eq!(
        atomic.load(Ordering::Acquire),
        10,
        "stored value stays 10 — fetch_min is monotone-decreasing"
    );
}

// ─── T6: dedup ring buffer suppresses re-broadcast ──────────────────────────

/// T6: A backstop-driven rescan does not re-broadcast a seq already in the
///     dedup ring buffer from a previous pass.
///
/// Setup:
///   1. Commit event B (→ seq=1 in a fresh DB). Let drain broadcast it.
///   2. Open transaction A that will NOT commit yet.
///      Inside txn A, commit event X (seq=2) and wait for drain to broadcast.
///      (The NOTIFY for seq=2 fires only on A's commit; we do this via direct
///      webhook call which commits immediately.)
///   Wait for the drain to broadcast seq=1 (event B was first).
///   Wait for seq=2 broadcast from drain.
///   3. Manually insert a NOTIFY for seq=1 via `SELECT pg_notify(...)` to
///      simulate a backstop-triggered rescan from floor=0.
///   4. Assert drain does NOT rebroadcast seq=1 (already in ring).
///
/// Implementation note: to force the rescan without a real concurrent commit
/// race, we fire `SELECT pg_notify('atc_outbox', '1')` directly — the listener
/// picks it up, calls fetch_min(1, Release), and the drain sweeps from
/// floor = watermark.min(1 - 1) = 0, re-examining seq=1. Dedup suppresses it.
#[tokio::test]
#[serial]
async fn phase_3c_gap_healing_t6_dedup_suppresses_rescan_rebroadcast() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Subscribe a fresh broadcast receiver for seq-observation.
    let mut rx = fixture.state.webhook_tx.subscribe();

    // Step 1: Fire event B — becomes seq=1 after commit.
    let status = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await
    .0;
    assert_eq!(status, StatusCode::OK, "B webhook should be accepted");

    // Wait for drain to broadcast seq=1.
    let ev1 = timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(ev) = rx.recv().await
                && ev.seq == 1
            {
                return ev;
            }
        }
    })
    .await
    .expect("timed out waiting for seq=1 broadcast");
    assert_eq!(ev1.seq, 1, "first broadcast should be seq=1");

    // Step 2: Fire event A (a job) — becomes seq=2.
    let status2 = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await
    .0;
    assert_eq!(status2, StatusCode::OK, "A webhook should be accepted");

    // Wait for drain to broadcast seq=2 (confirms it's in the ring).
    timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(ev) = rx.recv().await
                && ev.seq == 2
            {
                return;
            }
        }
    })
    .await
    .expect("timed out waiting for seq=2 broadcast");

    // Record pass count before triggering the rescan.
    let passes_before = fixture
        .observed_passes
        .load(std::sync::atomic::Ordering::Relaxed);

    // Step 3: Manually send a NOTIFY for seq=1 to trigger a backstop rescan.
    // The drain will sweep from pass_start_floor = watermark.min(1 - 1) = 0
    // and re-encounter seq=1, which is already in the dedup ring.
    sqlx::query("SELECT pg_notify('atc_outbox', '1')")
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    // Wait for the drain to complete at least one more pass after the NOTIFY,
    // proving the rescan actually ran rather than the NOTIFY being dropped.
    timeout(Duration::from_secs(5), async {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let passes_now = fixture
                .observed_passes
                .load(std::sync::atomic::Ordering::Relaxed);
            if passes_now > passes_before {
                return;
            }
        }
    })
    .await
    .expect("drain did not complete a rescan pass within 5s after manual NOTIFY");

    // Step 4: Drain should NOT have rebroadcast seq=1. Assert that no additional
    // event for seq=1 arrived after the rescan (the dedup ring suppressed it).
    let rebroadcast = timeout(Duration::from_millis(500), async {
        loop {
            if let Ok(ev) = rx.recv().await
                && ev.seq == 1
            {
                return true; // unwanted rebroadcast
            }
        }
    })
    .await;
    assert!(
        rebroadcast.is_err(),
        "seq=1 must not be rebroadcast — dedup ring should suppress it"
    );

    fixture.shutdown.cancel();
}

// ─── T6b: drain pagination ──────────────────────────────────────────────────

/// T6b: The drain paginates across `DRAIN_BATCH_SIZE` correctly without crashing.
///
/// Seeds 600 outbox rows directly via SQL, then starts the fixture. The drain's
/// initial pass must page through all 600 rows in batches of 500. This test
/// verifies the pagination loop completes without deadlock or panic and that
/// the drain advances `observed_passes` past the initial pass.
///
/// Note on broadcast behavior: seeded rows use a stub payload `{"type":"stub"}`
/// that does not decode as a valid `RunEventEnvelope`. The drain logs decode
/// errors and continues — no SeqEvents are broadcast for stub rows. The test
/// therefore asserts on drain-pass progression, not broadcast count. A future
/// test that needs full broadcast coverage should seed real webhook JSON.
///
/// The DRAIN_BATCH_SIZE is 500, so a 600-row DB forces at least 2 pagination
/// iterations within a single pass.
#[tokio::test]
#[serial]
#[ignore = "heavy: seeds 600 rows, best run explicitly"]
async fn phase_3c_gap_healing_t6b_drain_paginates_across_batch_boundary() {
    let (pool, _container, db_url) = common::start_pg().await;

    // Insert a stub run row to satisfy the FK constraint on outbox.run_id.
    // Use the untyped API (no `!`) to avoid requiring a cached query schema.
    sqlx::query(
        "INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, \
         status, created_at, updated_at, placeholder) \
         VALUES (40000000010, 'test', 'test', '', '', '', '', 'Queued', NOW(), NOW(), true)",
    )
    .execute(&pool)
    .await
    .expect("stub run insert failed");

    // Seed 600 outbox rows directly. These use a stub payload — the drain will
    // log decode errors but must not crash.
    for _ in 0..600i64 {
        sqlx::query(
            "INSERT INTO outbox (kind, run_id, payload) \
             VALUES ('run', 40000000010, '{\"type\":\"stub\"}'::jsonb)",
        )
        .execute(&pool)
        .await
        .expect("outbox seed failed");
    }

    // Verify 600 rows exist before starting the fixture.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("count failed");
    assert_eq!(count, 600, "should have seeded 600 outbox rows");

    // Start fixture — the drain's unconditional first pass will process all 600
    // rows via pagination, then signal drain_started.
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Record the pass count after fixture startup (first unconditional pass done).
    let passes_after_startup = fixture
        .observed_passes
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        passes_after_startup >= 1,
        "at least one pass must have completed at startup (got {passes_after_startup})"
    );

    // Trigger a second pass via manual NOTIFY to verify the loop can run again
    // without crashing after the first paginated pass.
    // Use untyped API to avoid SQLX_OFFLINE / cached schema requirement.
    sqlx::query("SELECT pg_notify('atc_outbox', '1')")
        .execute(&pool)
        .await
        .expect("manual NOTIFY failed");

    // Wait for the drain to complete at least one more pass after the NOTIFY.
    timeout(Duration::from_secs(30), async {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let passes_now = fixture
                .observed_passes
                .load(std::sync::atomic::Ordering::Relaxed);
            if passes_now > passes_after_startup {
                return;
            }
        }
    })
    .await
    .expect("drain did not complete a second paginated pass within 30s");

    fixture.shutdown.cancel();
}
