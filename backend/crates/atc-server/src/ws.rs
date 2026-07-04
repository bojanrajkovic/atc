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
use std::sync::Weak;
use std::sync::atomic::{AtomicI64, Ordering};

use atc_core::RunnerPoolCapacity;
use atc_wire::CommittedEvent;
use axum::{
    extract::{
        State,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use opentelemetry::KeyValue;
use opentelemetry::metrics::Counter;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use crate::auth::AuthContext;
use crate::config_watcher::ConfigEvent;
use crate::state::AppState;

/// Meter scope for WS instruments. Mirrors the `ConfigWatcherMetrics` and
/// `PgMetrics` conventions.
const METER_SCOPE: &str = "atc";

/// OTel-instrumentation surface for the WebSocket layer.
///
/// Registered once at startup via [`WsMetrics::register`] and threaded onto
/// `AppState` so the handler can record per-connection events.
///
/// Mirrors the cached-instrument + observable-gauge-via-`Weak<Arc<AtomicI64>>`
/// pattern used by [`crate::config_watcher::ConfigWatcherMetrics`] and
/// `atc_store_pg::metrics::PgMetrics` — see `docs/architecture/metrics.md`
/// § "Cached instrument convention".
pub struct WsMetrics {
    lagged_evictions: Counter<u64>,
    active_connections: Arc<AtomicI64>,
    attrs_committed: [KeyValue; 1],
    attrs_config: [KeyValue; 1],
}

/// Which broadcast channel a WS connection lagged on. Bounded label set —
/// labeled on the `atc_ws_lagged_evictions_total` counter.
#[derive(Clone, Copy)]
pub enum LaggedChannel {
    /// `state.persist.subscribe()` — the `CommittedEvent` broadcast.
    Committed,
    /// `state.config_events_tx.subscribe()` — the operator-config broadcast.
    Config,
}

impl WsMetrics {
    /// Register OTel instruments against the global meter.
    ///
    /// Must run after `otel::init_otel`. Safe to call under the no-op meter
    /// (everything compiles to no-ops).
    #[must_use]
    pub fn register() -> Arc<Self> {
        let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);

        let lagged_evictions = meter
            .u64_counter("atc_ws_lagged_evictions_total")
            .with_description(
                "WebSocket clients force-disconnected because their broadcast \
                 receiver fell behind and the bounded buffer (capacity 256) \
                 overflowed. A sustained nonzero rate means the broadcast \
                 buffer is undersized for current traffic OR a client is \
                 stalled. Labeled by channel: \
                 channel=committed → CommittedEvent stream (webhook fan-out); \
                 channel=config → ConfigEvent stream (operator-config reloads).",
            )
            .build();

        let active_connections = Arc::new(AtomicI64::new(0));
        let active_weak: Weak<AtomicI64> = Arc::downgrade(&active_connections);
        let _gauge = meter
            .i64_observable_gauge("atc_ws_connections_active")
            .with_description(
                "Number of WebSocket clients currently connected to /v1/ws. \
                 Reflects the count of in-flight `handle_socket` tasks.",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = active_weak.upgrade() {
                    observer.observe(atomic.load(Ordering::Acquire), &[]);
                }
            })
            .build();

        Arc::new(Self {
            lagged_evictions,
            active_connections,
            attrs_committed: [KeyValue::new("channel", "committed")],
            attrs_config: [KeyValue::new("channel", "config")],
        })
    }

    fn record_connection_started(&self) {
        self.active_connections.fetch_add(1, Ordering::Release);
    }

    fn record_connection_ended(&self) {
        self.active_connections.fetch_sub(1, Ordering::Release);
    }

    fn record_lagged(&self, channel: LaggedChannel) {
        let attrs = match channel {
            LaggedChannel::Committed => &self.attrs_committed,
            LaggedChannel::Config => &self.attrs_config,
        };
        self.lagged_evictions.add(1, attrs);
    }
}

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

