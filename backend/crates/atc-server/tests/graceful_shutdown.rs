//! Integration test for two-phase cooperative graceful shutdown.
//!
//! Spins up a full PG-mode server with real TCP listeners, connects an idle
//! WebSocket client via tokio-tungstenite, fires `shutdown.cancel()` from the
//! test, and asserts:
//!
//! 1. The WS client receives `Message::Close(_)` with code 1001 within the
//!    WS phase-2 budget (~2.5 s).
//! 2. All background-task handles plus both spawned serve tasks complete within
//!    the aggregate shutdown budget (~13 s).
//!
//! Docker/OrbStack required. Set `DOCKER_HOST=unix://$HOME/.orbstack/run/docker.sock`
//! if using OrbStack.

mod common;

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serial_test::serial;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use atc_core::{RunStateMachine, SystemClock};
use atc_server::listener;
use atc_server::metrics;
use atc_server::persist::PgStore;
use atc_server::routes;
use atc_server::shutdown::{
    SHUTDOWN_TIMEOUT_DRAIN, SHUTDOWN_TIMEOUT_EVICTION, SHUTDOWN_TIMEOUT_LISTENER,
    SHUTDOWN_TIMEOUT_METRICS, SHUTDOWN_TIMEOUT_SERVES, SHUTDOWN_TIMEOUT_WS,
    run_shutdown_orchestration,
};
use atc_server::state::{AppState, SeqEvent};

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Full-stack fixture with real TCP listeners, PG pool, and all background tasks.
struct FullServerFixture {
    main_addr: SocketAddr,
    shutdown: CancellationToken,
    orchestration_handle: JoinHandle<()>,
}

async fn start_full_server(pool: sqlx::PgPool, db_url: String) -> FullServerFixture {
    let layer = common::PROMETHEUS_INIT
        .get_or_init(common::install_test_recorder)
        .0
        .clone();

    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    // Keepalive clone: see run_shutdown_orchestration for why this is required.
    let webhook_tx_keepalive = webhook_tx.clone();
    let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));
    let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()));
    let broadcast_watermark = Arc::new(AtomicI64::new(0));
    let drain_in_flight = Arc::new(AtomicBool::new(false));
    let startup_at = Instant::now();

    // Initialize watermark (same logic as main.rs).
    let initial_watermark: i64 =
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(MAX(seq), 0) FROM outbox")
            .fetch_one(&pool)
            .await
            .expect("watermark query failed");
    broadcast_watermark.store(initial_watermark, std::sync::atomic::Ordering::Release);

    let seq = Arc::new(tokio::sync::Mutex::new(0u64));
    let persist =
        Arc::new(PgStore::new(pool.clone())) as Arc<dyn atc_server::persist::PersistentStore>;

    let ws_close = CancellationToken::new();
    let ws_tracker = TaskTracker::new();

    let state = Arc::new(AppState {
        state_machine,
        webhook_tx,
        webhook_secret: None,
        seq,
        pg_pool: Some(pool.clone()),
        min_pending_seq: min_pending_seq.clone(),
        last_drain_pass_at,
        broadcast_watermark: broadcast_watermark.clone(),
        persist,
        ws_close: ws_close.clone(),
        ws_tracker: ws_tracker.clone(),
    });

    let shutdown = CancellationToken::new();
    let eviction_handle = state
        .state_machine
        .start_eviction_task(Duration::from_secs(60), shutdown.clone());
    let metrics_handle = metrics::spawn_process_collector(shutdown.clone());

    let pg_listener = listener::connect_listener(&db_url)
        .await
        .expect("connect_listener failed");
    let drain_notify = Arc::new(tokio::sync::Notify::new());
    let listener_handle = listener::spawn_listener_task(
        pg_listener,
        drain_notify.clone(),
        min_pending_seq.clone(),
        drain_in_flight.clone(),
        shutdown.clone(),
        None,
    );
    let drain_handle = listener::spawn_drain_task(
        pool,
        initial_watermark,
        startup_at,
        drain_notify,
        min_pending_seq,
        state.last_drain_pass_at.clone(),
        broadcast_watermark,
        drain_in_flight,
        state.webhook_tx.clone(),
        shutdown.clone(),
        None,
        None,
        None,
    );

    let app = routes::api_routes(layer)
        .with_state(state)
        .fallback(atc_server::assets::fallback_handler());

    // Bind to an ephemeral port so tests don't conflict.
    let main_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind main listener");
    let main_addr = main_listener.local_addr().expect("local_addr");
    // Bind a second ephemeral port for the metrics server. The global Prometheus
    // recorder is already installed via PROMETHEUS_INIT; we use a minimal stub
    // router rather than calling metrics::build() which would panic on
    // double-install.
    let metrics_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind metrics listener");
    // Minimal stub: the metrics endpoint is not under test here; it just needs
    // to be present so the orchestration's serves-join path has something to await.
    let metrics_router = axum::Router::new();

    let main_serve =
        axum::serve(main_listener, app).with_graceful_shutdown(shutdown.clone().cancelled_owned());
    let metrics_serve = axum::serve(metrics_listener, metrics_router)
        .with_graceful_shutdown(shutdown.clone().cancelled_owned());

    let main_serve_task = tokio::spawn(main_serve.into_future());
    let metrics_serve_task = tokio::spawn(metrics_serve.into_future());

    let shutdown_clone = shutdown.clone();
    let orchestration_handle = tokio::spawn(run_shutdown_orchestration(
        shutdown_clone,
        ws_close,
        ws_tracker,
        webhook_tx_keepalive,
        main_serve_task,
        metrics_serve_task,
        Some(drain_handle),
        Some(listener_handle),
        eviction_handle,
        metrics_handle,
    ));

    FullServerFixture {
        main_addr,
        shutdown,
        orchestration_handle,
    }
}

