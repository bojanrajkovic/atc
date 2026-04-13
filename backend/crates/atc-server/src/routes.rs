use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use axum_prometheus::PrometheusMetricLayer;
use serde::Serialize;

use atc_github::{ParseResult, parse_webhook, verify_signature};

use crate::state::{AppState, SeqEvent};
use crate::ws;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// REST state snapshot for client backfill.
///
/// Returned by `GET /v1/state`. `seq` is the next sequence number to
/// assign — clients discard buffered WS events with `seq < snapshot_seq`.
/// A snapshot at `seq: N` reflects all committed events with event seq < N.
#[derive(Serialize)]
struct StateSnapshot {
    seq: u64,
    runs: Vec<atc_core::WorkflowRun>,
    jobs: Vec<atc_core::Job>,
    pool_stats: Vec<atc_core::RunnerPoolStats>,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Return current state snapshot with seq cursor.
///
/// Reads store state via `StateStore::snapshot()` (single RwLock
/// acquisition), then reads the atomic seq counter. The counter
/// value is the next seq to assign — exactly what REST returns.
///
/// Because the webhook handler applies events to the store BEFORE
/// incrementing seq (with Release ordering), a snapshot at seq: N
/// is guaranteed to reflect all events with event seq < N.
async fn state_handler(
    State(state): State<Arc<AppState>>,
) -> Json<StateSnapshot> {
    let (result, pool_stats) = state.store.snapshot().await;
    let seq = state.seq.load(Ordering::Acquire);

    Json(StateSnapshot {
        seq,
        runs: result.runs,
        jobs: result.jobs,
        pool_stats,
    })
}

/// Handler for removed endpoints that should return 404.
/// `/health` was renamed to `/healthz` in phase 1 and has no backward-compat alias.
async fn removed_endpoint_404() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// API routes. Mount these before the asset fallback.
///
/// `prometheus_layer` is applied here so that every request to the main router
/// is counted in `axum_http_requests_total`.
///
/// Returns a `Router<Arc<AppState>>` that will be attached to application state
/// in `main.rs` via `.with_state()`.
pub fn api_routes(prometheus_layer: PrometheusMetricLayer<'static>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/state", get(state_handler))
        .route("/v1/webhooks/github", post(webhook_handler))
        .route("/v1/ws", get(ws::ws_handler))
        // Removed endpoints: explicitly return 404 instead of falling through to SPA
        .route("/health", get(removed_endpoint_404))
        .layer(prometheus_layer)
}

/// Handle incoming GitHub webhook payloads.
///
/// Verifies HMAC signature (when configured), parses the payload into domain
/// events, applies them to the state store, assigns a monotonic seq number,
/// and broadcasts to WebSocket clients.
#[tracing::instrument(skip(state, body))]
async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // 1. Extract X-GitHub-Event header
    let event_type = match headers.get("x-github-event").and_then(|v| v.to_str().ok()) {
        Some(et) => et,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "missing X-GitHub-Event header"})),
            );
        }
    };

    tracing::debug!(event_type, "webhook received");

    // 2. Verify HMAC-SHA256 signature if secret is configured
    if let Some(ref secret) = state.webhook_secret {
        let signature = match headers
            .get("x-hub-signature-256")
            .and_then(|v| v.to_str().ok())
        {
            Some(sig) => sig,
            None => {
                tracing::warn!("missing X-Hub-Signature-256 header");
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "missing X-Hub-Signature-256 header"})),
                );
            }
        };

        if let Err(_e) = verify_signature(secret.as_bytes(), &body, signature) {
            tracing::warn!("HMAC verification failed");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "invalid signature"})),
            );
        }
    }

    // 3. Parse webhook payload
    let result = match parse_webhook(event_type, &body) {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(error = %e, event_type, "webhook parse error");
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(serde_json::json!({"error": e.to_string()})),
            );
        }
    };

    // 4. Handle parse result
    match result {
        ParseResult::Parsed(event) => {
            // Apply to store. StoreError variants are currently all
            // deterministic (invalid state transitions) — retrying would
            // fail identically, so we return 200 to prevent GitHub retries.
            //
            // Only assign seq and broadcast on successful store mutation.
            // Failed transitions should not produce SeqEvents — clients
            // must never receive events that aren't reflected in the store.
            //
            // Future: when state externalizes to a database, transient
            // failures (connection errors, deadlocks) should return 5xx
            // so GitHub retries. Add a new StoreError variant for those
            // and take a different branch here.
            let store_ok = match &*event {
                atc_github::WebhookEvent::Run(envelope) => state
                    .store
                    .apply_run_event(envelope.clone())
                    .await
                    .map_err(|e| tracing::warn!(error = %e, "store run transition warning"))
                    .is_ok(),
                atc_github::WebhookEvent::Job(envelope) => state
                    .store
                    .apply_job_event(envelope.clone())
                    .await
                    .map_err(|e| tracing::warn!(error = %e, "store job transition warning"))
                    .is_ok(),
            };

            if store_ok {
                // Assign seq AFTER successful store mutation. fetch_add
                // returns the OLD value — this becomes the event's assigned
                // seq (0 for the first event, 1 for the second, etc.).
                //
                // The REST handler returns the live counter value (next seq
                // to assign). After N events, counter = N. Clients discard
                // buffered WS events with seq < snapshot_seq.
                let seq = state.seq.fetch_add(1, Ordering::Release);

                // Broadcast to WebSocket clients. Ignore send error (no receivers).
                let seq_event = SeqEvent { seq, event: *event };
                let _ = state.webhook_tx.send(seq_event);

                tracing::info!(event_type, seq, "event processed");
            } else {
                tracing::info!(event_type, "event accepted (transition already applied)");
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "processed"})),
            )
        }
        ParseResult::Skipped { ref event_type } => {
            tracing::debug!(event_type, "event skipped");
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "skipped"})),
            )
        }
    }
}
