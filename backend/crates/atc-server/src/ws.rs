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
async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SeqEvent>,
) {
    tracing::info!("WebSocket client connected");

    loop {
        match rx.recv().await {
            Ok(seq_event) => {
                let json = match serde_json::to_string(&seq_event) {
                    Ok(j) => j,
                    Err(e) => {
                        tracing::error!(error = %e, "failed to serialize SeqEvent");
                        continue;
                    }
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    // Client disconnected — exit loop
                    break;
                }
                tracing::debug!(seq = seq_event.seq, "forwarded event to WS client");
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(missed = n, "WebSocket client lagging");
                // Continue receiving — don't disconnect. Client can
                // recover via GET /v1/state when it notices gaps.
            }
            Err(broadcast::error::RecvError::Closed) => {
                // Channel closed — server shutting down
                break;
            }
        }
    }

    tracing::info!("WebSocket client disconnected");
}
