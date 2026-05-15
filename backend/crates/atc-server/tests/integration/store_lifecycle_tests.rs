//! Lifecycle and event-emission tests for `PersistentStore` implementations.
//!
//! Covers the trait-level `subscribe()` and `shutdown()` extension plus the
//! per-impl `start()` / `start_with_test_hooks()` / `new_for_test()`
//! constructors that own background tasks internally.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::SystemClock;
use atc_core::event::{RunEvent, RunEventEnvelope};
use atc_core::fixed_test_timestamp;
use atc_core::types::RunId;
use atc_server::listener;
use atc_server::persist::{InMemoryStore, PersistentStore, PgStore};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

fn run_requested(run_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        run_id: RunId(run_id),
        org: "test-org".to_string(),
        repo: "test-repo".to_string(),
        workflow_name: Some("CI".to_string()),
        workflow_path: Some(".github/workflows/ci.yml".to_string()),
        branch: Some("main".to_string()),
        head_sha: "abc123".to_string(),
        commit_message: Some("Initial commit".to_string()),
        trigger_event: "push".to_string(),
        display_title: "Test run".to_string(),
        html_url: format!("https://github.com/test-org/test-repo/actions/runs/{run_id}"),
        created_at: fixed_test_timestamp(),
        run_started_at: None,
        updated_at: fixed_test_timestamp(),
        action: RunEvent::Requested,
    }
}

// ---------------------------------------------------------------------------
// InMemoryStore::start — owns broadcast sender and eviction task internally
// ---------------------------------------------------------------------------

/// `InMemoryStore::start()` returns an `Arc<Self>` whose `subscribe()` yields a
/// receiver that observes `apply_run_event` broadcasts.
#[tokio::test]
async fn in_memory_start_subscribe_observes_apply() {
    let shutdown = CancellationToken::new();
    let store = InMemoryStore::start(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        Duration::from_mins(1),
        shutdown.clone(),
    );

    let mut rx = store.subscribe();
    let env = run_requested(9_000_001);
    store.apply_run_event(env).await.expect("apply_run_event");

    let event = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("broadcast did not arrive within 1s")
        .expect("broadcast channel closed");
    assert_eq!(event.seq, 1, "first event should carry seq=1");

    // Shut down to reclaim the eviction task before the test exits.
    // `shutdown.cancel()` signals cooperative exit; `store.shutdown().await`
    // joins the task within its per-task timeout.
    shutdown.cancel();
    store.shutdown().await;
}

/// `InMemoryStore::new_for_test` lets tests pick a custom broadcast capacity
/// without spawning the eviction task, so lagging-client tests can force
/// `RecvError::Lagged` with a handful of events instead of 256.
#[tokio::test]
async fn in_memory_new_for_test_uses_custom_capacity() {
    let store = InMemoryStore::new_for_test(Arc::new(SystemClock), Duration::from_hours(1), 2);

    let mut rx = store.subscribe();
    for n in 0..3 {
        store
            .apply_run_event(run_requested(9_000_100 + n))
            .await
            .expect("apply_run_event");
    }

    // Third event should overrun the capacity-2 buffer for the unread receiver.
    let res = timeout(Duration::from_secs(1), rx.recv()).await;
    let res = res.expect("recv timed out").expect_err("expected Lagged");
    assert!(
        matches!(res, tokio::sync::broadcast::error::RecvError::Lagged(_)),
        "expected RecvError::Lagged, got {res:?}",
    );
}

/// `InMemoryStore::shutdown` is idempotent: a second call returns immediately
/// without panicking on the already-taken handle.
#[tokio::test]
async fn in_memory_shutdown_is_idempotent() {
    let shutdown = CancellationToken::new();
    let store = InMemoryStore::start(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        Duration::from_mins(1),
        shutdown.clone(),
    );

    // First call joins after cancel; second call observes None and is a no-op.
    shutdown.cancel();
    store.shutdown().await;
    timeout(Duration::from_secs(2), store.shutdown())
        .await
        .expect("second shutdown should return immediately");
}

// ---------------------------------------------------------------------------
// PgStore::start_with_test_hooks — owns listener + drain tasks internally
// ---------------------------------------------------------------------------

