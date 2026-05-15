//! WebSocket integration tests.
//!
//! Each test starts an ephemeral server and uses tokio_tungstenite to connect
//! as a client. For lag handling tests we drive events through
//! `persist.apply_run_event(...)` with distinct run ids — the store fans them
//! out through its internal broadcast sender.

use crate::common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atc_core::SystemClock;
use atc_server::persist::InMemoryStore;
use atc_server::routes;
use atc_server::state::AppState;
use atc_wire::CommittedEvent;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use futures_util::stream::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Build an ephemeral server with a custom broadcast capacity.
/// Returns (server_address, AppState).
///
/// The broadcast capacity is parameterized so the lagging-client test can
/// trigger lag by using a capacity smaller than the number of events sent.
async fn test_setup(broadcast_capacity: usize) -> (SocketAddr, Arc<AppState>) {
    common::ensure_recorder_installed();

    let persist = InMemoryStore::new_for_test(
        Arc::new(SystemClock),
        Duration::from_hours(1),
        broadcast_capacity,
    ) as Arc<dyn atc_persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: Vec::new(),
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });

    let main_router = routes::api_routes()
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    (main_addr, app_state)
}

/// GET /v1/ws upgrades to WebSocket connection
#[tokio::test]
#[serial_test::serial]
async fn ws_upgrade_succeeds() {
    let (server_addr, _) = test_setup(256).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let result = tokio_tungstenite::connect_async(&ws_url).await;

    assert!(
        result.is_ok(),
        "WebSocket connection should succeed; got error: {:?}",
        result.err()
    );

    let (mut _socket, _response) = result.unwrap();
    // Connection established successfully. In a real scenario, the client would
    // keep the socket alive and receive frames. Here we just verify the upgrade succeeded.
}

/// Connected client receives CommittedEvent after webhook ingestion
#[tokio::test]
#[serial_test::serial]
async fn ws_receives_webhook_event() {
    let (server_addr, _) = test_setup(256).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Post a webhook via HTTP
    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);
    let body = common::fixture_workflow_run_requested();

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200, "Webhook should be accepted");

    // Read the WebSocket frame with a timeout
    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("Timeout waiting for WebSocket frame")
        .expect("WebSocket next() should return Some")
        .expect("WebSocket frame should be OK");

    // Verify it's a text frame
    let text = match frame {
        Message::Text(t) => t,
        other => panic!("Expected text frame, got: {:?}", other),
    };

    // Deserialize as CommittedEvent and verify the structure
    let committed_event: CommittedEvent =
        serde_json::from_str(&text).expect("CommittedEvent JSON deserialization should succeed");

    assert_eq!(committed_event.seq, 1, "First event should have seq=1");
    // Just verify it's a Run event variant (don't deep-inspect the enum)
    match &committed_event.event {
        atc_github::WebhookEvent::Run(_) => {}
        atc_github::WebhookEvent::Job(_) => panic!("Expected Run event, got Job"),
    }
}

/// Multiple connected clients each receive the same CommittedEvent
#[tokio::test]
#[serial_test::serial]
async fn multiple_clients_receive_same_event() {
    let (server_addr, _state) = test_setup(256).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);

    // Connect two WebSocket clients
    let (mut socket1, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 1 connection failed");
    let (mut socket2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 2 connection failed");

    // Post a webhook via HTTP
    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);
    let body = common::fixture_workflow_run_requested();

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200);

    // Both clients should receive the same event
    let frame1 = tokio::time::timeout(Duration::from_secs(2), socket1.next())
        .await
        .expect("Timeout on socket1")
        .expect("socket1.next() should return Some")
        .expect("socket1 frame should be OK");

    let frame2 = tokio::time::timeout(Duration::from_secs(2), socket2.next())
        .await
        .expect("Timeout on socket2")
        .expect("socket2.next() should return Some")
        .expect("socket2 frame should be OK");

    let text1 = match frame1 {
        Message::Text(t) => t,
        _ => panic!("Expected text frame on socket1"),
    };

    let text2 = match frame2 {
        Message::Text(t) => t,
        _ => panic!("Expected text frame on socket2"),
    };

    let committed_event1: CommittedEvent =
        serde_json::from_str(&text1).expect("CommittedEvent 1 JSON deserialization should succeed");
    let committed_event2: CommittedEvent =
        serde_json::from_str(&text2).expect("CommittedEvent 2 JSON deserialization should succeed");

    // Both should have the same seq
    assert_eq!(
        committed_event1.seq, committed_event2.seq,
        "Both clients should receive event with same seq"
    );
    assert_eq!(committed_event1.seq, 1, "First event seq should be 1");
}

