//! Integration tests: row-lock serialization for same-entity concurrent commits.
//!
//! Two concurrent webhooks for the SAME run entity are serialized by PG row
//! locking. Both use `workflow_run.requested` (idempotent same-status replay),
//! so both must succeed (status="accepted") and both must produce outbox rows.
//! The drain broadcasts both in strictly increasing seq order.
//!
//! Deliberately NOT asserted: that the gap-healing rescan never fires
//! (`atc_pg_drain_duplicate_skipped_total` staying at zero). Under load a
//! delayed NOTIFY can push processing past the rescan backstop, producing a
//! legitimate duplicate skip without violating any ordering property — the
//! dedup ring is what keeps the broadcast stream correct in that case. See
//! #519 for the observed flake.
//!
//! Docker/OrbStack required.

use crate::common;

use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

/// Two concurrent `workflow_run.requested` webhooks for the SAME run_id are
/// serialized by PG row-level locking.
///
/// Both use the `requested` action which targets status=Queued. predecessors_of(Queued)
/// includes Queued itself, so the second committer performs an idempotent same-status
/// replay — both transactions succeed and each writes an outbox row.
///
/// The drain broadcasts both in durable outbox.seq order (strictly increasing),
/// proving the PG row-lock argument from §D3 of the design plan: same-entity
/// serialization yields ordered, exactly-once broadcasts. Whether the
/// gap-healing rescan fires along the way is a load-dependent implementation
/// detail (see the module doc) and is not asserted.
#[tokio::test]
#[serial]
async fn concurrent_same_entity_commits_in_seq_order() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;
    let mut rx = fixture.state.persist.subscribe();

    let router_a = fixture.router.clone();
    let router_b = fixture.router.clone();

    // Both webhooks use `requested` (idempotent same-status replay under the row-lock).
    let body_a = common::fixture_workflow_run_requested();
    let body_b = common::fixture_workflow_run_requested();

    // Fire both webhooks concurrently — same run entity.
    let (result_a, result_b) = tokio::join!(
        common::post_webhook_to_router(router_a, "workflow_run", &body_a),
        common::post_webhook_to_router(router_b, "workflow_run", &body_b),
    );

    let (status_a, body_a) = result_a;
    let (status_b, body_b) = result_b;

    // Both webhooks must return 200.
    assert_eq!(status_a, StatusCode::OK, "webhook A must return 200");
    assert_eq!(status_b, StatusCode::OK, "webhook B must return 200");

    // Both must be accepted — idempotent same-status replays succeed under PG row-lock.
    let status_a_str = body_a["status"].as_str().unwrap_or("unknown");
    let status_b_str = body_b["status"].as_str().unwrap_or("unknown");
    assert_eq!(
        status_a_str, "accepted",
        "webhook A must be accepted (idempotent requested replay); body={body_a}"
    );
    assert_eq!(
        status_b_str, "accepted",
        "webhook B must be accepted (idempotent requested replay); body={body_b}"
    );

    // Collect 2 CommittedEvents from the drain (one per committed outbox row).
    let mut broadcast_seqs: Vec<u64> = Vec::new();
    timeout(Duration::from_secs(10), async {
        while broadcast_seqs.len() < 2 {
            match rx.recv().await {
                Ok(ev) => broadcast_seqs.push(ev.seq),
                Err(_) => break,
            }
        }
    })
    .await
    .expect("timed out waiting for 2 drain broadcasts");

    // Drain must broadcast exactly 2 events (one per outbox row).
    assert_eq!(
        broadcast_seqs.len(),
        2,
        "drain must broadcast exactly 2 CommittedEvents; got {broadcast_seqs:?}"
    );

    // Seqs must be strictly monotonically increasing — drain emits in ORDER BY seq.
    assert!(
        broadcast_seqs[0] < broadcast_seqs[1],
        "drain seqs must be strictly increasing; got {broadcast_seqs:?}"
    );

    // Both seqs must be positive.
    for &seq in &broadcast_seqs {
        assert!(seq > 0, "all broadcast seqs must be positive; got {seq}");
    }

    // Outbox must contain exactly 2 rows (one per accepted commit).
    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("outbox count query failed");
    assert_eq!(
        outbox_count, 2,
        "outbox must contain exactly 2 rows (one per accepted commit)"
    );

    // In PG mode the broadcast watermark advances via the drain, not in-memory.
    // The fixture's last_drain_pass_at should be a recent timestamp (drain ran).
    let drain_heartbeat = fixture
        .last_drain_pass_at
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        drain_heartbeat > 0,
        "drain heartbeat must be non-zero after processing events"
    );

    fixture.shutdown.cancel();
}

/// The outbox contains exactly one row per accepted commit — no phantom rows
/// from failed or concurrent duplicate commits.
#[tokio::test]
#[serial]
async fn outbox_row_count_matches_accepted_commits() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;

    // Fire run + job webhooks (distinct entities, no conflict).
    let (s1, b1) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(b1["status"], "accepted");

    let (s2, b2) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(b2["status"], "accepted");

    // Query the outbox directly.
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .expect("outbox count query failed");

    assert_eq!(
        count, 2,
        "outbox must contain exactly 2 rows (one per accepted commit)"
    );

    fixture.shutdown.cancel();
}
