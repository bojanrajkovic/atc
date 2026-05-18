//! Protocol version handshake and going-away envelope tests (issue #47).
//!
//! Asserts the wire shape of the two new lifecycle frames:
//!
//! 1. `ServerHello { version }` MUST be the first text frame on every fresh
//!    WS connection, carrying the same `env!("VERGEN_GIT_DESCRIBE")` value the
//!    `atc_build_info` gauge already emits.
//! 2. `GoingAway { reason }` MUST be sent as the last application-level frame
//!    before the existing Close-1001 transport frame on graceful shutdown.
//!
//! These tests are intentionally tight on framing order — the frontend's
//! version-mismatch detection relies on ServerHello being the first text frame
//! per connection (not after some other broadcast), and the deploy-detected
//! UX relies on GoingAway arriving before the socket transitions to closed.

use crate::common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atc_core::SystemClock;
use atc_server::routes;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use futures_util::stream::StreamExt;
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Lightweight ephemeral server fixture using the in-memory store. Returns the
/// bound socket address plus the `AppState` so the test can cancel the
/// shutdown token via `app_state.shutdown.cancel()`. The serve task itself is
/// detached and kept running by the spawned task — its handle is dropped
/// because no test in this file needs to join the serve task; the OS reclaims
/// the listener when the test process exits.
async fn test_setup() -> (SocketAddr, Arc<AppState>) {
    common::ensure_recorder_installed();

    let persist = InMemoryStore::new_for_test(Arc::new(SystemClock), Duration::from_hours(1), 256)
        as Arc<dyn atc_persist::PersistentStore>;

    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: tokio::sync::broadcast::channel(16).0,
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });

    let router = routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    (addr, app_state)
}

/// First text frame on a fresh WS connection MUST be ServerHello, and its
/// `version` field MUST equal the same VERGEN_GIT_DESCRIBE the server's
/// `atc_build_info` gauge emits.
#[tokio::test]
#[serial_test::serial]
async fn ws_first_frame_is_server_hello_with_vergen_git_describe() {
    let (server_addr, _state) = test_setup().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("timed out waiting for first WS frame")
        .expect("socket.next() should return Some")
        .expect("first WS frame should be Ok");

    let text = match frame {
        Message::Text(t) => t,
        other => panic!("first frame must be a text frame, got: {:?}", other),
    };

    let json: Value = serde_json::from_str(&text).expect("first frame should deserialize as JSON");

    assert_eq!(
        json.get("kind").and_then(|v| v.as_str()),
        Some("ServerHello"),
        "first frame's `kind` must be ServerHello; got: {}",
        text
    );

    let version = json
        .get("version")
        .and_then(|v| v.as_str())
        .expect("ServerHello must carry a `version` string field");
    assert!(
        !version.is_empty(),
        "ServerHello.version must be non-empty (build pipeline broken?)"
    );
    assert_eq!(
        version,
        env!("VERGEN_GIT_DESCRIBE"),
        "ServerHello.version must match the same VERGEN_GIT_DESCRIBE the metrics layer uses"
    );
}

/// On `shutdown.cancel()`, the WS handler MUST send a `GoingAway { reason }`
/// frame BEFORE the existing Close-1001 transport frame. Asserts the full
/// frame sequence after connect: ServerHello → GoingAway → Close(1001).
#[tokio::test]
#[serial_test::serial]
async fn ws_going_away_precedes_close_on_graceful_shutdown() {
    let (server_addr, state) = test_setup().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Consume the ServerHello frame so the test focuses on shutdown ordering.
    let first = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("timed out on first frame")
        .expect("first frame Some")
        .expect("first frame Ok");
    match first {
        Message::Text(t) => {
            let json: Value = serde_json::from_str(&t).expect("hello JSON");
            assert_eq!(
                json.get("kind").and_then(|v| v.as_str()),
                Some("ServerHello"),
                "ws_going_away test precondition: first frame must be ServerHello, got {}",
                t
            );
        }
        other => panic!("first frame must be Text(ServerHello), got: {:?}", other),
    }

    // Give the handler a moment to enter the select loop after the synchronous
    // ServerHello send completes.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Fire shutdown.
    state.shutdown.cancel();

    // The handler should emit GoingAway, then Close-1001. Skip any stray text
    // frames that arrive between (none should, but tolerate broadcast races).
    let close_budget = Duration::from_secs(3);
    let (going_away_json, close_frame) = tokio::time::timeout(close_budget, async {
        let mut going_away: Option<Value> = None;
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(t))) => {
                    let json: Value =
                        serde_json::from_str(&t).expect("post-shutdown text frame should be JSON");
                    if json.get("kind").and_then(|v| v.as_str()) == Some("GoingAway") {
                        going_away = Some(json);
                    } else {
                        // Tolerate other text frames (unlikely path); keep looking for the
                        // close frame.
                    }
                }
                Some(Ok(Message::Close(f))) => return (going_away, f),
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("WebSocket read error during shutdown: {e}"),
                None => panic!("connection dropped before Close frame"),
            }
        }
    })
    .await
    .expect("timed out waiting for GoingAway + Close after shutdown.cancel()");

    let going_away = going_away_json.expect(
        "GoingAway frame must arrive BEFORE the Close frame; \
         received Close without an intervening GoingAway",
    );

    let reason = going_away
        .get("reason")
        .and_then(|v| v.as_str())
        .expect("GoingAway must carry a `reason` string field");
    assert_eq!(
        reason, "server shutdown",
        "GoingAway.reason must be the SIGTERM-shutdown sentinel string"
    );

    let close = close_frame.expect("Close frame should carry a CloseFrame payload");
    assert_eq!(
        u16::from(close.code),
        1001,
        "close code must remain 1001 (Going Away); got {:?}",
        close.code
    );
}
