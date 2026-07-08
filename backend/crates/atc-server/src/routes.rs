use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    Json, Router,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_otel_metrics::HttpMetricsLayerBuilder;
use opentelemetry::trace::TraceContextExt;
use opentelemetry_http::HeaderExtractor;
use serde::Serialize;
use tower_http::trace::TraceLayer;
use tracing::{Instrument, Span, field, info_span};
use tracing_opentelemetry::OpenTelemetrySpanExt;

use atc_core::{PersistError, RunId};
use atc_github::{ParseResult, parse_webhook, verify_signature};

use crate::auth::AuthContext;
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
            tracing::warn!(error.message = %e, "readyz: db check failed");
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
/// Dispatches uniformly through `state.persist.read_snapshot(cutoff)`. The
/// cutoff is computed here from `AppState.clock` and `AppState.display_ttl`
/// — the store trait stays config-agnostic (ADR-0008) and the cutoff is
/// the only event-vs-config interaction on the read path.
///
/// For `PgStore`: loads `broadcast_watermark` (Acquire) BEFORE the REPEATABLE
/// READ snapshot — the drain's commit-order cursor ensures every seq ≤ lastSeq
/// is visible in the snapshot (see ADR 0002).
///
/// For `InMemoryStore`: locks seq across snapshot + seq read so the cursor
/// matches snapshot content.
///
/// `auth.mode = "github"`: fails closed on a stale session (`AuthContext::require_fresh`)
/// before the read, then filters `runs`/`jobs` in-memory, post-read, to the
/// session's authorized repo set — never in SQL (locked; keeps the read path
/// and store trait untouched). `mode = "none"` is unaffected — see
/// `AuthContext::can_see`.
async fn state_handler(ctx: AuthContext, State(state): State<Arc<AppState>>) -> Response {
    let span = info_span!(
        "state.snapshot",
        http.route = "/v1/state",
        snapshot.runs_count = field::Empty,
        snapshot.jobs_count = field::Empty,
        snapshot.last_seq = field::Empty,
    );
    async move {
        let now = state.clock.now();

        // Fail closed on a missing/expired session before touching the
        // store at all; `Disabled` (mode=none) passes through unchanged.
        // `auth_required` is already handled by the `AuthContext` extractor
        // itself — this only ever surfaces `stale_authorization`.
        let ctx = match ctx.require_fresh(now, "state") {
            Ok(ctx) => ctx,
            Err(rejection) => {
                state
                    .auth_metrics
                    .record_rejection("state", "stale_authorization");
                return rejection.into_response();
            }
        };

        // Compute the display-TTL cutoff once per request. The 60s startup
        // floor and the use of `std::time::Duration` make this conversion
        // infallible for any realistic configured value — chrono's
        // `TimeDelta` range comfortably exceeds humantime-parseable inputs.
        let display_ttl_chrono = chrono::Duration::from_std(state.display_ttl)
            .expect("display_ttl fits chrono::Duration");
        let cutoff = now - display_ttl_chrono;

        match state.persist.read_snapshot(Some(cutoff)).await {
            Ok(mut snap) => {
                // Compose operator-declared pool capacities from `AppState`
                // onto the persistent-store-derived snapshot. The store trait
                // owns event-derived state only; capacity is config, surfaced
                // here so the snapshot rail carries everything the frontend
                // needs for its first render.
                snap.runner_pool_capacities = state.runner_pool_capacities.read().await.clone();
                // Stamp the configured TTL onto the snapshot so the frontend
                // can age out completed rows reactively against
                // `uiStore.nowMs`. `u32::try_from` is defensive — any
                // realistic humantime-parseable value fits in `u32::MAX`
                // seconds (~136 years).
                snap.display_ttl_seconds =
                    u32::try_from(state.display_ttl.as_secs()).unwrap_or(u32::MAX);
                // `Disabled` (mode=none): `can_see` is always `true`, so this
                // retain is a byte-for-byte no-op — `AuthContext::Session`
                // filters `runs` to the session's authorized repo set (a run
                // with no `repo_id`, e.g. a pre-migration row, is never
                // visible to an authenticated session) and `jobs` through
                // their parent run, since jobs carry no repo identity of
                // their own. `lastSeq`, pool capacities, and `displayTtlSeconds`
                // are left untouched — global operator data, visible
                // regardless of the session's repo set.
                //
                // Edge case (two flavors, same root cause): a job can be
                // legitimately visible under mode=none with no matching
                // entry in `snap.runs` — (1) its parent run hasn't arrived
                // yet (webhook job-before-run race; both stores hide the
                // FK-stub/placeholder row but keep the job), or (2) it's a
                // re-run's job at a higher `run_attempt` than its parent
                // row, whose prior attempt already aged past `display_ttl`
                // and was cut off (see `atc-store-pg::reads::read_all_jobs`'s
                // doc comment for both). Either way the job's `repo_id` is
                // only knowable through a run this retain can't see, so it
                // fails closed in auth mode — consistent with "NULL repo_id
                // invisible in auth mode" — and self-heals the moment the
                // run event lands and promotes/advances the row.
                let is_session = matches!(ctx, AuthContext::Session(_));
                if is_session {
                    snap.runs.retain(|r| ctx.can_see(r.repo_id));
                    let kept_run_ids: HashSet<RunId> = snap.runs.iter().map(|r| r.id).collect();
                    snap.jobs.retain(|j| kept_run_ids.contains(&j.run_id));
                }
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
                let mut response = Json(snap).into_response();
                if is_session {
                    // The body now varies per session; block any
                    // intermediary (proxy, CDN) from ever caching or
                    // sharing one session's filtered snapshot with another.
                    response.headers_mut().insert(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("private, no-store"),
                    );
                }
                response
            }
            Err(e) => {
                tracing::error!(error.message = ?e, "state_handler: snapshot failed");
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

/// Blanket per-request root span for every route, including the asset
/// fallback — apply this LAST, after `.fallback(...)`, so it wraps the whole
/// app rather than just `api_routes`. Handler-authored spans (webhook.handler,
/// state.snapshot, ws.connection, auth.callback) nest under it automatically
/// via tracing's ambient span stack; see docs/architecture/metrics.md § "Span
/// inventory". Trivial routes (healthz, readyz, logout, whoami, static
/// assets) had zero trace visibility before this — only the
/// axum-otel-metrics duration histogram saw them.
///
/// Honors an incoming W3C `traceparent` header when present and valid (e.g.
/// from a service-mesh sidecar with tracing enabled — see
/// docs/architecture/metrics.md § "W3C trace context propagation"), attaching
/// it as this span's parent so every route benefits uniformly rather than
/// special-casing any one handler. Absent or malformed, the span roots itself
/// normally.
pub fn with_request_tracing(router: Router) -> Router {
    router.layer(
        TraceLayer::new_for_http()
            .make_span_with(|req: &axum::extract::Request| {
                let span = info_span!(
                    "http.request",
                    http.request.method = %req.method(),
                    http.route = %req.uri().path(),
                    http.response.status_code = field::Empty,
                );
                let parent_cx = opentelemetry::global::get_text_map_propagator(|prop| {
                    prop.extract(&HeaderExtractor(req.headers()))
                });
                if parent_cx.span().span_context().is_valid() {
                    let _ = span.set_parent(parent_cx);
                }
                span
            })
            .on_response(
                |res: &axum::response::Response, _latency: std::time::Duration, span: &Span| {
                    span.record("http.response.status_code", res.status().as_u16());
                },
            ),
    )
}

/// API routes. Mount these before the asset fallback.
///
/// The HTTP metrics layer (`axum-otel-metrics::HttpMetricsLayer`) reads from
/// the global meter provider configured by `otel::init_otel`. When OTel is
/// disabled the layer captures the SDK's no-op meter and emissions never reach
/// an exporter — request handling itself is unaffected.
///
/// `auth_enabled` mirrors `auth.mode = "github"` (see `AppState::auth`):
/// when `false`, the two `/v1/auth/github/*` routes are never merged into
/// the router — a request to them 404s the same way any unmounted path
/// does, rather than being handled by a runtime mode check inside the
/// handlers themselves.
///
/// Returns a `Router<Arc<AppState>>` that will be attached to application state
/// in `main.rs` via `.with_state()`.
pub fn api_routes(auth_enabled: bool) -> Router<Arc<AppState>> {
    let http_metrics = HttpMetricsLayerBuilder::new().build();
    let mut router = Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/v1/state", get(state_handler))
        .route("/v1/webhooks/github", post(webhook_handler))
        .route("/v1/ws", get(ws::ws_handler))
        // Removed endpoints: explicitly return 404 instead of falling through
        // to the SPA fallback (which would serve index.html with status 200
        // and silently mislead scrapers that still hit these paths).
        .route("/health", get(removed_endpoint_404))
        .route("/metrics", get(removed_endpoint_404));
    if auth_enabled {
        router = router.merge(crate::auth::auth_routes());
    }
    router.layer(http_metrics)
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
    request: Request,
) -> (StatusCode, Json<serde_json::Value>) {
    // Read the body by hand (rather than via the `Bytes` extractor) so the
    // read gets its own span with the byte count attached — the extractor
    // form buffers the whole body before this function even starts, so that
    // time (tens of ms for large `workflow_run` deliveries relayed through
    // the homelab ingress chain) used to show up only as unattributed idle
    // time on the outer `http.request` span. `usize::MAX` doesn't disable
    // the size cap: axum's automatic `DefaultBodyLimit` (2 MiB) already wraps
    // the body before this runs, so exceeding it surfaces as an `Err` below
    // either way.
    let (parts, body) = request.into_parts();
    let headers: HeaderMap = parts.headers;
    let body_span = info_span!(
        "webhook.body_read",
        http.route = "/v1/webhooks/github",
        http.request.body.size = field::Empty,
    );
    let body = match async {
        let result = to_bytes(body, usize::MAX).await;
        if let Ok(ref b) = result {
            Span::current().record("http.request.body.size", b.len());
        }
        result
    }
    .instrument(body_span)
    .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "failed to read webhook request body");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "failed to read request body"})),
            );
        }
    };

    let span = info_span!(
        "webhook.handler",
        http.route = "/v1/webhooks/github",
        http.request.method = "POST",
        http.response.status_code = field::Empty,
        webhook.delivery_id = field::Empty,
        webhook.event_type = field::Empty,
    );

    async move {
        let response: (StatusCode, Json<serde_json::Value>) = 'response: {
            // Captured once and reused on the boundary log lines below (in
            // addition to the span field), so `delivery_id` is present even in
            // pretty (non-span-list) log output.
            let delivery_id = headers
                .get("x-github-delivery")
                .and_then(|v| v.to_str().ok());
            if let Some(d) = delivery_id {
                Span::current().record("webhook.delivery_id", d);
            }

            // 1. Extract X-GitHub-Event header
            let event_type = match headers.get("x-github-event").and_then(|v| v.to_str().ok()) {
                Some(et) => et,
                None => {
                    break 'response (
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
                        tracing::warn!(delivery_id = delivery_id.unwrap_or_default(), "missing X-Hub-Signature-256 header");
                        break 'response (
                            StatusCode::UNAUTHORIZED,
                            Json(
                                serde_json::json!({"error": "missing X-Hub-Signature-256 header"}),
                            ),
                        );
                    }
                };

                if let Err(_e) = verify_signature(secret.as_bytes(), &body, signature) {
                    tracing::warn!(delivery_id = delivery_id.unwrap_or_default(), "HMAC verification failed");
                    break 'response (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({"error": "invalid signature"})),
                    );
                }
            }

            // 3. Parse webhook payload
            let result = match parse_webhook(event_type, &body) {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(error.message = %e, event_type, delivery_id = delivery_id.unwrap_or_default(), "webhook parse error");
                    break 'response (
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
                            match &event {
                                atc_github::WebhookEvent::Run(env) => {
                                    tracing::info!(
                                        event_type,
                                        seq,
                                        run_id = env.run_id.0,
                                        delivery_id = delivery_id.unwrap_or_default(),
                                        "event accepted"
                                    );
                                }
                                atc_github::WebhookEvent::Job(env) => {
                                    tracing::info!(
                                        event_type,
                                        seq,
                                        run_id = env.run_id.0,
                                        job_id = env.job_id.0,
                                        delivery_id = delivery_id.unwrap_or_default(),
                                        "event accepted"
                                    );
                                }
                            }
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
                                        delivery_id = delivery_id.unwrap_or_default(),
                                        "transition invalid; rejecting"
                                    );
                                }
                                atc_github::WebhookEvent::Job(env) => {
                                    tracing::warn!(
                                        event_type,
                                        run_id = env.run_id.0,
                                        job_id = env.job_id.0,
                                        delivery_id = delivery_id.unwrap_or_default(),
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
                            tracing::error!(error.message = %e, event_type, delivery_id = delivery_id.unwrap_or_default(), "persistence write failed");
                            (
                                StatusCode::SERVICE_UNAVAILABLE,
                                Json(serde_json::json!({"status": "error"})),
                            )
                        }
                    }
                }
                ParseResult::Ping => {
                    tracing::info!(event_type, delivery_id = delivery_id.unwrap_or_default(), "ping received");
                    (StatusCode::OK, Json(serde_json::json!({"status": "ok"})))
                }
                ParseResult::Skipped { ref event_type } => {
                    tracing::info!(event_type, delivery_id = delivery_id.unwrap_or_default(), "event skipped");
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({"status": "skipped"})),
                    )
                }
            }
        };
        // Single exit point — record the final status code on the span
        // before returning to axum. `as_u16()` is the OTel semconv type
        // (integer status code, not the textual reason phrase).
        Span::current().record("http.response.status_code", response.0.as_u16());
        response
    }
    .instrument(span)
    .await
}
