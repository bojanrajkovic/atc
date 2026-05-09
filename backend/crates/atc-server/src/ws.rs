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
    let ws_close = state.ws_close.clone();
    ws.on_upgrade(move |socket| {
        state
            .ws_tracker
            .track_future(handle_socket(socket, rx, ws_close))
    })
}

/// Per-connection task: forward broadcast events as JSON text frames.
///
/// Uses `tokio::select! { biased; … }` to prioritise arms top-down:
/// 1. `rx.recv()` — drain any buffered broadcast events first.
/// 2. `socket.recv()` — detect client-initiated close or errors promptly.
/// 3. `ws_close.cancelled()` — send `Close(1001 "going away")` on shutdown.
///
/// The biased order ensures buffered events are delivered to the client
/// before the cancellation arm fires, preserving the event stream up to the
/// shutdown signal. The close send is best-effort (`let _ = ...`): if the
/// client has already disconnected, the attempt silently fails.
async fn handle_socket(
    mut socket: WebSocket,
    mut rx: broadcast::Receiver<SeqEvent>,
    ws_close: CancellationToken,
) {
    tracing::info!("WebSocket client connected");

    let reason = loop {
        tokio::select! {
            biased;
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
                        // past their cursor. Continuing on the same receiver
                        // would deliver subsequent events but leave the gap
                        // permanently filled in the client's view. Close the
                        // socket instead — the frontend's reconnect handler
                        // will fetch /v1/state, which returns a fresh
                        // snapshot keyed by `broadcast_watermark` and
                        // re-establishes the seq cursor.
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
            () = ws_close.cancelled() => {
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "going away".into(),
                }))).await;
                break "shutdown";
            }
        }
    };

    tracing::info!(reason, "WebSocket client disconnected");
}