/// Parse both sides as origins and compare scheme + host + port (using each
/// scheme's well-known default port when unspecified), rather than a raw
/// string compare — an operator who writes `https://atc.example.com:443` in
/// config would otherwise never match a real browser's `Origin` header,
/// which omits the default port.
fn origin_matches(origin: &str, public_origin: &str) -> bool {
    let (Ok(a), Ok(b)) = (
        reqwest::Url::parse(origin),
        reqwest::Url::parse(public_origin),
    ) else {
        return false;
    };
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Axum handler: upgrade HTTP to WebSocket, subscribe to both event channels.
///
/// `auth.mode = "github"`: pre-upgrade, in order — `Origin` must match
/// `auth.github.public_origin` exactly ([`origin_matches`]; missing Origin ⇒
/// reject) and the session must be fresh (`AuthContext::require_fresh`,
/// [`AuthContext::from_request_parts`] having already rejected a
/// missing/expired session with `auth_required`). Non-browser clients that
/// legitimately send no `Origin` header are out of scope for github mode —
/// only browsers (the only clients that carry the ambient session cookie
/// this mode authenticates) are expected to connect. The resolved
/// `AuthContext` is snapshotted into the connection task once, here, and
/// never re-checked for the life of the connection — mid-stream
/// revocation/staleness is explicitly out of scope (locked decision; a
/// revoked or newly-stale session keeps streaming until the connection
/// ends for an unrelated reason — lag eviction, config reload, shutdown, or
/// a client-initiated reconnect — not on any bounded cadence).
///
/// `mode = "none"`: no Origin check, no auth, no filtering — bit-for-bit
/// today's path (`AuthContext::Disabled` never touches `SessionStore`).
pub async fn ws_handler(
    ctx: AuthContext,
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
) -> Response {
    if matches!(ctx, AuthContext::Session(_)) {
        let auth = state
            .auth
            .as_ref()
            .expect("AuthContext::Session implies auth.mode = github");
        let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
        if !origin.is_some_and(|o| origin_matches(o, &auth.public_origin)) {
            tracing::debug!(
                cause = "origin_mismatch",
                origin = ?origin,
                "WS upgrade rejected pre-upgrade"
            );
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    // `require_fresh` only ever fails with `AuthRejection::Stale` — a
    // missing/expired session is already rejected with `auth_required` by
    // `AuthContext`'s own extraction, before this handler body runs at all.
    // `AuthRejection::into_response` already traces this rejection (`reason
    // = "stale_authorization"`); no separate log here avoids double-logging
    // the same event.
    let ctx = match ctx.require_fresh(state.clock.now()) {
        Ok(ctx) => ctx,
        Err(rejection) => return rejection.into_response(),
    };

    let committed_rx = state.persist.subscribe();
    let config_rx = state.config_events_tx.subscribe();
    let shutdown = state.shutdown.clone();
    let ws_metrics = Arc::clone(&state.ws_metrics);
    ws.on_upgrade(move |socket| {
        state.ws_tracker.track_future(handle_socket(
            socket,
            committed_rx,
            config_rx,
            shutdown,
            ws_metrics,
            ctx,
        ))
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
    socket: WebSocket,
    committed_rx: broadcast::Receiver<CommittedEvent>,
    config_rx: broadcast::Receiver<ConfigEvent>,
    shutdown: CancellationToken,
    ws_metrics: Arc<WsMetrics>,
    ctx: AuthContext,
) {
    // One info-span wraps the whole connection lifetime so the trace surfaces
    // close reason + lagged flag as late-bound fields once the loop returns.
    // `ws.close_reason` is mandatory on disconnect; `ws.lagged` is recorded
    // only on the lagged-eviction branches.
    let span = tracing::info_span!(
        "ws.connection",
        ws.close_reason = tracing::field::Empty,
        ws.lagged_channel = tracing::field::Empty,
    );
    handle_socket_inner(socket, committed_rx, config_rx, shutdown, ws_metrics, ctx)
        .instrument(span)
        .await;
}

async fn handle_socket_inner(
    mut socket: WebSocket,
    mut committed_rx: broadcast::Receiver<CommittedEvent>,
    mut config_rx: broadcast::Receiver<ConfigEvent>,
    shutdown: CancellationToken,
    ws_metrics: Arc<WsMetrics>,
    ctx: AuthContext,
) {
    ws_metrics.record_connection_started();
    // Decrement on every exit path via a drop guard. Inline because the only
    // thing it does is the matching `record_connection_ended` call.
    struct ConnectionGuard<'a>(&'a WsMetrics);
    impl<'a> Drop for ConnectionGuard<'a> {
        fn drop(&mut self) {
            self.0.record_connection_ended();
        }
    }
    let _conn_guard = ConnectionGuard(&ws_metrics);

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
                        ws_metrics.record_lagged(LaggedChannel::Config);
                        tracing::Span::current().record("ws.lagged_channel", "config");
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
                        tracing::warn!(error.message = %e, "WebSocket read error");
                        break "read error";
                    }
                }
            }
            result = committed_rx.recv() => {
                match result {
                    Ok(committed_event) => {
                        let seq = committed_event.seq;
                        // `Disabled` (mode=none): `can_see` is always `true`,
                        // so every event forwards — bit-for-bit today's
                        // behavior. `Session`: dropping a filtered event is
                        // safe (not a break/disconnect) — ADR-0003 places
                        // seq contiguity out of contract, and the frontend
                        // does no gap detection.
                        if !ctx.can_see(committed_event.event.repo_id()) {
                            tracing::debug!(seq, "dropped committed event outside session repo set");
                            continue;
                        }
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
                        ws_metrics.record_lagged(LaggedChannel::Committed);
                        tracing::Span::current().record("ws.lagged_channel", "committed");
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

    tracing::Span::current().record("ws.close_reason", reason);
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
            tracing::error!(error.message = %e, "failed to serialize WireFrame; skipping");
            return Ok(());
        }
    };
    if socket.send(Message::Text(json.into())).await.is_err() {
        return Err("send failed");
    }
    Ok(())
}
