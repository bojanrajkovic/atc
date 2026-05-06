//! Integration tests for the webhook-handler transactional write path (Phase 2c).
//!
//! Boots ephemeral Postgres via testcontainers, mounts the full router with
//! a real `pg_pool: Some(pool)`, fires webhook payloads, and asserts atomically
//! consistent PG + in-memory state. Covers Phase 2c behavioral invariants.
//!
//! Requires Docker (or OrbStack) to be running.
//!
//! Phase 2c behavioral contract (differs from Phase 2b shadow writes):
//! - PG failure → 503 SERVICE_UNAVAILABLE (not 200)
//! - Success → both PG run/job row AND outbox row exist (transactional atomicity)
//! - Parity rejection → 200 rejected, transaction rolled back (no outbox row)
//! - In-memory apply follows PG commit — no drift tolerated

mod common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::state::{AppState, SeqEvent};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() installed once per binary.
// All tests in this file must be #[serial_test::serial].
static PROMETHEUS_INIT: OnceLock<(PrometheusMetricLayer<'static>, axum::Router)> = OnceLock::new();

fn prometheus_init() -> &'static (PrometheusMetricLayer<'static>, axum::Router) {
    PROMETHEUS_INIT.get_or_init(atc_server::metrics::build)
}

fn prometheus_layer() -> PrometheusMetricLayer<'static> {
    prometheus_init().0.clone()
}

/// GET /metrics from the side-port router and return the body as a String.
async fn render_metrics() -> String {
    let metrics_router = prometheus_init().1.clone();
    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = metrics_router.oneshot(req).await.unwrap();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// Parse `atc_pg_write_failures_total{kind="<kind>"}` from the Prometheus
/// text output. Returns the integer value, or 0 if the line is absent (counter
/// has never been incremented for that label).
fn parse_counter_value(metrics_body: &str, kind: &str) -> u64 {
    let needle = format!("kind=\"{kind}\"");
    for line in metrics_body.lines() {
        if line.starts_with("atc_pg_write_failures_total")
            && line.contains(&needle)
            && let Some(value_str) = line.split_whitespace().last()
        {
            return value_str.parse::<u64>().unwrap_or(0);
        }
    }
    0
}

/// Build a full router with a real PG pool mounted.
///
/// Returns (router, app_state, broadcast_receiver, pool).
/// The pool is returned so callers can run direct SQL assertions.
pub fn build_app_with_pg(
    pool: sqlx::PgPool,
) -> (
    axum::Router,
    Arc<AppState>,
    tokio::sync::broadcast::Receiver<SeqEvent>,
) {
    let layer = prometheus_layer();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, rx) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: Some(pool),
    });
    let app = atc_server::routes::api_routes(layer)
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state, rx)
}

/// POST a webhook and return the full response (status + body).
async fn post_webhook_full(
    app: axum::Router,
    event_type: &str,
    body: &[u8],
) -> (StatusCode, serde_json::Value) {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", event_type)
        .body(Body::from(body.to_vec()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

/// POST a webhook and return the HTTP response status only.
async fn post_webhook(app: axum::Router, event_type: &str, body: &[u8]) -> StatusCode {
    post_webhook_full(app, event_type, body).await.0
}

// ---------------------------------------------------------------------------
// Transactional success: both run/job row AND outbox row exist after commit
// ---------------------------------------------------------------------------

/// Phase 2c: Full run lifecycle: Queued → InProgress → Completed.
/// At each step, assert in-memory and PG agree and outbox has a new row.
#[tokio::test]
#[serial_test::serial]
async fn transactional_write_run_lifecycle() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    // Requested
    let body = common::fixture_workflow_run_requested();
    let (status, json) = post_webhook_full(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "Requested should return 200");
    assert_eq!(json["status"], "processed", "should be processed");

    let snap = state.store.snapshot().await;
    let mem_run = &snap.runs[0];
    assert_eq!(
        format!("{:?}", mem_run.status),
        "Queued",
        "in-memory should be Queued"
    );

    let pg_row = sqlx::query!("SELECT status FROM runs WHERE id = $1", mem_run.id.0)
        .fetch_optional(&pool)
        .await
        .unwrap();
    let pg_status = pg_row.map(|r| r.status).unwrap_or_default();
    assert_eq!(pg_status, "Queued", "PG should also be Queued");

    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox_count, 1, "outbox should have 1 row after Requested");

    // In-progress
    let body = common::fixture_workflow_run_in_progress();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "InProgress should return 200");

    let snap = state.store.snapshot().await;
    let mem_run = &snap.runs[0];
    assert_eq!(format!("{:?}", mem_run.status), "InProgress");
    let pg_row = sqlx::query!("SELECT status FROM runs WHERE id = $1", mem_run.id.0)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_row.status, "InProgress");

    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        outbox_count, 2,
        "outbox should have 2 rows after InProgress"
    );

    // Completed
    let body = common::fixture_workflow_run_completed();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "Completed should return 200");

    let snap = state.store.snapshot().await;
    let mem_run = &snap.runs[0];
    assert_eq!(format!("{:?}", mem_run.status), "Completed");
    let pg_row = sqlx::query!(
        "SELECT status, conclusion FROM runs WHERE id = $1",
        mem_run.id.0
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pg_row.status, "Completed");
    assert!(pg_row.conclusion.is_some(), "conclusion should be set");

    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox_count, 3, "outbox should have 3 rows after Completed");
}