/// Client disconnect does not crash server or affect other clients
#[tokio::test]
#[serial_test::serial]
async fn disconnect_does_not_crash_server() {
    let (server_addr, _state) = test_setup(256).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);

    // Connect two clients
    let (socket1, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 1 connection failed");
    let (mut socket2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 2 connection failed");

    // Drop client 1 (disconnect)
    drop(socket1);

    // Give the server a moment to process the disconnect
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Post a webhook via HTTP
    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);
    let body = common::fixture_workflow_run_requested();

    let resp = client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(body)
        .send()
        .await
        .expect("Webhook POST failed");

    assert_eq!(resp.status(), 200, "Server should still accept webhooks");

    // Client 2 should still receive the event
    let frame = tokio::time::timeout(Duration::from_secs(2), socket2.next())
        .await
        .expect("Timeout waiting for frame on socket2")
        .expect("socket2.next() should return Some")
        .expect("socket2 frame should be OK");

    let text = match frame {
        Message::Text(t) => t,
        _ => panic!("Expected text frame on socket2"),
    };

    let committed_event: CommittedEvent =
        serde_json::from_str(&text).expect("CommittedEvent JSON deserialization should succeed");
    assert_eq!(committed_event.seq, 1, "Client 2 should receive the event");
}

/// Lagging client is disconnected — the server closes the connection on lag.
///
/// Uses a capacity-2 broadcast channel. The WS client subscribes and then
/// does NOT read from the socket. Sending 3 events via the broadcast sender
/// wraps the ring buffer, causing the receiver to lag. When the handler
/// finally calls `rx.recv()`, it gets `RecvError::Lagged(n)` and closes
/// the connection. The test asserts the client sees the connection close.
#[tokio::test]
#[serial_test::serial]
async fn lagging_client_is_disconnected() {
    // Capacity-2 channel: 3 sends without any recv causes lag.
    let (server_addr, state) = test_setup(2).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Give the handler time to subscribe to the broadcast channel before we
    // flood it.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Parse a valid fixture to get a real RunEventEnvelope template.
    let fixture = common::fixture_workflow_run_requested();
    let parsed = atc_github::parse_webhook("workflow_run", &fixture).expect("fixture parse failed");
    let env_template = match parsed {
        atc_github::ParseResult::Parsed(boxed) => match *boxed {
            atc_github::WebhookEvent::Run(env) => env,
            other => panic!("expected a Run event, got {other:?}"),
        },
        other => panic!("unexpected parse result: {other:?}"),
    };

    // Apply 3 run events with distinct run_ids so each call is a first-sight
    // create (no Queued → Queued transition handling involved). Each successful
    // apply broadcasts a `CommittedEvent`; with capacity 2 the third broadcast laps
    // the ring buffer, invalidating the receiver's position so its next
    // `recv()` returns `Lagged`.
    for i in 0..3i64 {
        let mut env = env_template.clone();
        env.run_id = atc_core::types::RunId(9_900_000 + i);
        state
            .persist
            .apply_run_event(env)
            .await
            .expect("apply_run_event");
    }

    // The handler should observe lag and close the socket. Assert close or
    // connection drop within a short budget.
    let got_close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(_))) => return true,
                Some(Ok(_)) => {} // may receive frames before lag fires
                Some(Err(_)) | None => return true, // connection dropped
            }
        }
    })
    .await
    .expect("timed out waiting for server to close lagging connection");

    assert!(
        got_close,
        "lagging client should have its connection closed by the server"
    );
}

/// Cancelling the WS-close token causes an idle connected client to receive
/// a Close frame with code 1001 "going away" within 200 ms.
///
/// The client is idle — it neither sends frames nor reads during the cancel
/// window, so the server-side handler is blocked in the select loop when the
/// cancel fires. We cancel, wait a short budget, then read and verify the
/// Close frame arrives.
#[tokio::test]
#[serial_test::serial]
async fn idle_client_receives_close_on_cancel() {
    let (server_addr, state) = test_setup(256).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Give the handler time to subscribe and enter the select loop before we cancel.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Fire the shutdown token; WS handler should send Close(1001) and exit.
    state.shutdown.cancel();

    // The handler should send Close(1001) promptly. Assert within 200 ms.
    let frame = tokio::time::timeout(Duration::from_millis(200), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Close(frame))) => return frame,
                Some(Ok(_)) => {} // skip any stray frames
                Some(Err(e)) => panic!("WebSocket error waiting for close: {e}"),
                None => panic!("connection dropped without a Close frame"),
            }
        }
    })
    .await
    .expect("timed out waiting for Close frame from server");

    let close = frame.expect("Close frame should carry a CloseFrame payload");
    assert_eq!(
        u16::from(close.code),
        1001,
        "close code should be 1001 (Going Away), got {:?}",
        close.code
    );
}
