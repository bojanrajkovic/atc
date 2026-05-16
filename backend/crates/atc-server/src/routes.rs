use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use axum_otel_metrics::HttpMetricsLayerBuilder;
use opentelemetry_http::HeaderExtractor;
use serde::Serialize;
use tracing::{Instrument, Span, field, info_span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use atc_core::PersistError;
use atc_github::{ParseResult, parse_webhook, verify_signature};

use crate::state::AppState;
use crate::ws;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.shutdown.is_cancelled() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(HealthResponse {
                status: "shutting_down",
            }),
        )
            .into_response();
    }
    // Dispatch liveness check through the persist layer.
    // PgStore: SELECT 1 + drain heartbeat staleness check.
    // InMemoryStore: always Ok.
    match state.persist.liveness_check().await {
        Ok(()) => (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response(),
        Err(atc_persist::LivenessError::DbUnreachable(e)) => {
            tracing::warn!(error = %e, "readyz: db check failed");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "db_unreachable",
                }),
            )
                .into_response()
        }
        Err(atc_persist::LivenessError::DrainStale { age_ms }) => {
            tracing::warn!(age_ms, "readyz: drain heartbeat stale");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "drain_stale",
                }),
            )
                .into_response()
        }
    }
}

/// Return current state snapshot with lastSeq cursor.
///
/// Dispatches uniformly through `state.persist.read_snapshot()`.
///
/// For `PgStore`: loads `broadcast_watermark` (Acquire) BEFORE the REPEATABLE
/// READ snapshot — the drain's commit-order cursor ensures every seq ≤ lastSeq
/// is visible in the snapshot (see ADR 0002).
///
/// For `InMemoryStore`: locks seq across snapshot + seq read so the cursor
/// matches snapshot content.
async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let span = info_span!(
        "state.snapshot",
        http.route = "/v1/state",
        snapshot.runs_count = field::Empty,
        snapshot.jobs_count = field::Empty,
        snapshot.last_seq = field::Empty,
    );
    async move {
        match state.persist.read_snapshot().await {
            Ok(mut snap) => {
                // Compose operator-declared pool capacities from `AppState`
                // onto the persistent-store-derived snapshot. The store trait
                // owns event-derived state only; capacity is config, surfaced
                // here so the snapshot rail carries everything the frontend
                // needs for its first render.
                snap.runner_pool_capacities = state.runner_pool_capacities.read().await.clone();
                let current = tracing::Span::current();
                current.record("snapshot.runs_count", snap.runs.len());
                current.record("snapshot.jobs_count", snap.jobs.len());
                current.record("snapshot.last_seq", snap.last_seq);
                tracing::debug!(
                    last_seq = snap.last_seq,
                    runs_count = snap.runs.len(),
                    jobs_count = snap.jobs.len(),
                    "state snapshot served"
                );
                Json(snap).into_response()
            }
            Err(e) => {
                tracing::error!(error = ?e, "state_handler: snapshot failed");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "snapshot failed"})),
                )
                    .into_response()
            }
        }
    }
    .instrument(span)
    .await
}

/// Handler for removed endpoints that should return 404.
/// `/health` was renamed to `/healthz` and has no backward-compat alias.
async fn removed_endpoint_404() -> StatusCode {
    StatusCode::NOT_FOUND
}

/// API routes. Mount these before the asset fallback.
///
/// The HTTP metrics layer (`axum-otel-metrics::HttpMetricsLayer`) reads from
/// the global meter provider configured by `otel::init_otel`. When OTel is
/// disabled the layer captures the SDK's no-op meter and emissions never reach
/// an exporter — request handling itself is unaffected.
///
/// Returns a `Router<Arc<AppState>>` that will be attached to application state
/// in `main.rs` via `.with_state()`.
pub fn api_routes() -> Router<Arc<AppState>> {
    let http_metrics = HttpMetricsLayerBuilder::new().build();
    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/state", get(state_handler))
        .route("/v1/webhooks/github", post(webhook_handler))
        .route("/v1/ws", get(ws::ws_handler))
        // Removed endpoints: explicitly return 404 instead of falling through
        // to the SPA fallback (which would serve index.html with status 200
        // and silently mislead scrapers that still hit these paths).
        .route("/health", get(removed_endpoint_404))
        .route("/metrics", get(removed_endpoint_404))
        .layer(http_metrics)
}

/// Handle incoming GitHub webhook payloads.
///
/// Verifies HMAC signature (when configured) and parses the payload into a
/// domain event. Dispatches to `AppState.persist` (either `PgStore` or
/// `InMemoryStore`) for storage and returns a unified response:
///
/// - **Success**: `{"status":"accepted","seq":<u64>}` — event applied, seq allocated.
/// - **Invalid transition**: `{"status":"rejected"}` — backward/parity rejection.
/// - **Backend error**: 503 + `{"status":"error"}` — transient storage failure.
///
/// Metric counters (`atc_pg_write_failures_total`, `atc_pg_notify_emitted_total`)
/// are emitted by the `PersistentStore` impl (not here).
async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // Build the parent OTel context from incoming headers BEFORE the span
    // exists. `set_parent` errors if called after the span has been entered;
    // the only safe attachment point is between `info_span!` and the first
    // poll of the instrumented future.
    let parent_cx = opentelemetry::global::get_text_map_propagator(|prop| {
        prop.extract(&HeaderExtractor(&headers))
    });

    let span = info_span!(
        "webhook.handler",
        http.route = "/v1/webhooks/github",
        webhook.delivery_id = field::Empty,
        webhook.event_type = field::Empty,
    );
    // Ignore `SetParentError`: the only reason this errors is when no OTel
    // tracing layer is installed (default-disabled posture), in which case the
    // request still runs as a no-op span.
    let _ = span.set_parent(parent_cx);

    async move {
        if let Some(delivery_id) = headers
            .get("x-github-delivery")
            .and_then(|v| v.to_str().ok())
        {
            Span::current().record("webhook.delivery_id", delivery_id);
        }

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
        Span::current().record("webhook.event_type", event_type);

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
            ParseResult::Parsed(boxed_event) => {
                let event = *boxed_event;

                let persist_result = match &event {
                    atc_github::WebhookEvent::Run(env) => {
                        state.persist.apply_run_event(env.clone()).await
                    }
                    atc_github::WebhookEvent::Job(env) => {
                        state.persist.apply_job_event(env.clone()).await
                    }
                };

                match persist_result {
                    Ok(seq) => {
                        tracing::info!(event_type, seq, "event accepted");
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"status": "accepted", "seq": seq})),
                        )
                    }
                    Err(PersistError::InvalidTransition) => {
                        match &event {
                            atc_github::WebhookEvent::Run(env) => {
                                tracing::warn!(
                                    event_type,
                                    run_id = env.run_id.0,
                                    "transition invalid; rejecting"
                                );
                            }
                            atc_github::WebhookEvent::Job(env) => {
                                tracing::warn!(
                                    event_type,
                                    run_id = env.run_id.0,
                                    job_id = env.job_id.0,
                                    "transition invalid; rejecting"
                                );
                            }
                        }
                        (
                            StatusCode::OK,
                            Json(serde_json::json!({"status": "rejected"})),
                        )
                    }
                    Err(PersistError::Backend(e)) => {
                        tracing::error!(error = %e, "persistence write failed");
                        (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(serde_json::json!({"status": "error"})),
                        )
                    }
                }
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
    .instrument(span)
    .await
}
