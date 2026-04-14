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

    let reason = loop {
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
                            break "send failed";
                        }
                        tracing::debug!(seq = seq_event.seq, "forwarded event to WS client");
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(missed = n, "WebSocket client lagging");
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
