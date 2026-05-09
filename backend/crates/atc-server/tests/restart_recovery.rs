//! Integration tests: restart recovery (no historical replay).
//!
//! T10 — After a simulated server restart (second fixture against same PG),
//!       the new drain task does NOT replay events already committed before the
//!       restart. The watermark is initialized to COALESCE(MAX(seq), 0) at boot,
//!       so historical outbox rows are skipped.
//!
//! Docker/OrbStack required.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

/// T10: After restart, the new drain task initializes its watermark from
///      MAX(seq) in the outbox and does NOT rebroadcast historical events.
///
/// Protocol:
///   1. Start fixture f1 (pool + listener). Fire two webhooks → seq=1, seq=2.
///   2. Await drain broadcasts for seq=1 and seq=2 from f1.
///   3. Cancel f1 and await both task handles — simulating a clean shutdown.
///   4. Build fixture f2 against the SAME PG pool. Subscribe to its broadcast.
///   5. Trigger one new webhook → seq=3. The drain in f2 should only broadcast
///      seq=3, NOT seq=1 or seq=2 (historical replay must not happen).
///   6. Assert no seq=1 or seq=2 events arrive in f2's channel. Assert seq=3
///      arrives within the timeout.
#[tokio::test]
#[serial]
async fn t10_no_historical_replay_after_restart() {
    let (pool, _container, db_url) = common::start_pg().await;

    // ── Step 1: f1 — commit two events ───────────────────────────────────────
    let f1 = common::build_app_with_pg_and_listener(pool.clone(), db_url.clone()).await;
    let mut rx1 = f1.state.webhook_tx.subscribe();

    let (s1, b1) = common::post_webhook_to_router(
        f1.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(b1["seq"], 1, "first event should be seq=1");

    let (s2, b2) = common::post_webhook_to_router(
        f1.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b2["seq"], 2, "second event should be seq=2");

    // Await drain broadcasts for both events.
    let mut got_seq1 = false;
    let mut got_seq2 = false;
    timeout(Duration::from_secs(10), async {
        while !got_seq1 || !got_seq2 {
            match rx1.recv().await {
                Ok(ev) => {
                    if ev.seq == 1 {
                        got_seq1 = true;
                    }
                    if ev.seq == 2 {
                        got_seq2 = true;
                    }
                }
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timed out waiting for seq=1 and seq=2 from f1");

    assert!(got_seq1, "f1 drain must have broadcast seq=1");
    assert!(got_seq2, "f1 drain must have broadcast seq=2");

    // ── Step 2: simulate clean shutdown of f1 ────────────────────────────────
    f1.shutdown.cancel();
    timeout(Duration::from_secs(5), async {
        let _ = tokio::join!(f1.listener_handle, f1.drain_handle);
    })
    .await
    .expect("f1 tasks did not shut down within 5s");

    // ── Step 3: f2 — fresh instance against the same PG ─────────────────────
    let f2 = common::build_app_with_pg_and_listener(pool.clone(), db_url.clone()).await;
    let mut rx2 = f2.state.webhook_tx.subscribe();

    // ── Step 4: fire a new event → seq=3 ─────────────────────────────────────
    // Fire workflow_run_completed (updates the existing run to Completed).
    let (s3, b3) = common::post_webhook_to_router(
        f2.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_completed(),
    )
    .await;
    assert_eq!(s3, StatusCode::OK);
    assert_eq!(b3["status"], "accepted");
    let seq3 = b3["seq"].as_i64().expect("seq should be i64");
    assert_eq!(seq3, 3, "third event should be seq=3");

    // ── Step 5: assert no historical replay, seq=3 arrives ──────────────────
    // Collect events for up to 5 seconds.
    let mut historical_replays: Vec<u64> = Vec::new();
    let mut got_seq3 = false;

    timeout(Duration::from_secs(5), async {
        while !got_seq3 {
            match rx2.recv().await {
                Ok(ev) => {
                    if ev.seq == 1 || ev.seq == 2 {
                        historical_replays.push(ev.seq);
                    }
                    if ev.seq == 3 {
                        got_seq3 = true;
                    }
                }
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timed out waiting for seq=3 from f2");

    assert!(
        got_seq3,
        "f2 drain must broadcast seq=3 (new event after restart)"
    );
    assert!(
        historical_replays.is_empty(),
        "f2 must NOT replay historical events (seq=1, seq=2); replayed: {historical_replays:?}"
    );

    f2.shutdown.cancel();
}
