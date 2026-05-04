//! Integration tests for the Phase 2c outbox acceptance criteria.
//!
//! Boots ephemeral Postgres via testcontainers. Route-handler tests fire real
//! webhooks through the full Axum router. Direct DB tests operate against the
//! pool via sqlx using the untyped (non-macro) API to avoid needing compile-time
//! query caching.
//!
//! Requires Docker (or OrbStack) to be running.
//!
//! Test naming convention: `phase_2c_outbox_ac<N>_<seq>_<description>`

mod common;

use std::sync::Arc;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::state::{AppState, SeqEvent};
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Prometheus singleton for tests that check /metrics counters
// ---------------------------------------------------------------------------

static PROMETHEUS_INIT: OnceLock<(PrometheusMetricLayer<'static>, axum::Router)> = OnceLock::new();

fn prometheus_init() -> &'static (PrometheusMetricLayer<'static>, axum::Router) {
    PROMETHEUS_INIT.get_or_init(atc_server::metrics::build)
}

fn prometheus_layer() -> PrometheusMetricLayer<'static> {
    prometheus_init().0.clone()
}

/// Parse `atc_pg_write_failures_total{kind="<kind>"}` from Prometheus text output.
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

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

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

/// Build a full router with a real PG pool mounted.
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

/// POST a webhook via tower::ServiceExt::oneshot and return (status, body).
async fn post_webhook(
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

/// GET /metrics from the Prometheus side-port router.
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

/// Insert a minimal stub runs row for FK satisfaction. Uses untyped sqlx API.
async fn insert_stub_run(pool: &sqlx::PgPool, run_id: i64) {
    sqlx::query(
        r#"
        INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at)
        VALUES ($1, 'test-org', 'test-repo', '', '', '', '', 'Queued', now(), now())
        ON CONFLICT (id) DO NOTHING
        "#,
    )
    .bind(run_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Insert a Completed run directly (to trigger parity rejection on subsequent Requested webhook).
async fn insert_completed_run(pool: &sqlx::PgPool, run_id: i64) {
    sqlx::query(
        r#"
        INSERT INTO runs (id, org, repo, head_sha, event, display_title, html_url, status, created_at, updated_at)
        VALUES ($1, 'org', 'repo', 'abc', 'push', 'Test', 'http://x', 'Completed', now(), now())
        "#,
    )
    .bind(run_id)
    .execute(pool)
    .await
    .unwrap();
}

/// Count rows in a table.
async fn count_rows(pool: &sqlx::PgPool, table: &str) -> i64 {
    sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*)::bigint FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Count outbox rows by kind.
async fn count_outbox_by_kind(pool: &sqlx::PgPool, kind: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*)::bigint FROM outbox WHERE kind = $1")
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// Count rows matching an integer id filter.
async fn count_by_id(pool: &sqlx::PgPool, table: &str, id: i64) -> i64 {
    sqlx::query_scalar::<_, i64>(&format!(
        "SELECT COUNT(*)::bigint FROM {table} WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// Fetch status string from a table by id.
async fn fetch_status(pool: &sqlx::PgPool, table: &str, id: i64) -> String {
    sqlx::query_scalar::<_, String>(&format!("SELECT status FROM {table} WHERE id = $1"))
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
// AC1 — Atomicity (success path)
// ---------------------------------------------------------------------------

/// AC1.1: A workflow_run.requested webhook produces both a `runs` row and an
/// `outbox` row with kind='run' in the same transaction.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac1_1_run_webhook_produces_run_and_outbox_row() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "processed");

    // One runs row
    assert_eq!(
        count_rows(&pool, "runs").await,
        1,
        "must have exactly 1 runs row"
    );

    // One outbox row with kind='run'
    assert_eq!(
        count_outbox_by_kind(&pool, "run").await,
        1,
        "must have exactly 1 outbox row with kind='run'"
    );

    // The outbox row must reference the correct run_id (24290980517 from fixture)
    let outbox_run_id: i64 =
        sqlx::query_scalar("SELECT run_id FROM outbox WHERE kind = 'run' LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        outbox_run_id, 24290980517i64,
        "outbox run_id must match fixture"
    );
}

/// AC1.2: seq values in the outbox are strictly increasing across N committed transactions.
///
/// Uses direct DB INSERT statements for different run_ids to avoid fixture limitations.
/// "Strictly increasing" is the requirement — not "consecutive" (BIGSERIAL may have gaps
/// from aborted txns; see AC2.2).
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac1_2_seq_strictly_increasing() {
    let (pool, _c) = start_pg().await;

    // Insert stub runs for FK satisfaction
    let run_ids = [20001i64, 20002i64, 20003i64];
    for &run_id in &run_ids {
        insert_stub_run(&pool, run_id).await;
    }

    // Insert 3 outbox rows in separate committed transactions (default autocommit via sqlx)
    for &run_id in &run_ids {
        sqlx::query("INSERT INTO outbox (kind, run_id, payload) VALUES ('run', $1, '{}'::jsonb)")
            .bind(run_id)
            .execute(&pool)
            .await
            .unwrap();
    }

    // SELECT seq values in committed order
    let seqs: Vec<i64> = sqlx::query_scalar::<_, i64>("SELECT seq FROM outbox ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(seqs.len(), 3, "should have 3 outbox rows");

    // Strictly increasing: each seq must be greater than the previous
    for window in seqs.windows(2) {
        assert!(
            window[1] > window[0],
            "seq values must be strictly increasing: {} > {} failed",
            window[1],
            window[0]
        );
    }
}

/// AC1.3: The outbox payload serializes as a RunEventEnvelope and does NOT
/// contain top-level keys `pool_stats_after` or `seq`.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac1_3_payload_roundtrips_as_envelope() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    // Fetch the outbox payload as a raw serde_json::Value
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM outbox WHERE kind = 'run' LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();

    // Must deserialize successfully as RunEventEnvelope
    let envelope: atc_core::event::RunEventEnvelope = serde_json::from_value(payload.clone())
        .expect("payload must deserialize as RunEventEnvelope");
    assert_eq!(
        envelope.run_id.0, 24290980517i64,
        "deserialized run_id must match fixture"
    );

    // Must NOT contain SeqEvent-specific top-level keys
    assert!(
        payload.get("pool_stats_after").is_none(),
        "payload must not contain pool_stats_after (would indicate SeqEvent serialization)"
    );
    assert!(
        payload.get("seq").is_none(),
        "payload must not contain seq (would indicate SeqEvent serialization)"
    );
}

// ---------------------------------------------------------------------------
// AC2 — Atomicity (rollback path)
// ---------------------------------------------------------------------------

/// AC2.1: Parity rejection (invalid run state transition) rolls back the outbox INSERT.
///
/// Pre-insert a Completed run directly. Fire workflow_run.requested (Queued target),
/// which fails the predicate `WHERE status = ANY(['Queued'])` — Completed is not in
/// predecessors_of(Queued). The transaction rolls back; outbox stays at 0 rows.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac2_1_parity_rejection_rolls_back_outbox() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // The fixture run_id is 24290980517
    let run_id = 24290980517i64;
    insert_completed_run(&pool, run_id).await;

    // Fire the Requested webhook — predicate for Queued is [Queued], but row is Completed
    // → InvalidTransition → parity rejection → transaction rolled back
    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "parity rejection returns 200");
    assert_eq!(json["status"], "rejected", "body must be 'rejected'");

    // Outbox must still have 0 rows — the INSERT was rolled back with the transaction
    assert_eq!(
        count_rows(&pool, "outbox").await,
        0,
        "outbox must have 0 rows: transaction was rolled back"
    );
}

/// AC2.2: BIGSERIAL gap property — an aborted transaction consumes a seq that
/// will never appear in the committed outbox rows.
///
/// Direct DB test (no route handler):
/// - Open tx, INSERT INTO outbox → capture seq_a, ROLLBACK
/// - Open new tx, INSERT INTO outbox → commit, capture seq_b
/// - seq_b > seq_a (strictly) AND no committed row has seq = seq_a
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac2_2_bigserial_gap_property() {
    let (pool, _c) = start_pg().await;

    // Insert a stub run for FK constraint satisfaction
    insert_stub_run(&pool, 99001).await;

    // Transaction A: INSERT outbox row, capture seq, ROLLBACK
    let seq_a: i64 = {
        let mut tx = pool.begin().await.unwrap();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 99001, '{}'::jsonb) RETURNING seq",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        seq
    };

    // Transaction B: INSERT outbox row, commit, capture seq
    let seq_b: i64 = {
        let mut tx = pool.begin().await.unwrap();
        let seq: i64 = sqlx::query_scalar(
            "INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 99001, '{}'::jsonb) RETURNING seq",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        seq
    };

    // seq_b must be strictly greater than seq_a (BIGSERIAL advanced past the aborted tx)
    assert!(
        seq_b > seq_a,
        "committed seq ({seq_b}) must be strictly greater than the aborted seq ({seq_a})"
    );

    // No committed outbox row has seq = seq_a (it was rolled back)
    let gap_count: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM outbox WHERE seq = $1")
        .bind(seq_a)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        gap_count, 0,
        "no committed row should have the aborted seq={seq_a}"
    );
}

/// AC2.3: Second job webhook with invalid transition (Queued → Completed) rolls back
/// the transaction. End state: 1 stub run, 1 job (still Queued), 1 outbox row
/// (from first successful webhook only).
///
/// predecessors_of(Completed) for jobs = [InProgress, Completed].
/// Queued → Completed is therefore invalid (Queued is not a predecessor of Completed for jobs).
/// The transaction rolls back; no new outbox row is written.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac2_3_job_upsert_rejection_rolls_back_stub_run() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Fixture shared IDs
    let run_id = 24290980517i64; // from all fixtures
    let job_id = 70928200168i64; // from workflow_job_queued.json and workflow_job_completed.json

    // First webhook: workflow_job.queued — creates stub run + job + outbox row
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "processed");

    // Assert initial state: 1 stub run, 1 job (Queued), 1 outbox row
    assert_eq!(
        count_by_id(&pool, "runs", run_id).await,
        1,
        "should have 1 stub run"
    );
    assert_eq!(
        fetch_status(&pool, "jobs", job_id).await,
        "Queued",
        "job must be Queued"
    );
    assert_eq!(
        count_rows(&pool, "outbox").await,
        1,
        "should have 1 outbox row after first webhook"
    );

    // Second webhook: workflow_job.completed for the SAME job_id.
    // job is currently Queued; predecessors_of(Completed) = [InProgress, Completed].
    // Queued is NOT in predecessors_of(Completed) → InvalidTransition → parity rejection.
    // The transaction (including the outbox INSERT) is rolled back.
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_completed(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "parity rejection returns 200");
    assert_eq!(json["status"], "rejected", "body must be 'rejected'");

    // End state assertions:
    // runs: still exactly 1 row
    assert_eq!(
        count_by_id(&pool, "runs", run_id).await,
        1,
        "must still have exactly 1 run row"
    );

    // jobs: still exactly 1 row, still Queued (update was rolled back)
    assert_eq!(
        fetch_status(&pool, "jobs", job_id).await,
        "Queued",
        "job must still be Queued"
    );

    // outbox: still exactly 1 row (the rolled-back tx added nothing)
    assert_eq!(
        count_rows(&pool, "outbox").await,
        1,
        "outbox must still have 1 row (rolled-back tx added nothing)"
    );
}

