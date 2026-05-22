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

use crate::common;

use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serial_test::serial;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use atc_server::metrics;
use atc_server::routes;
use atc_server::shutdown::{
    SHUTDOWN_TIMEOUT_METRICS, SHUTDOWN_TIMEOUT_SERVES, SHUTDOWN_TIMEOUT_WS,
    run_shutdown_orchestration,
};
use atc_server::state::AppState;
use atc_store_mem::EVICTION_SHUTDOWN_TIMEOUT;
use atc_store_pg::store::{SHUTDOWN_TIMEOUT_DRAIN, SHUTDOWN_TIMEOUT_LISTENER};

/// Full-stack fixture with real TCP listeners, PG pool, and all background tasks.
struct FullServerFixture {
    main_addr: SocketAddr,
    shutdown: CancellationToken,
    orchestration_handle: JoinHandle<bool>,
}

async fn start_full_server(pool: atc_store_pg::TracedPool, db_url: String) -> FullServerFixture {
    common::ensure_recorder_installed();

    let shutdown = CancellationToken::new();
    let ws_tracker = TaskTracker::new();

    let store = common::start_pg_store_for_test(pool, &db_url, shutdown.clone()).await;
    let persist = Arc::clone(&store) as Arc<dyn atc_persist::PersistentStore>;

    let clock: Arc<dyn atc_core::Clock> = Arc::new(atc_core::SystemClock);
    let state = Arc::new(AppState {
        persist: Arc::clone(&persist),
        clock,
        display_ttl: std::time::Duration::from_secs(60 * 60),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: tokio::sync::broadcast::channel(16).0,
        shutdown: shutdown.clone(),
        ws_tracker: ws_tracker.clone(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
    });

    let metrics_handle = metrics::spawn_process_collector(shutdown.clone());

    // Mirror main.rs: clone the Arc into the router so `state` itself stays
    // in this scope, keeping the store's broadcast sender open through
    // shutdown orchestration.
    let app = routes::api_routes()
        .with_state(state.clone())
        .fallback(atc_server::assets::fallback_handler());

    // Bind to an ephemeral port so tests don't conflict.
    let main_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind main listener");
    let main_addr = main_listener.local_addr().expect("local_addr");

    let main_serve =
        axum::serve(main_listener, app).with_graceful_shutdown(shutdown.clone().cancelled_owned());

    let main_serve_task = tokio::spawn(main_serve.into_future());

    let orchestration_handle = tokio::spawn(run_shutdown_orchestration(
        shutdown.clone(),
        ws_tracker,
        main_serve_task,
        persist,
        metrics_handle,
        None,
        None,
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
        + EVICTION_SHUTDOWN_TIMEOUT
        + SHUTDOWN_TIMEOUT_METRICS
        + Duration::from_secs(2); // slop for test harness overhead

    let serve_failure = timeout(aggregate_budget, fixture.orchestration_handle)
        .await
        .expect("orchestration did not complete within aggregate shutdown budget")
        .expect("orchestration task should not panic");

    assert!(
        !serve_failure,
        "signal-driven shutdown must report serve_failure=false"
    );
}
