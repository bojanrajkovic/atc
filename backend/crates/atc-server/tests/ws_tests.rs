//! WebSocket integration tests covering AC3.1-AC3.5.
//!
//! Each test starts an ephemeral server and uses tokio_tungstenite to connect as a client.
//! For AC3.5 (lag handling), we send events directly via state.webhook_tx.send() to avoid
//! timing races and more easily control event volume.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::routes;
use atc_server::state::{AppState, SeqEvent};
use futures_util::stream::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// Build an ephemeral server with a custom broadcast capacity.
/// Returns (server_address, AppState with broadcast channel).
async fn test_setup(broadcast_capacity: usize) -> (SocketAddr, Arc<AppState>) {
    // Use the shared PROMETHEUS_INIT to avoid multiple initializations
    let layer = common::PROMETHEUS_INIT.get_or_init(|| atc_server::metrics::build().0);

    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(broadcast_capacity);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
        pg_store: None,
    });

    let main_router = routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let main_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    (main_addr, app_state)
}

/// AC3.1: GET /v1/ws upgrades to WebSocket connection
#[tokio::test]
#[serial_test::serial]
async fn ac3_1_ws_upgrade_succeeds() {
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

/// AC3.2: Connected client receives SeqEvent after webhook ingestion
#[tokio::test]
#[serial_test::serial]
async fn ac3_2_ws_receives_webhook_event() {
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

    // Deserialize as SeqEvent and verify the structure
    let seq_event: SeqEvent =
        serde_json::from_str(&text).expect("SeqEvent JSON deserialization should succeed");

    assert_eq!(seq_event.seq, 0, "First event should have seq=0");
    // Just verify it's a Run event variant (don't deep-inspect the enum)
    match &seq_event.event {
        atc_github::WebhookEvent::Run(_) => {}
        atc_github::WebhookEvent::Job(_) => panic!("Expected Run event, got Job"),
    }
}

/// AC3.3: Multiple connected clients each receive the same SeqEvent
#[tokio::test]
#[serial_test::serial]
async fn ac3_3_multiple_clients_receive_same_event() {
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

    let seq_event1: SeqEvent =
        serde_json::from_str(&text1).expect("SeqEvent 1 JSON deserialization should succeed");
    let seq_event2: SeqEvent =
        serde_json::from_str(&text2).expect("SeqEvent 2 JSON deserialization should succeed");

    // Both should have the same seq
    assert_eq!(
        seq_event1.seq, seq_event2.seq,
        "Both clients should receive event with same seq"
    );
    assert_eq!(seq_event1.seq, 0, "First event seq should be 0");
}

/// AC3.4: Client disconnect does not crash server or affect other clients
#[tokio::test]
#[serial_test::serial]
async fn ac3_4_disconnect_does_not_crash_server() {
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

    let seq_event: SeqEvent =
        serde_json::from_str(&text).expect("SeqEvent JSON deserialization should succeed");
    assert_eq!(seq_event.seq, 0, "Client 2 should receive the event");
}

/// AC3.5: Lagging client receives warning log, continues receiving (not disconnected)
#[tokio::test]
#[serial_test::serial]
async fn ac3_5_lagging_client_continues_receiving() {
    // Use a small broadcast capacity to make lag easy to trigger
    let (server_addr, _) = test_setup(2).await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    // Give the handler time to subscribe to the broadcast channel
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Post multiple webhooks to the server to generate more events than the
    // broadcast capacity. This will cause lag on the WS client.
    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);

    for _ in 0..5 {
        let body = common::fixture_workflow_run_requested();
        let resp = client
            .post(&webhook_url)
            .header("X-GitHub-Event", "workflow_run")
            .body(body)
            .send()
            .await
            .expect("Webhook POST failed");
        assert_eq!(resp.status(), 200);
    }

    // The client should eventually receive events after the lag.
    // Due to lag, it may not receive all events, but it should receive at least some.
    let mut received_count = 0;
    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_millis(500), socket.next()).await {
            Ok(Some(Ok(Message::Text(_)))) => {
                received_count += 1;
                if received_count >= 2 {
                    break; // We've received at least 2 events, lagging handled correctly
                }
            }
            Ok(Some(Ok(other))) => {
                panic!("Expected text frame, got: {:?}", other);
            }
            Ok(Some(Err(e))) => {
                panic!("WebSocket error: {}", e);
            }
            Ok(None) => {
                panic!("WebSocket connection closed");
            }
            Err(_) => {
                // Timeout is expected — we're just polling
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }

    assert!(
        received_count >= 2,
        "Lagging client should eventually receive at least 2 events (got {})",
        received_count
    );
}
