//! WebSocket event stream.
//!
//! Each connection subscribes to two broadcast channels — the persistent
//! store's `CommittedEvent` stream (event-derived state, seq-keyed) and the
//! config watcher's `ConfigEvent` stream (operator-config reloads) — and
//! forwards each incoming event wrapped in an outer-discriminator
//! [`WireFrame`] as a JSON text frame. One-way push only —
//! client-to-server messages are ignored.
//!
//! The `WireFrame` discriminator gives the frontend dispatcher a single
//! entry point for both data streams. The two layers exist because
//! `CommittedEvent` (seq-keyed, replayable) and `ConfigEvent` (non-seq,
//! latest-wins) have different replay semantics that the frontend's
//! pre-snapshot buffering distinguishes; framing them under one outer
//! `kind` keeps the wire shape uniform without mixing those semantics in
//! the wire types themselves.

use std::sync::Arc;

use atc_core::RunnerPoolCapacity;
use atc_wire::CommittedEvent;
use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::config_watcher::ConfigEvent;
use crate::state::AppState;

/// Outer wire frame for the `/v1/ws` event stream.
///
/// Tagged with an outer `kind` discriminator so the frontend dispatcher can
/// branch by stream type before reaching the inner event shape. Field
/// renaming uses camelCase to match the existing `CommittedEvent` /
/// `StateSnapshot` convention — the frontend's generated TS file uses
/// camelCase throughout.
///
/// Variants:
/// - `Committed` — wraps the store's `CommittedEvent` envelope (event-derived
///   state, seq-keyed). Frontend replays unseen seqs against the snapshot's
///   `lastSeq` cursor.
/// - `ConfigUpdate` — full operator-declared capacity list after a successful
///   hot-reload. Not seq-keyed; latest-wins semantics in the pre-snapshot
///   buffer.
/// - `ConfigReloadError` — reload failed on the server. The server keeps
///   serving the last-known-good capacities; the wire `reason` is a
///   human-readable string (`err.to_string()`), not a category enum.
///   Frontend handling is owned by `docs/architecture/frontend-app.md`.
/// - `ServerHello` — sent as the first text frame on every fresh WS
///   connection, carrying the backend's `VERGEN_GIT_DESCRIBE` build
///   identifier. The frontend uses the first ServerHello in a tab session as
///   its session reference; later mismatches arm a deploy-detected refresh
///   banner. See issue #47.
/// - `GoingAway` — sent immediately before the existing Close-1001 transport
///   frame on graceful shutdown. Informational application-level metadata so
///   the frontend's ConnectionIndicator can show a tailored "Server
///   restarting" state during the gap between the close and the next
///   reconnect. The Close-1001 transport signal remains the authoritative
///   shutdown indication. See issue #47.
// The `Committed` variant flattens a full `CommittedEvent` (`WebhookEvent` is
// the bulk — ~296 B today), so the clippy::large_enum_variant size-skew
// warning fires on the smaller `ConfigUpdate` / `ConfigReloadError` /
// `ServerHello` / `GoingAway` variants. Boxing here would inject
// `{ "kind": "Committed", "0": { ... } }` into the wire shape (or force a
// wrapper struct), breaking the frontend's outer-kind switch. The size skew
// is irrelevant in practice — the WS handler owns one `WireFrame` value at a
// time per connection — so we accept the lint here rather than corrupt the
// wire shape.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, serde::Serialize, ts_rs::TS)]
#[serde(tag = "kind")]
#[ts(export)]
pub enum WireFrame {
    Committed(CommittedEvent),
    #[serde(rename_all = "camelCase")]
    ConfigUpdate {
        runner_pool_capacities: Vec<RunnerPoolCapacity>,
    },
    ConfigReloadError {
        reason: String,
    },
    ServerHello {
        version: String,
    },
    GoingAway {
        reason: String,
    },
}

/// Axum handler: upgrade HTTP to WebSocket, subscribe to both event channels.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let committed_rx = state.persist.subscribe();
    let config_rx = state.config_events_tx.subscribe();
    let shutdown = state.shutdown.clone();
    ws.on_upgrade(move |socket| {
        state
            .ws_tracker
            .track_future(handle_socket(socket, committed_rx, config_rx, shutdown))
    })
}

