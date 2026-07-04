//! WebSocket integration tests.
//!
//! Each test starts an ephemeral server and uses tokio_tungstenite to connect
//! as a client. For lag handling tests we drive events through
//! `persist.apply_run_event(...)` with distinct run ids — the store fans them
//! out through its internal broadcast sender.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::SystemClock;
use atc_server::config_watcher::ConfigEvent;
use atc_server::routes;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use atc_wire::CommittedEvent;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use futures_util::stream::StreamExt;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

/// GET /v1/ws upgrades to WebSocket connection
#[tokio::test]
#[serial_test::serial]
async fn ws_upgrade_succeeds() {
    let (server_addr, _) = common::spawn_in_memory_server().await;

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
    let (server_addr, _) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    common::consume_server_hello(&mut socket).await;

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

    // Deserialize as the inner CommittedEvent. The wire shape now carries an
    // outer `kind` discriminator (WireFrame), but the `Committed` variant is
    // flattened so `seq` and `event` appear at the top level alongside `kind`,
    // and serde_json's untagged deserialize for `CommittedEvent` still reads
    // the same fields.
    let json: serde_json::Value =
        serde_json::from_str(&text).expect("WireFrame JSON deserialization should succeed");
    assert_eq!(json["kind"], "Committed");
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
    let (server_addr, _state) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);

    // Connect two WebSocket clients
    let (mut socket1, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 1 connection failed");
    let (mut socket2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 2 connection failed");

    common::consume_server_hello(&mut socket1).await;
    common::consume_server_hello(&mut socket2).await;

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
    let (server_addr, _state) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);

    // Connect two clients
    let (socket1, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 1 connection failed");
    let (mut socket2, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WS client 2 connection failed");

    common::consume_server_hello(&mut socket2).await;

    // Drop client 1 (disconnect). Its ServerHello may not have been read
    // before the drop, but that's irrelevant — the test asserts that the
    // server keeps running for client 2.
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
    let (server_addr, state) = common::spawn_in_memory_server_with_capacity(2).await;

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
    let (server_addr, state) = common::spawn_in_memory_server().await;

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

/// WireFrame: a committed event arrives as `{"kind":"Committed", ...}` with
/// camelCase fields (matching the existing CommittedEvent convention).
#[tokio::test]
#[serial_test::serial]
async fn wireframe_committed_kind_matches_camelcase() {
    let (server_addr, _) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    common::consume_server_hello(&mut socket).await;

    let client = reqwest::Client::new();
    let webhook_url = format!("http://{}/v1/webhooks/github", server_addr);
    client
        .post(&webhook_url)
        .header("X-GitHub-Event", "workflow_run")
        .body(common::fixture_workflow_run_requested())
        .send()
        .await
        .expect("Webhook POST failed");

    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("timeout waiting for frame")
        .expect("frame")
        .expect("frame is Ok");
    let text = match frame {
        Message::Text(t) => t,
        other => panic!("Expected text frame, got: {other:?}"),
    };

    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(
        json["kind"], "Committed",
        "outer kind discriminator should be \"Committed\"",
    );
    assert!(json["seq"].is_number());
    assert!(json["event"].is_object());
}

/// WireFrame: a config-watcher Update arrives as
/// `{"kind":"ConfigUpdate","runnerPoolCapacities":[...]}` with camelCase
/// field names.
#[tokio::test]
#[serial_test::serial]
async fn wireframe_config_update_kind_and_camelcase_fields() {
    let (server_addr, state) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    common::consume_server_hello(&mut socket).await;

    // Wait briefly so the handler subscribes before we broadcast.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let caps = vec![atc_core::RunnerPoolCapacity {
        labels: atc_core::LabelSet::new(["self-hosted", "linux"]),
        capacity: Some(7),
    }];
    state
        .config_events_tx
        .send(ConfigEvent::Update(caps))
        .expect("send ConfigEvent::Update");

    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("timeout waiting for frame")
        .expect("frame")
        .expect("frame is Ok");
    let text = match frame {
        Message::Text(t) => t,
        other => panic!("Expected text frame, got: {other:?}"),
    };

    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(json["kind"], "ConfigUpdate");
    let caps_json = json["runnerPoolCapacities"]
        .as_array()
        .expect("runnerPoolCapacities is array");
    assert_eq!(caps_json.len(), 1);
    assert_eq!(caps_json[0]["capacity"], 7);
}

/// WireFrame: a config-watcher ReloadError arrives as
/// `{"kind":"ConfigReloadError","reason":"..."}`.
#[tokio::test]
#[serial_test::serial]
async fn wireframe_config_reload_error_kind() {
    let (server_addr, state) = common::spawn_in_memory_server().await;

    let ws_url = format!("ws://{}/v1/ws", server_addr);
    let (mut socket, _response) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connection failed");

    common::consume_server_hello(&mut socket).await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    state
        .config_events_tx
        .send(ConfigEvent::ReloadError {
            reason: "capacity must be >= 1".to_string(),
        })
        .expect("send ConfigEvent::ReloadError");

    let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .expect("timeout waiting for frame")
        .expect("frame")
        .expect("frame is Ok");
    let text = match frame {
        Message::Text(t) => t,
        other => panic!("Expected text frame, got: {other:?}"),
    };

    let json: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(json["kind"], "ConfigReloadError");
    assert_eq!(json["reason"], "capacity must be >= 1");
}

/// Symmetric Lagged handling: a slow client on the config channel that lags
/// past the broadcast capacity gets disconnected, matching the existing
/// committed-channel close-on-lag behavior.
#[tokio::test]
#[serial_test::serial]
async fn config_channel_lagged_closes_socket() {
    // Build state with a tiny config-channel capacity (2). The committed
    // channel uses the standard capacity — we're isolating config-channel
    // lag from committed-channel lag here.
    common::ensure_recorder_installed();

    let clock: Arc<dyn atc_core::Clock> = Arc::new(SystemClock);
    let persist = InMemoryStore::new_for_test(Arc::clone(&clock), Duration::from_hours(1), 256)
        as Arc<dyn atc_persist::PersistentStore>;
    let (config_tx, _) = tokio::sync::broadcast::channel::<ConfigEvent>(2);
    let state = Arc::new(AppState {
        persist,
        clock,
        display_ttl: Duration::from_hours(1),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: config_tx.clone(),
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
        auth: None,
    });

    let router = routes::api_routes(false)
        .with_state(state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    let ws_url = format!("ws://{addr}/v1/ws");
    let (mut socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .expect("WebSocket connect");

    // Wait for the handler to subscribe.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Push more events than the capacity-2 channel can hold without a
    // consumer keeping pace. The first events get buffered; pushing past
    // capacity advances the receiver cursor and the next recv() returns
    // Lagged. The handler closes the socket on Lagged.
    for i in 0..10 {
        let _ = config_tx.send(ConfigEvent::ReloadError {
            reason: format!("forced lag {i}"),
        });
    }

    // The handler should close the socket; wait for the Close frame OR for
    // the stream to end. The first frames may arrive successfully (channel
    // doesn't lag until the cursor advances past capacity), so iterate.
    let close = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(_))) => continue,
                Some(Ok(Message::Close(frame))) => return Some(frame),
                Some(Ok(_)) => continue,
                Some(Err(_)) | None => return None,
            }
        }
    })
    .await
    .expect("timed out waiting for socket close after lag");
    // Either a Close frame or a connection-end is acceptable — both mean
    // the handler exited. Tungstenite often surfaces the close as an
    // immediate stream-end after a server-side disconnect; the key
    // assertion is that the socket no longer accepts further frames.
    drop(close);
}