// ---------------------------------------------------------------------------
// AC3 — Error policy (subset testable without failure injection)
// ---------------------------------------------------------------------------

/// AC3.1: Parity rejection returns HTTP 200 with body `{"status":"rejected"}`
/// and increments the parity counter by 1.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac3_1_parity_rejection_returns_200_rejected() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    let before = render_metrics().await;
    let baseline_parity = parse_counter_value(&before, "parity");

    // Pre-insert Completed run to force parity rejection on Requested
    let run_id = 24290980517i64;
    insert_completed_run(&pool, run_id).await;

    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "parity rejection must return 200");
    assert_eq!(json["status"], "rejected", "body status must be 'rejected'");

    let after = render_metrics().await;
    let after_parity = parse_counter_value(&after, "parity");
    assert_eq!(
        after_parity,
        baseline_parity + 1,
        "parity counter must increment by 1; metrics:\n{after}"
    );
}

/// AC3.4: Successful webhook returns HTTP 200 with body `{"status":"processed"}`
/// and does NOT increment `atc_pg_write_failures_total`.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac3_4_success_returns_200_processed() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    let before = render_metrics().await;
    let baseline_parity = parse_counter_value(&before, "parity");
    let baseline_transient = parse_counter_value(&before, "transient");

    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "success must return 200");
    assert_eq!(
        json["status"], "processed",
        "body status must be 'processed'"
    );

    let after = render_metrics().await;
    assert_eq!(
        parse_counter_value(&after, "parity"),
        baseline_parity,
        "parity counter must not increment on success"
    );
    assert_eq!(
        parse_counter_value(&after, "transient"),
        baseline_transient,
        "transient counter must not increment on success"
    );
}