/// Per-connection task: forward both broadcast streams as `WireFrame` JSON
/// text frames.
///
/// On `shutdown.cancelled()`, emits `Close(1001 "going away")` and returns.
/// Catch-up after reconnect is the frontend's responsibility via `/v1/state`
/// on a healthy replica (see `docs/architecture/backend-server.md` §
/// "Supervision and Shutdown").
///
/// Lagged handling is symmetric across both channels: missing events on
/// either stream closes the socket, and the client's reconnect handler
/// fetches `/v1/state` to re-establish both the seq cursor and the current
/// capacity list. Symmetric handling avoids the "silent drop" trap where
/// one channel's overflow goes unnoticed.
///
/// `tokio::select! { biased; … }` evaluates arms top-down. Order matters:
/// `shutdown.cancelled()` first so the cancel signal is preferred over any
/// concurrently-ready arm (keeps shutdown predictable for tests and
/// operators); then the low-volume arms (`config_rx`, the client socket)
/// before `committed_rx`. Putting the high-volume committed arm last
/// prevents sustained webhook traffic from perpetually starving the config
/// arm — under burst load every iteration committed_rx is ready, and a
/// committed-first bias would mean a fresh `ConfigUpdate` could sit
/// unforwarded until the buffer rolls over to `Lagged`. Config and socket
/// poll fairly under that bias because their ready-rates are near zero, so
/// shifting them above committed doesn't materially delay committed
/// forwarding. `main` keeps an `Arc<AppState>` alive (and therefore the
/// `Arc<dyn PersistentStore>` inside it) through orchestration, so the
/// store's broadcast sender stays open through the cancel-fire window.
async fn handle_socket(
    mut socket: WebSocket,
    mut committed_rx: broadcast::Receiver<CommittedEvent>,
    mut config_rx: broadcast::Receiver<ConfigEvent>,
    shutdown: CancellationToken,
) {
    tracing::info!("WebSocket client connected");

    // Synchronously send ServerHello as the first text frame on this
    // connection. Broadcast receivers were subscribed BEFORE the upgrade
    // completed (ws_handler), so any events that fire between subscription and
    // this point accumulate in the broadcast buffer (capacity 256) and drain
    // through the `select!` loop AFTER ServerHello ships. One task owns the
    // socket, so the ordering invariant — "ServerHello is the first text
    // frame" — holds without additional synchronization.
    if send_frame(
        &mut socket,
        &WireFrame::ServerHello {
            version: env!("VERGEN_GIT_DESCRIBE").to_string(),
        },
    )
    .await
    .is_err()
    {
        tracing::info!("WebSocket client disconnected before ServerHello could be sent");
        return;
    }

    let reason = loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                // GoingAway is informational application-level metadata that
                // gives the frontend a chance to render a tailored "Server
                // restarting" state before the transport closes. The Close
                // frame below remains the authoritative shutdown signal —
                // best-effort sends because the client may already be gone.
                let _ = send_frame(
                    &mut socket,
                    &WireFrame::GoingAway { reason: "server shutdown".into() },
                ).await;
                let _ = socket.send(Message::Close(Some(CloseFrame {
                    code: 1001,
                    reason: "going away".into(),
                }))).await;
                break "shutdown";
            }
            result = config_rx.recv() => {
                match result {
                    Ok(config_event) => {
                        let frame = match config_event {
                            ConfigEvent::Update(caps) => WireFrame::ConfigUpdate {
                                runner_pool_capacities: caps,
                            },
                            ConfigEvent::ReloadError { reason } => {
                                WireFrame::ConfigReloadError { reason }
                            }
                        };
                        if let Err(reason) = send_frame(&mut socket, &frame).await {
                            break reason;
                        }
                        tracing::debug!("forwarded config event to WS client");
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Symmetric with the committed channel: close on lag,
                        // client reconnects and re-fetches /v1/state to pick
                        // up the latest capacities from the snapshot rail.
                        tracing::warn!(
                            missed = n,
                            "WebSocket client lagging on config channel; closing to force re-snapshot",
                        );
                        break "config lagged";
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break "config channel closed";
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
            result = committed_rx.recv() => {
                match result {
                    Ok(committed_event) => {
                        let seq = committed_event.seq;
                        let frame = WireFrame::Committed(committed_event);
                        if let Err(reason) = send_frame(&mut socket, &frame).await {
                            break reason;
                        }
                        tracing::debug!(seq, "forwarded committed event to WS client");
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // The subscriber missed `n` events because the
                        // bounded broadcast channel (capacity 256) advanced
                        // past their cursor. Close the socket — the frontend's
                        // reconnect handler will fetch /v1/state and
                        // re-establish the seq cursor from the snapshot.
                        tracing::warn!(
                            missed = n,
                            "WebSocket client lagging on committed channel; closing to force re-snapshot",
                        );
                        break "lagged";
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break "broadcast channel closed";
                    }
                }
            }
        }
    };

    tracing::info!(reason, "WebSocket client disconnected");
}

/// Serialize a `WireFrame` as JSON and push it as a text frame.
///
/// Returns `Err(reason)` if the send failed (the caller breaks the loop with
/// that reason). Serialization errors are logged and treated as a transient
/// skip — the WS connection survives so other frames can still flow.
async fn send_frame(socket: &mut WebSocket, frame: &WireFrame) -> Result<(), &'static str> {
    let json = match serde_json::to_string(frame) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize WireFrame; skipping");
            return Ok(());
        }
    };
    if socket.send(Message::Text(json.into())).await.is_err() {
        return Err("send failed");
    }
    Ok(())
}