/// Phase 2c: Job lifecycle with run pre-existing, both stores agree.
#[tokio::test]
#[serial_test::serial]
async fn transactional_write_job_lifecycle() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Pre-insert the run first
    let run_body = common::fixture_workflow_run_requested();
    post_webhook(app.clone(), "workflow_run", &run_body).await;

    // Queued job (job_id=70928200168)
    let job_body = common::fixture_workflow_job_queued();
    let status = post_webhook(app.clone(), "workflow_job", &job_body).await;
    assert_eq!(status, StatusCode::OK);

    let queued_row = sqlx::query!("SELECT status FROM jobs WHERE id = 70928200168")
        .fetch_one(&pool)
        .await
        .expect("queued job row not found in PG");
    assert_eq!(queued_row.status, "Queued");

    // Outbox should have 2 rows (run + job)
    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox_count, 2, "outbox should have 2 rows (run + job)");
}

/// Phase 2c: Job-before-run — fire job first, then run; assert stub then reconciliation.
#[tokio::test]
#[serial_test::serial]
async fn transactional_write_job_before_run_lifecycle() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Fire job first (before run)
    let job_body = common::fixture_workflow_job_queued();
    let status = post_webhook(app.clone(), "workflow_job", &job_body).await;
    assert_eq!(status, StatusCode::OK);

    // One stub run should exist
    let stub_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stub_count, 1, "one stub run should exist");

    let stub_status: String = sqlx::query_scalar("SELECT status FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(stub_status, "Queued", "stub run should be Queued");

    // Outbox has 1 row (the job event)
    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox WHERE kind = 'job'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(outbox_count, 1, "outbox should have 1 job row");

    // Now fire the real run event
    let run_body = common::fixture_workflow_run_requested();
    let status = post_webhook(app.clone(), "workflow_run", &run_body).await;
    assert_eq!(status, StatusCode::OK);

    // Run row should now have real data
    let run_row = sqlx::query!("SELECT head_sha, workflow_name FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(run_row.head_sha, "", "head_sha should be reconciled");

    // Outbox should now have 2 rows total (job + run)
    let total_outbox: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(total_outbox, 2, "outbox should have 2 total rows");
}

/// Phase 2c: Idempotent replay — same webhook twice → both stores stable.
#[tokio::test]
#[serial_test::serial]
async fn transactional_write_idempotent_replay() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    let body = common::fixture_workflow_run_requested();

    let s1 = post_webhook(app.clone(), "workflow_run", &body).await;
    let s2 = post_webhook(app.clone(), "workflow_run", &body).await;

    // Second webhook is a same-status replay (Queued→Queued), which is idempotent.
    // Both should return 200. The second returns "processed" (in-memory accepts idempotent
    // replay) or "rejected" depending on whether the predicate matches.
    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);

    let snap = state.store.snapshot().await;
    assert_eq!(snap.runs.len(), 1, "exactly one run in memory");

    let pg_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_count, 1, "exactly one run in PG");
}

// ---------------------------------------------------------------------------
// Parity rejection: 200 rejected, transaction rolled back, no outbox row
// ---------------------------------------------------------------------------