/// AC3.5: With pg_pool: None (in-memory only mode), a webhook returns 200 processed
/// and the in-memory store reflects the event. No DB calls are made.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac3_5_no_pg_pool_uses_in_memory_path() {
    // Build app inline using the local prometheus_layer() to reuse the OnceLock recorder.
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
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "in-memory path must return 200"
    );

    // In-memory store reflects the event — no outbox to check in this mode
    let snap = app_state.store.snapshot().await;
    assert_eq!(snap.0.runs.len(), 1, "run must be in the in-memory store");
}

// ---------------------------------------------------------------------------
// AC5 — Stub run created inside transaction for job-before-run path
// ---------------------------------------------------------------------------

/// AC5.1: A workflow_job.queued for an unknown run_id creates:
/// - 1 stub run row (status=Queued)
/// - 1 job row
/// - 1 outbox row with kind='job'
///
/// Then a workflow_run.completed for the same run_id upgrades the stub and
/// adds a second outbox row.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac5_1_job_webhook_creates_stub_run_and_outbox() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // The fixtures share run_id=24290980517, job_id=70928200168
    let run_id = 24290980517i64;

    // Step 1: Fire workflow_job.queued with NO prior runs row
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "processed");

    // runs: 1 stub row, status=Queued
    assert_eq!(
        fetch_status(&pool, "runs", run_id).await,
        "Queued",
        "stub run must have status Queued"
    );

    // jobs: 1 row
    assert_eq!(
        count_rows(&pool, "jobs").await,
        1,
        "must have exactly 1 job row"
    );

    // outbox: 1 row with kind='job'
    assert_eq!(
        count_outbox_by_kind(&pool, "job").await,
        1,
        "must have 1 outbox row with kind='job'"
    );

    // Step 2: Fire workflow_run.completed for the same run_id.
    // The run is currently at Queued (stub), and Completed is reachable from Queued
    // (predecessors_of(Completed) for runs = [Queued, InProgress, Completed]).
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_completed(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "processed");

    // runs.status should now be Completed (stub upgraded)
    assert_eq!(
        fetch_status(&pool, "runs", run_id).await,
        "Completed",
        "stub must be upgraded to Completed"
    );

    // outbox: 2 rows total (job + run)
    assert_eq!(
        count_rows(&pool, "outbox").await,
        2,
        "must have 2 outbox rows total"
    );
}

