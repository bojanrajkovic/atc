//! Integration tests: drain task forwards outbox events to WS clients.
//!
//! Covers the PG-mode broadcast pipeline: the webhook handler commits to PG
//! and is write-only (no direct broadcast via `webhook_tx`); the drain task
//! is the sole writer to the broadcast channel and forwards outbox rows in
//! seq order.
//!
//! Docker/OrbStack required.

mod common;

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::StatusCode;
use serial_test::serial;
use tokio::time::timeout;

// ─── drain broadcasts in delivery order ───────────────────────────────────

/// Events committed via the PG webhook handler are broadcast by the drain
/// in outbox seq order (1, 2, …).
///
/// Fires a run webhook (→ seq=1) then a job webhook (→ seq=2). Asserts both
/// SeqEvents arrive at broadcast_rx with the correct seq numbers and in order.
#[tokio::test]
#[serial]
async fn drain_broadcasts_seq_in_order() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool, db_url).await;
    let mut rx = fixture.state.webhook_tx.subscribe();

    // Fire run webhook (seq=1).
    let (status1, body1) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status1, StatusCode::OK);
    // PG-mode handler returns {"status":"accepted","seq":1}.
    assert_eq!(
        body1["status"], "accepted",
        "PG-mode handler should return accepted"
    );
    assert_eq!(
        body1["seq"], 1,
        "first event should be assigned seq=1 by PG"
    );

    // Fire job webhook (seq=2).
    let (status2, body2) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status2, StatusCode::OK);
    assert_eq!(body2["status"], "accepted");
    assert_eq!(body2["seq"], 2, "second event should be seq=2");

    // Await seq=1 from the drain.
    let ev1 = timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.seq == 1 => return ev,
                Ok(_) => continue,
                Err(_) => panic!("broadcast channel closed"),
            }
        }
    })
    .await
    .expect("timed out waiting for seq=1 from drain");

    // Await seq=2 from the drain.
    let ev2 = timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.seq == 2 => return ev,
                Ok(_) => continue,
                Err(_) => panic!("broadcast channel closed"),
            }
        }
    })
    .await
    .expect("timed out waiting for seq=2 from drain");

    // Assert ordering: seq numbers must be strictly increasing.
    assert!(
        ev1.seq < ev2.seq,
        "drain must broadcast seq=1 before seq=2; got seq={} then seq={}",
        ev1.seq,
        ev2.seq
    );

    fixture.shutdown.cancel();
}

// ─── handler does NOT broadcast in PG mode ────────────────────────────────

/// In PG mode the webhook handler is write-only — it does NOT send to the
/// broadcast channel directly. The drain task is the SOLE writer to
/// `webhook_tx`.
///
/// Determinism strategy: inject a 500 ms `drain_delay` so each drain pass
/// sleeps before querying the outbox. After the handler returns 200, there is
/// a guaranteed window (≪500 ms) during which the drain has not yet
/// processed the row — any event in the channel during that window must have
/// come from the handler. The window is long enough for tokio to schedule
/// the drain task if it wanted to broadcast (the drain wakes on `notified()`
/// inside the select; the delay is awaited AFTER the wake), so the proof
/// of absence is real and not just a scheduling race.
#[tokio::test]
#[serial]
async fn handler_silent_in_pg_mode() {
    let (pool, _container, db_url) = common::start_pg().await;
    // Slow the drain so the handler's return strictly precedes any drain
    // broadcast for the row it just committed. drain_delay sleeps at the
    // start of each drain_pass — the listener wakes the drain on NOTIFY,
    // the drain enters drain_pass, then sleeps drain_delay before SELECT.
    let fixture = common::build_app_with_pg_and_slow_drain(
        pool,
        db_url,
        std::time::Duration::from_millis(500),
    )
    .await;

    // Subscribe AFTER fixture build so the startup pass (which runs
    // immediately in build_app_inner before the delay setting matters) has
    // already cleared. The fixture's drain_started signal fires after that.
    let mut rx = fixture.state.webhook_tx.subscribe();

    // Fire webhook and await HTTP 200.
    let (status, body) = common::post_webhook_to_router(
        fixture.router.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["status"], "accepted",
        "PG-mode handler should return accepted, not processed"
    );

    // Window: between handler-return and drain-broadcast there is a
    // guaranteed 500 ms gap (drain_delay). Sleep 100 ms — well inside the
    // gap — and assert the channel is empty. If the handler broadcast,
    // the event would be here.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let immediate = rx.try_recv();
    assert!(
        immediate.is_err(),
        "handler must not broadcast in PG mode; saw event during drain_delay window: {immediate:?}",
    );

    // Wait past the delay for the drain to broadcast. Confirms the full
    // pipe works — silence is meaningful only if the drain DOES broadcast
    // afterward.
    let drain_ev = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for drain to broadcast")
        .expect("broadcast channel closed");
    assert_eq!(
        drain_ev.seq, 1,
        "drain should broadcast seq=1 (first event)"
    );

    // After the drain broadcasts seq=1, no second event is expected for
    // this webhook. Wait one more drain_delay window and assert silence.
    let extra = timeout(Duration::from_millis(700), rx.recv()).await;
    assert!(
        extra.is_err(),
        "no second broadcast expected (drain is the only writer); got {extra:?}",
    );

    // Confirm the seq counter in AppState was NOT incremented by the handler
    // (it stays at 0 in PG mode — only the drain path advances it via
    // BIGSERIAL, not the in-memory seq mutex).
    let seq_val = *fixture.state.seq.lock().await;
    assert_eq!(
        seq_val, 0,
        "in PG mode, the handler must not increment the in-memory seq counter"
    );

    // observed_recv (listener notifications) should be at least 1.
    let received = fixture.observed_recv.load(Ordering::Relaxed);
    assert!(
        received >= 1,
        "listener should have received at least one NOTIFY; got {received}"
    );

    fixture.shutdown.cancel();
}
