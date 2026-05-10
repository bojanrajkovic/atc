//! WebSocket event stream.
//!
//! Each connection subscribes to the broadcast channel and receives
//! `SeqEvent`s as JSON text frames. One-way push only —
//! client-to-server messages are ignored.

use std::sync::Arc;

use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::state::{AppState, SeqEvent};

/// Axum handler: upgrade HTTP to WebSocket, subscribe to broadcast channel.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.webhook_tx.subscribe();
    let shutdown = state.shutdown.clone();
    ws.on_upgrade(move |socket| {
        state
            .ws_tracker
            .track_future(handle_socket(socket, rx, shutdown))
    })
}

/// Per-connection task: forward broadcast events as JSON text frames.
///
/// On `shutdown.cancelled()`, emits `Close(1001 "going away")` and returns.
/// Catch-up after reconnect is the frontend's responsibility via `/v1/state`
/// on a healthy replica (see `docs/architecture/backend-server.md` §
/// "Supervision and Shutdown").
///
/// `tokio::select! { biased; … }` evaluates arms top-down with
/// `shutdown.cancelled()` first so the cancel signal is preferred over any
/// concurrently-ready arm, keeping shutdown predictable for tests and
/// operators. `main` keeps an `Arc<AppState>` alive through orchestration, so
/// the broadcast channel stays open through the cancel-fire window — the
/// `RecvError::Closed` arm is only reached in genuinely abnormal scenarios.
async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SeqEvent>,
    shutdown: CancellationToken,
) {
    tracing::info!("WebSocket client connected");

    let reason = loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "going away".into(),
                }))).await;
                break "shutdown";
            }
            result = rx.recv() => {
                match result {
                    Ok(seq_event) => {
                        let json = match serde_json::to_string(&seq_event) {
                            Ok(j) => j,
                            Err(e) => {
                                tracing::error!(error = %e, "failed to serialize SeqEvent");
                                continue;
                            }
                        };
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            break "send failed";
                        }
                        tracing::debug!(seq = seq_event.seq, "forwarded event to WS client");
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // The subscriber missed `n` events because the
                        // bounded broadcast channel (capacity 256) advanced
                        // past their cursor. Close the socket — the frontend's
                        // reconnect handler will fetch /v1/state and
                        // re-establish the seq cursor from the snapshot.
                        tracing::warn!(
                            missed = n,
                            "WebSocket client lagging; closing to force re-snapshot",
                        );
                        break "lagged";
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break "broadcast channel closed";
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) => break "client sent close",
                    None => break "connection dropped",
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "WebSocket read error");
                        break "read error";
                    }
                }
            }
        }
    };

    tracing::info!(reason, "WebSocket client disconnected");
}