/// `PgStore::start_with_test_hooks` returns the store plus a handle struct
/// carrying abort handles for the listener and drain, and the watermark /
/// heartbeat atomics tests poke into.
#[tokio::test]
#[serial_test::serial]
async fn pg_start_with_test_hooks_exposes_handles() {
    let (pool, _c, db_url) = common::start_pg().await;
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener");
    let shutdown = CancellationToken::new();

    let (store, handles) = PgStore::start_with_test_hooks(
        Arc::new(SystemClock),
        pool,
        pg_listener,
        shutdown.clone(),
        Duration::from_secs(7 * 24 * 60 * 60),
        Default::default(),
    )
    .await
    .expect("start_with_test_hooks");

    // Apply one run event so the drain has something to broadcast.
    let mut rx = store.subscribe();
    store
        .apply_run_event(run_requested(9_001_001))
        .await
        .expect("apply_run_event");
    let event = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("broadcast did not arrive within 5s")
        .expect("broadcast closed");
    assert!(event.seq >= 1, "expected positive seq, got {}", event.seq);

    // The drain advances broadcast_watermark on a successful pass.
    let watermark_after_broadcast = handles
        .broadcast_watermark
        .load(std::sync::atomic::Ordering::Acquire);
    assert!(
        watermark_after_broadcast >= 1,
        "expected broadcast_watermark to advance after broadcast, got {watermark_after_broadcast}",
    );

    // Cancel the caller's token so the spawned tasks observe shutdown, then
    // join via `store.shutdown()` within the per-task budget.
    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown did not finish within 8s");

    // last_drain_pass_at was refreshed; abort handles still exist.
    let last = handles
        .last_drain_pass_at
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(last > 0, "last_drain_pass_at should have been refreshed");
    // Touching the abort handles after shutdown is a no-op but must compile.
    handles.drain_abort.abort();
    handles.listener_abort.abort();
}

/// `PgStore::shutdown` is idempotent: a second call observes the consumed
/// handles and returns immediately.
#[tokio::test]
#[serial_test::serial]
async fn pg_shutdown_is_idempotent() {
    let (pool, _c, db_url) = common::start_pg().await;
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener");
    let shutdown = CancellationToken::new();
    let (store, _handles) = PgStore::start_with_test_hooks(
        Arc::new(SystemClock),
        pool,
        pg_listener,
        shutdown.clone(),
        Duration::from_secs(7 * 24 * 60 * 60),
        Default::default(),
    )
    .await
    .expect("start_with_test_hooks");

    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("first shutdown");
    timeout(Duration::from_secs(2), store.shutdown())
        .await
        .expect("second shutdown should return immediately");
}

/// Aborting the drain through the test handle is treated as a clean exit by
/// `PgStore::shutdown` — the cancelled-task path logs at `warn`, not `error`,
/// and does not panic.
#[tokio::test]
#[serial_test::serial]
async fn pg_shutdown_handles_aborted_drain_cleanly() {
    let (pool, _c, db_url) = common::start_pg().await;
    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener");
    let shutdown = CancellationToken::new();
    let (store, handles) = PgStore::start_with_test_hooks(
        Arc::new(SystemClock),
        pool,
        pg_listener,
        shutdown.clone(),
        Duration::from_secs(7 * 24 * 60 * 60),
        Default::default(),
    )
    .await
    .expect("start_with_test_hooks");

    // Abort the drain externally — the typical `readyz` test pattern.
    handles.drain_abort.abort();
    // Give the runtime a beat to mark the task as cancelled.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // shutdown should join both tasks (drain returns JoinError::is_cancelled
    // — handled cleanly per ADR-0006). Cancel the token so the listener
    // observes shutdown too, then join.
    shutdown.cancel();
    timeout(Duration::from_secs(8), store.shutdown())
        .await
        .expect("shutdown should not hang on an aborted drain task");
}

// ---------------------------------------------------------------------------
// subscribe parity across implementations
// ---------------------------------------------------------------------------

/// The trait-level `subscribe()` works behind `Arc<dyn PersistentStore>`.
#[tokio::test]
async fn subscribe_via_trait_object() {
    let shutdown = CancellationToken::new();
    let store: Arc<dyn PersistentStore> = InMemoryStore::start(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        Duration::from_mins(1),
        shutdown.clone(),
    );

    let mut rx = store.subscribe();
    store
        .apply_run_event(run_requested(9_002_001))
        .await
        .expect("apply_run_event");
    let event = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("broadcast did not arrive within 1s")
        .expect("broadcast channel closed");
    assert_eq!(event.seq, 1);

    shutdown.cancel();
    store.shutdown().await;
}
