//! Integration tests for the webhook-handler dual-write path.
//!
//! Boots ephemeral Postgres via testcontainers, mounts the full router with
//! `pg_store: Some(Arc::new(PgStore::new(pool.clone())))`, fires webhook payloads,
//! and asserts in-memory + PG agreement. Covers AC5, AC6, AC7.
//!
//! Requires Docker (or OrbStack) to be running.

mod common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::persist::PgStore;
use atc_server::state::{AppState, SeqEvent};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() installed once per binary.
// All tests in this file must be #[serial_test::serial].
// We store (layer, metrics_router) so AC7 tests can GET /metrics and assert counter values.
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
        if line.starts_with("atc_pg_write_failures_total") && line.contains(&needle) {
            // Line format: `metric_name{labels} value`
            if let Some(value_str) = line.split_whitespace().last() {
                return value_str.parse::<u64>().unwrap_or(0);
            }
        }
    }
    0
}

/// Boot a fresh PG container and return a pool with migrations applied.
async fn start_pg() -> (sqlx::PgPool, impl Drop) {
    let container = Postgres::default()
        .with_tag("17-alpine")
        .start()
        .await
        .expect("failed to start postgres container");
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("failed to get port");
    let db_url = format!("postgres://postgres:postgres@127.0.0.1:{port}/postgres");
    let pool = atc_server::db::init_pool(&db_url)
        .await
        .expect("init_pool failed");
    (pool, container)
}

/// Build a full router with a real PG store mounted.
fn build_app_with_pg(
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
    let pg_store: Arc<dyn atc_core::PersistentStore + Send + Sync> =
        Arc::new(PgStore::new(pool.clone()));
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: Some(pool),
        pg_store: Some(pg_store),
    });
    let app = atc_server::routes::api_routes(layer)
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state, rx)
}

/// POST a webhook and return the HTTP response status.
async fn post_webhook(app: axum::Router, event_type: &str, body: &[u8]) -> StatusCode {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/webhooks/github")
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", event_type)
        .body(Body::from(body.to_vec()))
        .unwrap();
    app.oneshot(req).await.unwrap().status()
}

// ---------------------------------------------------------------------------
// AC5 — Dual-write composed with seq mutex
// ---------------------------------------------------------------------------

/// AC5: Full run lifecycle: Queued → InProgress → Completed.
/// At each step, assert in-memory and PG agree on the run status.
#[tokio::test]
#[serial_test::serial]
async fn dual_write_run_lifecycle() {
    let (pool, _c) = start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    // Requested
    let body = common::fixture_workflow_run_requested();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "Requested should return 200");

    // Give the handler time to complete PG write (single-threaded oneshot)
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = state.store.snapshot().await;
    let mem_run = &snap.0.runs[0];
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

    // In-progress
    let body = common::fixture_workflow_run_in_progress();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "InProgress should return 200");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = state.store.snapshot().await;
    let mem_run = &snap.0.runs[0];
    assert_eq!(format!("{:?}", mem_run.status), "InProgress");
    let pg_row = sqlx::query!("SELECT status FROM runs WHERE id = $1", mem_run.id.0)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_row.status, "InProgress");

    // Completed
    let body = common::fixture_workflow_run_completed();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "Completed should return 200");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let snap = state.store.snapshot().await;
    let mem_run = &snap.0.runs[0];
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
}