// ---------------------------------------------------------------------------
// AC6 — Payload is RunEventEnvelope / JobEventEnvelope, NOT SeqEvent
// ---------------------------------------------------------------------------

/// AC6.1: Both a run and a job outbox payload deserialize as their respective
/// envelope types and do NOT contain SeqEvent top-level keys.
#[tokio::test]
#[serial_test::serial]
async fn phase_2c_outbox_ac6_1_payload_is_envelope_not_seq_event() {
    let (pool, _c) = start_pg().await;
    let (app, _state, _rx) = build_app_with_pg(pool.clone());

    // Fire a run webhook first (to have a real run row for the job FK)
    post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    // Fire a job webhook
    post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;

    // Fetch both outbox rows: kind and payload
    #[derive(sqlx::FromRow)]
    struct OutboxRow {
        kind: String,
        payload: serde_json::Value,
    }

    let rows: Vec<OutboxRow> = sqlx::query_as("SELECT kind, payload FROM outbox ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(rows.len(), 2, "must have 2 outbox rows");

    for row in &rows {
        let payload = &row.payload;

        // No SeqEvent top-level keys in any payload
        assert!(
            payload.get("seq").is_none(),
            "kind='{}' payload must not contain 'seq' key (would indicate SeqEvent)",
            row.kind
        );
        assert!(
            payload.get("pool_stats_after").is_none(),
            "kind='{}' payload must not contain 'pool_stats_after' key (would indicate SeqEvent)",
            row.kind
        );

        match row.kind.as_str() {
            "run" => {
                // Must deserialize as RunEventEnvelope
                let _env: atc_core::event::RunEventEnvelope =
                    serde_json::from_value(payload.clone())
                        .expect("run outbox payload must deserialize as RunEventEnvelope");
            }
            "job" => {
                // Must deserialize as JobEventEnvelope
                let _env: atc_core::event::JobEventEnvelope =
                    serde_json::from_value(payload.clone())
                        .expect("job outbox payload must deserialize as JobEventEnvelope");
            }
            other => panic!("unexpected outbox kind: {other}"),
        }
    }
}
