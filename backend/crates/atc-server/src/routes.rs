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

use atc_core::PersistError;
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
        ParseResult::Parsed(boxed_event) => {
            // Unbox once so we can match by reference and move into SeqEvent at the end.
            let event = *boxed_event;

            // Acquire the seq mutex BEFORE any PG I/O. This serializes the
            // critical section so that:
            // (a) WS event seq values match durable commit order, and
            // (b) GET /v1/state cannot read between mutation and seq bump.
            //
            // The mutex must be held across pool.begin() (not just commit) so
            // that two concurrent webhooks cannot commit in one order and
            // broadcast in the reverse order.
            let mut seq_guard = state.seq.lock().await;

            match &state.pg_pool {
                Some(pool) => {
                    // ── Transactional PG path ────────────────────────────
                    // Begin transaction (seq mutex already held).
                    let mut tx = match pool.begin().await {
                        Ok(tx) => tx,
                        Err(e) => {
                            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                                .increment(1);
                            tracing::error!(error = %e, "pg begin failed");
                            drop(seq_guard);
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(
                                    serde_json::json!({"status": "error", "message": "database unavailable"}),
                                ),
                            );
                        }
                    };

                    // UPSERT + outbox INSERT + NOTIFY inside the transaction.
                    let mut notify_kind: Option<&'static str> = None;
                    let txn_result: Result<(), PersistError> = async {
                        match &event {
                            atc_github::WebhookEvent::Run(env) => {
                                crate::persist::upsert_run_in_txn(&mut tx, env).await?;
                                let seq =
                                    crate::persist::insert_outbox_run_in_txn(&mut tx, env).await?;
                                crate::persist::notify_outbox_seq_in_txn(&mut tx, seq).await?;
                                notify_kind = Some("run");
                            }
                            atc_github::WebhookEvent::Job(env) => {
                                crate::persist::upsert_job_in_txn(&mut tx, env).await?;
                                let seq =
                                    crate::persist::insert_outbox_job_in_txn(&mut tx, env).await?;
                                crate::persist::notify_outbox_seq_in_txn(&mut tx, seq).await?;
                                notify_kind = Some("job");
                            }
                        }
                        Ok(())
                    }
                    .await;

                    match txn_result {
                        Ok(()) => {
                            // Commit the transaction.
                            if let Err(e) = tx.commit().await {
                                metrics::counter!(
                                    "atc_pg_write_failures_total",
                                    "kind" => "transient"
                                )
                                .increment(1);
                                tracing::error!(error = %e, "pg commit failed");
                                drop(seq_guard);
                                return (
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    Json(
                                        serde_json::json!({"status": "error", "message": "database commit failed"}),
                                    ),
                                );
                            }
                            // PG committed — emit NOTIFY metric and fall through to in-memory apply.
                            if let Some(kind) = notify_kind {
                                metrics::counter!("atc_pg_notify_emitted_total", "kind" => kind)
                                    .increment(1);
                            }
                        }
                        Err(PersistError::InvalidTransition) => {
                            // tx drops here → auto-rollback. No outbox row written.
                            metrics::counter!(
                                "atc_pg_write_failures_total",
                                "kind" => "parity"
                            )
                            .increment(1);
                            tracing::warn!(
                                "pg parity rejection: transition invalid under predicate"
                            );
                            drop(seq_guard);
                            return (
                                StatusCode::OK,
                                Json(serde_json::json!({"status": "rejected"})),
                            );
                        }
                        Err(PersistError::Backend(e)) => {
                            metrics::counter!(
                                "atc_pg_write_failures_total",
                                "kind" => "transient"
                            )
                            .increment(1);
                            tracing::error!(error = %e, "pg backend failure mid-txn");
                            drop(seq_guard);
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(
                                    serde_json::json!({"status": "error", "message": "database error"}),
                                ),
                            );
                        }
                    }

                    // ── Apply to in-memory store (still under mutex) ──────
                    // PG committed. Apply to in-memory store and broadcast.
                    let broadcast_event: Option<Option<Vec<atc_core::RunnerPoolStats>>> =
                        match &event {
                            atc_github::WebhookEvent::Run(env) => {
                                match state.store.apply_run_event(env.clone()).await {
                                    Ok(_) => Some(None),
                                    Err(e) => {
                                        metrics::counter!("atc_pg_in_memory_drift_total")
                                            .increment(1);
                                        tracing::warn!(error = %e, "post-commit in-memory drift (run)");
                                        None
                                    }
                                }
                            }
                            atc_github::WebhookEvent::Job(env) => {
                                match state.store.apply_job_event(env.clone()).await {
                                    Ok(_) => Some(Some(state.store.pool_stats().await)),
                                    Err(e) => {
                                        metrics::counter!("atc_pg_in_memory_drift_total")
                                            .increment(1);
                                        tracing::warn!(error = %e, "post-commit in-memory drift (job)");
                                        None
                                    }
                                }
                            }
                        };

                    if let Some(pool_stats_after) = broadcast_event {
                        let seq = *seq_guard;
                        *seq_guard += 1;
                        let seq_event = SeqEvent {
                            seq,
                            event,
                            pool_stats_after,
                        };
                        let _ = state.webhook_tx.send(seq_event);
                        tracing::info!(event_type, seq, "event processed (pg+mem)");
                    } else {
                        tracing::info!(
                            event_type,
                            "pg committed but in-memory drift (no broadcast)"
                        );
                    }

                    drop(seq_guard);
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({"status": "processed"})),
                    )
                }
                None => {
                    // ── In-memory-only path ───────────────────────────────
                    // No PG pool configured. Apply directly to in-memory store.
                    // Seq mutex is already held.
                    //
                    // Only assign seq and broadcast on successful store mutation.
                    // Failed transitions should not produce SeqEvents — clients
                    // must never receive events that aren't reflected in the store.
                    let pool_stats_after: Option<Option<Vec<atc_core::RunnerPoolStats>>> =
                        match &event {
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
                        let seq_event = SeqEvent {
                            seq,
                            event,
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
