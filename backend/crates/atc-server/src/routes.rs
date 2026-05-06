use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// `/readyz` 503s if the drain heartbeat is older than this. 30 s is 6× the
/// 5 s drain heartbeat tick, so a healthy task always lands well inside.
const READYZ_HEARTBEAT_STALENESS_MS: i64 = 30_000;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

/// REST state snapshot for client backfill.
///
/// Returned by `GET /v1/state`. `last_seq` is the highest committed sequence
/// number — clients discard buffered WS events with `seq <= last_seq`.
/// A snapshot at `last_seq: N` reflects all committed events with event seq <= N.
#[derive(Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
struct StateSnapshot {
    last_seq: u64,
    runs: Vec<atc_core::WorkflowRun>,
    jobs: Vec<atc_core::Job>,
}

async fn healthz() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        if let Err(e) = sqlx::query("SELECT 1").execute(pool).await {
            tracing::warn!(error = %e, "readyz: db check failed");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "db_unreachable",
                }),
            )
                .into_response();
        }
        // PG up — also require a fresh drain heartbeat. The drain task ticks
        // its heartbeat every 5 s (HEARTBEAT_TICK in listener.rs) regardless
        // of NOTIFY arrival, so any value older than 30 s indicates the task
        // has stalled.
        let last = state.last_drain_pass_at.load(Ordering::Relaxed);
        let age = now_millis().saturating_sub(last);
        if age > READYZ_HEARTBEAT_STALENESS_MS {
            tracing::warn!(age_ms = age, "readyz: drain heartbeat stale");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthResponse {
                    status: "drain_stale",
                }),
            )
                .into_response();
        }
        (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
    } else {
        (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
    }
}

