use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use axum_prometheus::PrometheusMetricLayer;
use serde::{Deserialize, Serialize};

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
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
struct StateSnapshot {
    seq: u64,
    runs: Vec<atc_core::WorkflowRun>,
    jobs: Vec<atc_core::Job>,
    pool_stats: Vec<atc_core::RunnerPoolStats>,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        match sqlx::query("SELECT 1").execute(pool).await {
            Ok(_) => (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response(),
            Err(e) => {
                tracing::warn!(error = %e, "readyz: db check failed");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(HealthResponse {
                        status: "db_unreachable",
                    }),
                )
                    .into_response()
            }
        }
    } else {
        (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
    }
}

/// Return current state snapshot with seq cursor.
///
/// Holds the seq mutex across both the store snapshot and the seq
/// read, ensuring no webhook can commit between them. This
/// guarantees the cursor matches the snapshot content: a response
/// at `seq: N` reflects exactly all events with event seq < N.
async fn state_handler(State(state): State<Arc<AppState>>) -> Json<StateSnapshot> {
    let seq_guard = state.seq.lock().await;
    let (result, pool_stats) = state.store.snapshot().await;
    let seq = *seq_guard;
    drop(seq_guard);

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
            // Hold the seq mutex across store mutation + seq assignment.
            // This serializes the critical section so that:
            // (a) WS event seq values match store commit order, and
            // (b) GET /v1/state cannot read between mutation and seq bump.
            //
            // StoreError variants are currently all deterministic (invalid
            // state transitions) — retrying would fail identically, so we
            // return 200 to prevent GitHub retries.
            //
            // Only assign seq and broadcast on successful store mutation.
            // Failed transitions should not produce SeqEvents — clients
            // must never receive events that aren't reflected in the store.
            let mut seq_guard = state.seq.lock().await;

            // Outer Option: Some(...) = apply succeeded and we should broadcast;
            //               None = apply failed, skip broadcast (AC1.5).
            // Inner Option in Some(...): Some(vec) for Job events (AC1.1),
            //                             None for Run events (AC1.3).
            let pool_stats_after: Option<Option<Vec<atc_core::RunnerPoolStats>>> = match &*event {
                atc_github::WebhookEvent::Run(envelope) => {
                    match state.store.apply_run_event(envelope.clone()).await {
                        Ok(_) => Some(None),
                        Err(e) => {
                            tracing::warn!(error = %e, "store run transition warning");
                            None
                        }
                    }
                }
                atc_github::WebhookEvent::Job(envelope) => {
                    match state.store.apply_job_event(envelope.clone()).await {
                        Ok(_) => Some(Some(state.store.pool_stats().await)),
                        Err(e) => {
                            tracing::warn!(error = %e, "store job transition warning");
                            None
                        }
                    }
                }
            };

            if let Some(pool_stats_after) = pool_stats_after {
                let seq = *seq_guard;
                *seq_guard += 1;

                // Broadcast while still holding the mutex. send() is
                // synchronous (pushes into a bounded ring buffer) so
                // this is cheap. Holding through broadcast ensures that
                // when state_handler acquires the mutex, all events up
                // to that seq have already been broadcast — no window
                // where a snapshot cursor advertises an event that WS
                // clients haven't received yet.
                let seq_event = SeqEvent {
                    seq,
                    event: *event,
                    pool_stats_after,
                };
                let _ = state.webhook_tx.send(seq_event);

                tracing::info!(event_type, seq, "event processed");
            } else {
                tracing::info!(event_type, "event accepted (transition already applied)");
            }

            drop(seq_guard);

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