/// Phase 2c: Parity rejection returns 200 rejected and rolls back the transaction.
/// The parity counter increments and no outbox row is written.
#[tokio::test]
#[serial_test::serial]
async fn parity_metric_increments_when_pg_rejects() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Read the baseline counter value before this test runs.
    let before = render_metrics().await;
    let baseline_parity = parse_counter_value(&before, "parity");

    // 1. Insert run through normal flow (Queued)
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    // 2. Manually advance PG row to Completed (simulates divergence)
    sqlx::query!("UPDATE runs SET status = 'Completed' WHERE status = 'Queued'")
        .execute(&pool)
        .await
        .unwrap();

    // 3. Also reset in-memory to Queued by building a fresh in-memory path equivalent.
    //    (The existing in-memory store already has the run at Queued since it was
    //    applied after the first webhook. We can't easily force it to a different state
    //    without another event.)
    //
    // The scenario: PG has Completed, in-memory has Queued.
    // Fire InProgress: in-memory: Queued→InProgress valid; PG: Completed not in
    // predecessors_of(InProgress) → 0 rows affected → InvalidTransition → parity metric.
    // Phase 2c: returns 200 rejected (parity rejection is not a transient failure).
    let (status, json) = post_webhook_full(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "parity rejection returns 200");
    assert_eq!(json["status"], "rejected", "body must be rejected");

    // PG row should still be Completed (rejected, no update)
    let pg_row = sqlx::query!("SELECT status FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_row.status, "Completed", "PG should still be Completed");

    // No new outbox row from the rejected transaction
    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();
    // The first successful webhook produced 1 outbox row; rejected one adds 0
    assert_eq!(
        outbox_count, 1,
        "rejected transaction must not produce outbox rows (got {outbox_count})"
    );

    // Parity counter must have incremented by exactly 1
    let after = render_metrics().await;
    let after_parity = parse_counter_value(&after, "parity");
    assert_eq!(
        after_parity,
        baseline_parity + 1,
        "parity counter should increment by 1; metrics output:\n{after}"
    );
}

// ---------------------------------------------------------------------------
// Transient PG failure: 503, no in-memory row, no outbox row
// ---------------------------------------------------------------------------

/// Phase 2c: DB outage → 503 SERVICE_UNAVAILABLE; no in-memory row; no outbox row.
///
/// Unlike Phase 2b (shadow mode: 200 always, drift tolerated), Phase 2c returns 503
/// on transient PG failure and does NOT apply the event to the in-memory store.
///
/// Satisfies AC3.2
#[tokio::test]
#[serial_test::serial]
async fn transient_metric_increments_on_db_outage() {
    let (pool, _c, _) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    // Read the baseline counter value before this test runs.
    let before = render_metrics().await;
    let baseline_transient = parse_counter_value(&before, "transient");

    // Close the pool BEFORE firing any webhook (simulate outage from the start).
    // This causes pool.begin() to fail immediately → 503 with transient counter.
    pool.close().await;

    // Fire the webhook with the pool closed.
    let (status, _json) = post_webhook_full(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    // Phase 2c: transient PG failure → 503 (not 200)
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "DB outage must cause 503 in Phase 2c transactional mode"
    );

    // In-memory store must NOT have the run (transaction never committed)
    let snap = state.store.snapshot().await;
    assert!(
        snap.runs.is_empty(),
        "in-memory store must be empty: transaction never committed"
    );

    // Transient counter must have incremented by exactly 1
    let after = render_metrics().await;
    let after_transient = parse_counter_value(&after, "transient");
    assert_eq!(
        after_transient,
        baseline_transient + 1,
        "transient counter should increment by 1; metrics output:\n{after}"
    );
}

/// Verify that PG write failure counters are visible in /metrics output.
/// Triggers a parity failure to confirm the counter appears in the text output.
#[tokio::test]
#[serial_test::serial]
async fn pg_write_failure_counters_are_registered() {
    atc_server::metrics::register_pg_write_counters();

    let (pool, _c, _) = common::start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Establish a Queued run in both stores.
    let body = common::fixture_workflow_run_requested();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "write should succeed");

    // Manually advance PG to Completed — simulates divergence from in-memory.
    sqlx::query!("UPDATE runs SET status = 'Completed' WHERE status = 'Queued'")
        .execute(&pool)
        .await
        .unwrap();

    // Send InProgress: in-memory accepts (Queued→InProgress); PG rejects (Completed not a
    // predecessor of InProgress) → parity counter increments → counter appears in /metrics.
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;

    // The counter must appear in /metrics now that it has been incremented.
    let metrics_body = render_metrics().await;
    assert!(
        metrics_body.contains("atc_pg_write_failures_total"),
        "counter must appear in /metrics output after parity failure; got:\n{metrics_body}"
    );
}

// ---------------------------------------------------------------------------
// In-memory mode (pg_pool: None): no DB access, processes successfully
// ---------------------------------------------------------------------------

/// Phase 2c: pg_pool: None → webhooks still processed, no panic, in-memory reflects events.
#[tokio::test]
#[serial_test::serial]
async fn in_memory_mode_behavioral_invariance() {
    // Build app inline — avoid common::build_app_no_secret which uses a different OnceLock.
    let layer = prometheus_layer();
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
    });
    let app = atc_server::routes::api_routes(layer)
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());

    let body = common::fixture_workflow_run_requested();
    let req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "workflow_run")
        .body(Body::from(body))
        .unwrap();
    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // In-memory snapshot should reflect the event
    let snap = app_state.store.snapshot().await;
    assert_eq!(snap.runs.len(), 1, "run should be in-memory");
}