/// Return current state snapshot with lastSeq cursor.
///
/// PG mode: opens a single REPEATABLE READ transaction and reads runs, jobs,
/// and `MAX(seq)` from the same MVCC snapshot. This guarantees `lastSeq` is a
/// true upper bound on the runs/jobs content — under READ COMMITTED a
/// concurrent commit between the runs SELECT and the seq SELECT could
/// advance `lastSeq` past content that the snapshot hasn't materialized,
/// causing the frontend's `seq > lastSeq` filter at `connection.ts:113` to
/// drop a real event permanently.
///
/// In-memory mode: holds the seq mutex across the store snapshot and the
/// seq read, providing the same content/cursor consistency.
async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if let Some(pool) = &state.pg_pool {
        // PG path: REPEATABLE READ tx around the three reads.
        let mut tx = match pool.begin().await {
            Ok(tx) => tx,
            Err(e) => {
                tracing::error!(error = %e, "state_handler: pg begin failed");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "database unavailable"})),
                )
                    .into_response();
            }
        };
        if let Err(e) = sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
        {
            tracing::error!(error = %e, "state_handler: failed to set isolation level");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "database error"})),
            )
                .into_response();
        }

        let runs = match crate::persist::read_all_runs(&mut tx).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = ?e, "state_handler: read_all_runs failed");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "database error"})),
                )
                    .into_response();
            }
        };
        let jobs = match crate::persist::read_all_jobs(&mut tx).await {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = ?e, "state_handler: read_all_jobs failed");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "database error"})),
                )
                    .into_response();
            }
        };
        let max_seq = match crate::persist::read_last_seq(&mut tx).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = ?e, "state_handler: read_last_seq failed");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({"error": "database error"})),
                )
                    .into_response();
            }
        };
        if let Err(e) = tx.commit().await {
            tracing::warn!(error = %e, "state_handler: pg commit failed");
            // Reads succeeded; fall through and return them. A failed commit
            // on a read-only REPEATABLE READ tx is non-fatal for the response.
        }

        let last_seq = u64::try_from(max_seq).unwrap_or(0);
        Json(StateSnapshot {
            last_seq,
            runs,
            jobs,
        })
        .into_response()
    } else {
        // In-memory path: unchanged from pre-3c.
        let seq_guard = state.seq.lock().await;
        let result = state.store.snapshot().await;
        let last_seq = *seq_guard;
        drop(seq_guard);

        Json(StateSnapshot {
            last_seq,
            runs: result.runs,
            jobs: result.jobs,
        })
        .into_response()
    }
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

            match &state.pg_pool {
                Some(pool) => {
                    // ── PG mode: write-only handler (Phase 3c) ───────────
                    // No seq mutex, no in-memory apply, no broadcast — the
                    // drain task is the sole writer to webhook_tx in PG mode.
                    // The handler's job is to commit the outbox row; the
                    // BIGSERIAL seq comes from PG, the listener gets a NOTIFY
                    // on commit, and the drain task picks the row up and
                    // broadcasts.
                    let mut tx = match pool.begin().await {
                        Ok(tx) => tx,
                        Err(e) => {
                            metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                                .increment(1);
                            tracing::error!(error = %e, "pg begin failed");
                            return (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(
                                    serde_json::json!({"status": "error", "message": "database unavailable"}),
                                ),
                            );
                        }
                    };

                    let mut notify_kind: Option<&'static str> = None;
                    let mut allocated_seq: Option<i64> = None;
                    let txn_result: Result<(), PersistError> = async {
                        match &event {
                            atc_github::WebhookEvent::Run(env) => {
                                crate::persist::upsert_run_in_txn(&mut tx, env).await?;
                                let seq =
                                    crate::persist::insert_outbox_run_in_txn(&mut tx, env).await?;
                                crate::persist::notify_outbox_seq_in_txn(&mut tx, seq).await?;
                                notify_kind = Some("run");
                                allocated_seq = Some(seq);
                            }
                            atc_github::WebhookEvent::Job(env) => {
                                crate::persist::upsert_job_in_txn(&mut tx, env).await?;
                                let seq =
                                    crate::persist::insert_outbox_job_in_txn(&mut tx, env).await?;
                                crate::persist::notify_outbox_seq_in_txn(&mut tx, seq).await?;
                                notify_kind = Some("job");
                                allocated_seq = Some(seq);
                            }
                        }
                        Ok(())
                    }
                    .await;

                    match txn_result {
                        Ok(()) => {
                            if let Err(e) = tx.commit().await {
                                metrics::counter!(
                                    "atc_pg_write_failures_total",
                                    "kind" => "transient"
                                )
                                .increment(1);
                                tracing::error!(error = %e, "pg commit failed");
                                return (
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    Json(
                                        serde_json::json!({"status": "error", "message": "database commit failed"}),
                                    ),
                                );
                            }
                            if let Some(kind) = notify_kind {
                                metrics::counter!("atc_pg_notify_emitted_total", "kind" => kind)
                                    .increment(1);
                            }
                            tracing::info!(
                                event_type,
                                seq = ?allocated_seq,
                                "event accepted (pg outbox; drain will broadcast)",
                            );
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({
                                    "status": "accepted",
                                    "seq": allocated_seq,
                                })),
                            )
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
                            (
                                StatusCode::OK,
                                Json(serde_json::json!({"status": "rejected"})),
                            )
                        }
                        Err(PersistError::Backend(e)) => {
                            metrics::counter!(
                                "atc_pg_write_failures_total",
                                "kind" => "transient"
                            )
                            .increment(1);
                            tracing::error!(error = %e, "pg backend failure mid-txn");
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(
                                    serde_json::json!({"status": "error", "message": "database error"}),
                                ),
                            )
                        }
                    }
                }
                None => {
                    // ── In-memory-only path ───────────────────────────────
                    // Unchanged: seq mutex held across mutation + broadcast so
                    // WS event order matches commit order and /v1/state cursor
                    // matches snapshot content.
                    let mut seq_guard = state.seq.lock().await;
                    let should_broadcast = match &event {
                        atc_github::WebhookEvent::Run(envelope) => {
                            match state.store.apply_run_event(envelope.clone()).await {
                                Ok(_) => true,
                                Err(e) => {
                                    tracing::warn!(error = %e, "store run transition warning");
                                    false
                                }
                            }
                        }
                        atc_github::WebhookEvent::Job(envelope) => {
                            match state.store.apply_job_event(envelope.clone()).await {
                                Ok(_) => true,
                                Err(e) => {
                                    tracing::warn!(error = %e, "store job transition warning");
                                    false
                                }
                            }
                        }
                    };

                    if should_broadcast {
                        *seq_guard += 1;
                        let seq = *seq_guard;
                        let seq_event = SeqEvent { seq, event };
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
