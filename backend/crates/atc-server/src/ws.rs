//! WebSocket event stream.
//!
//! Each connection subscribes to the broadcast channel and receives
//! `SeqEvent`s as JSON text frames. One-way push only in Phase 9 —
//! client-to-server messages are ignored.

use std::sync::Arc;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use tokio::sync::broadcast;

use crate::state::{AppState, SeqEvent};

/// Axum handler: upgrade HTTP to WebSocket, subscribe to broadcast channel.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let rx = state.webhook_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, rx))
}

/// Per-connection task: forward broadcast events as JSON text frames.
///
/// Uses `tokio::select!` to race broadcast recv against socket recv,
/// so idle-period client disconnects are detected promptly rather than
/// waiting for the next broadcast event to trigger a failed send.
async fn handle_socket(mut socket: WebSocket, mut rx: broadcast::Receiver<SeqEvent>) {
    tracing::info!("WebSocket client connected");

    loop {
        tokio::select! {
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
                            break;
                        }
                        tracing::debug!(seq = seq_event.seq, "forwarded event to WS client");
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "WebSocket client lagging");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    // Client sent close or the connection dropped.
                    Some(Ok(Message::Close(_))) | None => break,
                    // Ignore all other client-to-server messages.
                    Some(Ok(_)) => {}
                    // Read error — connection is broken.
                    Some(Err(_)) => break,
                }
            }
        }
    }

    tracing::info!("WebSocket client disconnected");
}