/// An idle connected WS client receives Close(1001) within the WS phase-2
/// budget, and the entire orchestration completes within the aggregate budget.
#[tokio::test]
#[serial]
async fn idle_ws_client_receives_close_within_budget() {
    let (pool, _container, db_url) = common::start_pg().await;
    let fixture = start_full_server(pool, db_url).await;

    // Connect an idle WS client.
    let ws_url = format!("ws://{}/v1/ws", fixture.main_addr);
    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Give the handler time to subscribe and enter the select loop.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Fire phase-1 shutdown from the test.
    fixture.shutdown.cancel();

    // Phase 2 fires automatically inside the orchestration (after drain completes).
    // Assert WS client receives Close(1001) within SHUTDOWN_TIMEOUT_WS + epsilon.
    let close_budget = SHUTDOWN_TIMEOUT_WS + Duration::from_millis(500);
    let frame = timeout(close_budget, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(f))) => return f,
                Some(Ok(_)) => {} // skip stray frames
                Some(Err(e)) => panic!("WebSocket error waiting for close: {e}"),
                None => panic!("connection dropped without a Close frame"),
            }
        }
    })
    .await
    .expect("timed out waiting for Close(1001) from server");

    let close = frame.expect("Close frame should carry a CloseFrame payload");
    assert_eq!(
        u16::from(close.code),
        1001,
        "close code should be 1001 (Going Away), got {:?}",
        close.code
    );

    // Assert the entire orchestration completes within the aggregate shutdown budget.
    // Budget: drain(5) + ws(2) + serves(3) + listener(1) + eviction(1) + metrics(1) + slop
    let aggregate_budget = SHUTDOWN_TIMEOUT_DRAIN
        + SHUTDOWN_TIMEOUT_WS
        + SHUTDOWN_TIMEOUT_SERVES
        + SHUTDOWN_TIMEOUT_LISTENER
        + SHUTDOWN_TIMEOUT_EVICTION
        + SHUTDOWN_TIMEOUT_METRICS
        + Duration::from_secs(2); // slop for test harness overhead

    timeout(aggregate_budget, fixture.orchestration_handle)
        .await
        .expect("orchestration did not complete within aggregate shutdown budget")
        .expect("orchestration task should not panic");
}