/// AC5: Job lifecycle: Queued → InProgress (run pre-exists).
/// Note: the queued and in_progress job fixtures have different job IDs
/// (70928200168 vs 70928200174). We verify each at its own ID.
#[tokio::test]
#[serial_test::serial]
async fn dual_write_job_lifecycle() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Pre-insert the run first
    let run_body = common::fixture_workflow_run_requested();
    post_webhook(app.clone(), "workflow_run", &run_body).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Queued job (job_id=70928200168)
    let job_body = common::fixture_workflow_job_queued();
    let status = post_webhook(app.clone(), "workflow_job", &job_body).await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Assert the queued job row by its specific ID (from workflow_job_queued.json)
    let queued_row = sqlx::query!("SELECT status FROM jobs WHERE id = 70928200168")
        .fetch_one(&pool)
        .await
        .expect("queued job row not found in PG");
    assert_eq!(queued_row.status, "Queued");

    // InProgress job (different job_id=70928200174 — a distinct job in the same run)
    let job_body = common::fixture_workflow_job_in_progress();
    let status = post_webhook(app.clone(), "workflow_job", &job_body).await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Assert the in-progress job row by its specific ID (from workflow_job_in_progress.json)
    let in_progress_row = sqlx::query!("SELECT status FROM jobs WHERE id = 70928200174")
        .fetch_one(&pool)
        .await
        .expect("in-progress job row not found in PG");
    assert_eq!(in_progress_row.status, "InProgress");
}

/// AC5: Job-before-run — fire job first, then run; assert stub then reconciliation.
#[tokio::test]
#[serial_test::serial]
async fn dual_write_job_before_run_lifecycle() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Fire job first (before run)
    let job_body = common::fixture_workflow_job_queued();
    let status = post_webhook(app.clone(), "workflow_job", &job_body).await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Parse the run_id from PG (the stub run row)
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

    // Now fire the real run event
    let run_body = common::fixture_workflow_run_requested();
    let status = post_webhook(app.clone(), "workflow_run", &run_body).await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Run row should now have real data (head_sha, workflow_name etc.)
    let run_row = sqlx::query!("SELECT head_sha, workflow_name FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    // head_sha is set by the fixture (non-empty)
    assert_ne!(run_row.head_sha, "", "head_sha should be reconciled");
}

/// AC5: In-memory rejected transition → PG is never called → PG row unchanged.
#[tokio::test]
#[serial_test::serial]
async fn dual_write_invalid_transition_skips_pg() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Valid run to Completed
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_completed(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Attempt Completed → InProgress (in-memory rejects it)
    let status = post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;
    // Returns 200 even on rejection (shadow mode behavior)
    assert_eq!(status, StatusCode::OK, "always 200 in shadow mode");

    // PG row should still be Completed (PG write was skipped)
    let pg_row = sqlx::query!("SELECT status FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_row.status, "Completed", "PG should still be Completed");
}

/// AC5: Idempotent replay — same webhook twice → both stores stable.
#[tokio::test]
#[serial_test::serial]
async fn dual_write_idempotent_replay() {
    let (pool, _c) = start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    let body = common::fixture_workflow_run_requested();

    let s1 = post_webhook(app.clone(), "workflow_run", &body).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let s2 = post_webhook(app.clone(), "workflow_run", &body).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert_eq!(s1, StatusCode::OK);
    assert_eq!(s2, StatusCode::OK);

    let snap = state.store.snapshot().await;
    assert_eq!(snap.0.runs.len(), 1, "exactly one run in memory");

    let pg_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_count, 1, "exactly one run in PG");
}

// ---------------------------------------------------------------------------
// AC6 — In-memory mode behavioral invariance
// ---------------------------------------------------------------------------

/// AC6: pg_store: None → webhooks still processed, no panic, in-memory reflects events.
#[tokio::test]
#[serial_test::serial]
async fn in_memory_mode_behavioral_invariance() {
    // Build app inline (avoid common::build_app_no_secret which uses a different Prometheus OnceLock)
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
        pg_store: None, // No PG — type-level proof that no DB calls occur
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
    assert_eq!(snap.0.runs.len(), 1, "run should be in-memory");
    assert!(
        app_state.pg_store.is_none(),
        "pg_store is None — no DB attempt"
    );
}

// ---------------------------------------------------------------------------
// AC7 — Drift observability via metrics
// ---------------------------------------------------------------------------

/// AC7: When PG has a row at Completed but in-memory accepts Queued→InProgress,
/// PG's WHERE predicate rejects the write → parity counter increments.
#[tokio::test]
#[serial_test::serial]
async fn parity_metric_increments_when_pg_rejects() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Read the baseline counter value before this test runs (other serial tests may
    // have already incremented it).
    let before = render_metrics().await;
    let baseline_parity = parse_counter_value(&before, "parity");

    // 1. Insert run through normal flow (Queued)
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. Manually advance PG row to Completed (simulates divergence)
    sqlx::query!("UPDATE runs SET status = 'Completed' WHERE status = 'Queued'")
        .execute(&pool)
        .await
        .unwrap();

    // 3. Send InProgress (in-memory: Queued→InProgress valid; PG: Completed not in
    //    predecessors_of(InProgress) → 0 rows affected → InvalidTransition → parity metric)
    let status = post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "shadow mode always returns 200");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // PG row should still be Completed (rejected)
    let pg_row = sqlx::query!("SELECT status FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_row.status, "Completed", "PG should still be Completed");

    // Parity counter must have incremented by exactly 1.
    let after = render_metrics().await;
    let after_parity = parse_counter_value(&after, "parity");
    assert_eq!(
        after_parity,
        baseline_parity + 1,
        "parity counter should increment by 1; metrics output:\n{after}"
    );
}

/// AC7: DB outage → transient counter increments; in-memory write still succeeds.
#[tokio::test]
#[serial_test::serial]
async fn transient_metric_increments_on_db_outage() {
    let (pool, _c) = start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone());

    // Read the baseline counter value before this test runs.
    let before = render_metrics().await;
    let baseline_transient = parse_counter_value(&before, "transient");

    // Fire one successful webhook to establish state
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Drop the pool (simulate outage by closing all connections)
    // We can't stop the container easily without the container handle, but we can
    // close the pool to cause sqlx errors on the next write.
    pool.close().await;

    // Fire in-progress webhook; in-memory should accept but PG write will fail
    let status = post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_in_progress(),
    )
    .await;
    // Shadow mode: always 200, transient PG failure doesn't block response
    assert_eq!(
        status,
        StatusCode::OK,
        "DB outage should not cause 5xx in shadow mode"
    );
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // In-memory store should still reflect the InProgress state
    let snap = state.store.snapshot().await;
    assert!(!snap.0.runs.is_empty(), "in-memory run should exist");
    assert_eq!(
        format!("{:?}", snap.0.runs[0].status),
        "InProgress",
        "in-memory should be InProgress despite PG outage"
    );

    // Transient counter must have incremented by exactly 1.
    let after = render_metrics().await;
    let after_transient = parse_counter_value(&after, "transient");
    assert_eq!(
        after_transient,
        baseline_transient + 1,
        "transient counter should increment by 1; metrics output:\n{after}"
    );
}

/// Verify that shadow PG write failure counters are visible in /metrics output.
/// Triggers a parity failure (in-memory accepts, PG rejects) to confirm the counter appears.
#[tokio::test]
#[serial_test::serial]
async fn shadow_pg_write_failure_counters_are_registered() {
    // register_pg_write_counters() requires the recorder to be installed first.
    // The PROMETHEUS_INIT OnceLock ensures pair() is only called once per binary.
    // By the time this test runs, the recorder is already installed.
    atc_server::metrics::register_pg_write_counters();

    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Establish a Queued run in both stores.
    let body = common::fixture_workflow_run_requested();
    let status = post_webhook(app.clone(), "workflow_run", &body).await;
    assert_eq!(status, StatusCode::OK, "shadow write should succeed");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

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
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // The counter must appear in /metrics now that it has been incremented.
    let metrics_body = render_metrics().await;
    assert!(
        metrics_body.contains("atc_pg_write_failures_total"),
        "counter must appear in /metrics output after parity failure; got:\n{metrics_body}"
    );
}
