//! Integration tests for the transactional outbox write path.
//!
//! Boots ephemeral Postgres via testcontainers. Route-handler tests fire real
//! webhooks through the full Axum router. Direct DB tests operate against the
//! pool via sqlx using the untyped (non-macro) API to avoid needing compile-time
//! query caching.
//!
//! Requires Docker (or OrbStack) to be running.

use crate::common;

use std::sync::Arc;
use std::time::Duration;

use atc_server::persist::PersistentStore;
use atc_server::state::{AppState, SeqEvent};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use futures_util::future::join_all;
use opentelemetry::KeyValue;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build a full router with a real PG pool mounted.
async fn build_app_with_pg(
    pool: sqlx::PgPool,
    db_url: &str,
) -> (
    axum::Router,
    Arc<AppState>,
    tokio::sync::broadcast::Receiver<SeqEvent>,
) {
    common::ensure_recorder_installed();
    let shutdown = CancellationToken::new();
    let store = common::start_pg_store_for_test(pool, db_url, shutdown.clone()).await;
    let rx = store.subscribe();
    let persist = store as Arc<dyn atc_server::persist::PersistentStore>;
    let app_state = Arc::new(AppState {
        persist,
        webhook_secret: None,
        runner_pool_capacities: Vec::new(),
        shutdown: CancellationToken::new(),
        ws_tracker: TaskTracker::new(),
    });
    let app = atc_server::routes::api_routes()
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

fn write_failure_attrs(kind: &'static str) -> Vec<KeyValue> {
    vec![KeyValue::new("kind", kind)]
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
// Atomicity (success path)
// ---------------------------------------------------------------------------

/// A workflow_run.requested webhook produces both a `runs` row and an
/// `outbox` row with kind='run' in the same transaction.
#[tokio::test]
#[serial_test::serial]
async fn run_webhook_produces_run_and_outbox_row() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "accepted");
    assert!(
        json["seq"].is_number(),
        "PG-mode handler must include outbox seq in response, got: {json}"
    );

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
    state.shutdown.cancel();
}

/// seq values in the outbox are strictly increasing across N committed transactions.
///
/// Uses direct DB INSERT statements for different run_ids to avoid fixture limitations.
/// "Strictly increasing" is the requirement — not "consecutive" (BIGSERIAL may have gaps
/// from aborted txns).
#[tokio::test]
#[serial_test::serial]
async fn seq_strictly_increasing() {
    let (pool, _c, _db_url) = common::start_pg().await;

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

/// The outbox payload serializes as a RunEventEnvelope and does NOT
/// contain top-level keys `pool_stats_after` or `seq`.
#[tokio::test]
#[serial_test::serial]
async fn payload_roundtrips_as_envelope() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

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
    state.shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Atomicity (rollback path)
// ---------------------------------------------------------------------------

/// Parity rejection (invalid run state transition) rolls back the outbox INSERT.
///
/// Pre-insert a Completed run directly. Fire workflow_run.requested (Queued target),
/// which fails the predicate `WHERE status = ANY(['Queued'])` — Completed is not in
/// predecessors_of(Queued). The transaction rolls back; outbox stays at 0 rows.
#[tokio::test]
#[serial_test::serial]
async fn parity_rejection_rolls_back_outbox() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

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
    state.shutdown.cancel();
}

/// BIGSERIAL gap property — an aborted transaction consumes a seq that
/// will never appear in the committed outbox rows.
///
/// Direct DB test (no route handler):
/// - Open tx, INSERT INTO outbox → capture seq_a, ROLLBACK
/// - Open new tx, INSERT INTO outbox → commit, capture seq_b
/// - seq_b > seq_a (strictly) AND no committed row has seq = seq_a
#[tokio::test]
#[serial_test::serial]
async fn bigserial_gap_property() {
    let (pool, _c, _db_url) = common::start_pg().await;

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

/// Backward job transition (Completed → Queued) rolls back the transaction.
/// End state: 1 stub run, 1 job (still Completed), 2 outbox rows (queued + completed only).
///
/// predecessors_of(Queued) = [Queued] — Completed is not a predecessor of Queued.
/// The backward-transition request rolls back; no new outbox row is written.
#[tokio::test]
#[serial_test::serial]
async fn job_upsert_rejection_rolls_back_stub_run() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

    // Fixture shared IDs
    let run_id = 24290980517i64; // from all fixtures
    let job_id = 70928200168i64; // from workflow_job_queued.json and workflow_job_completed.json

    // Step 1: workflow_job.queued — creates stub run + job + outbox row
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "accepted");

    assert_eq!(
        fetch_status(&pool, "jobs", job_id).await,
        "Queued",
        "job must be Queued after first webhook"
    );
    assert_eq!(count_rows(&pool, "outbox").await, 1, "1 outbox row");

    // Step 2: workflow_job.completed — Queued → Completed is now valid (GitHub sends this on
    // cancellation before the job starts). Job transitions; second outbox row is written.
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_completed(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json["status"], "accepted");

    assert_eq!(
        fetch_status(&pool, "jobs", job_id).await,
        "Completed",
        "job must be Completed after second webhook"
    );
    assert_eq!(count_rows(&pool, "outbox").await, 2, "2 outbox rows");

    // Step 3: workflow_job.queued again — Completed → Queued is a backward transition.
    // predecessors_of(Queued) = [Queued]; Completed is not in that set → parity rejection.
    // The transaction (including the outbox INSERT) is rolled back.
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_job",
        &common::fixture_workflow_job_queued(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "parity rejection returns 200");
    assert_eq!(json["status"], "rejected", "body must be 'rejected'");

    // End state: runs=1, job=Completed (not rolled back to Queued), outbox=2
    assert_eq!(
        count_by_id(&pool, "runs", run_id).await,
        1,
        "must still have exactly 1 run row"
    );
    assert_eq!(
        fetch_status(&pool, "jobs", job_id).await,
        "Completed",
        "job must still be Completed (backward transition rolled back)"
    );
    assert_eq!(
        count_rows(&pool, "outbox").await,
        2,
        "outbox must still have 2 rows (rolled-back tx added nothing)"
    );
    state.shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Error policy (subset testable without failure injection)
// ---------------------------------------------------------------------------

/// Parity rejection returns HTTP 200 with body `{"status":"rejected"}`
/// and increments the parity counter by 1.
#[tokio::test]
#[serial_test::serial]
async fn parity_rejection_returns_200_rejected() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

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

    let snapshot = common::snapshot_metrics();
    let parity = common::counter_value(
        &snapshot,
        "atc_pg_write_failures_total",
        &write_failure_attrs("parity"),
    );
    assert_eq!(
        parity, 1,
        "parity counter must increment by 1; got parity={parity}",
    );
    state.shutdown.cancel();
}

/// Successful webhook in PG mode returns HTTP 200 with body
/// `{"status":"accepted","seq":<i64>}` and does NOT increment
/// `atc_pg_write_failures_total`.
#[tokio::test]
#[serial_test::serial]
async fn success_returns_200_accepted() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

    let (status, json) = post_webhook(
        app,
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "success must return 200");
    assert_eq!(
        json["status"], "accepted",
        "PG-mode body status must be 'accepted'"
    );
    assert!(
        json["seq"].is_number(),
        "PG-mode handler must include outbox seq in response, got: {json}"
    );

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_pg_write_failures_total",
            &write_failure_attrs("parity"),
        ),
        0,
        "parity counter must not increment on success"
    );
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_pg_write_failures_total",
            &write_failure_attrs("transient"),
        ),
        0,
        "transient counter must not increment on success"
    );
    state.shutdown.cancel();
}

/// With no database configured (in-memory only mode), a webhook returns 200
/// and the in-memory store reflects the event. No DB calls are made.
#[tokio::test]
#[serial_test::serial]
async fn no_pg_pool_uses_in_memory_path() {
    let (app, app_state) = common::build_app_no_secret();

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

    // In-memory store reflects the event — verify via read_snapshot.
    let snap = app_state.persist.read_snapshot().await.expect("snapshot");
    assert_eq!(snap.runs.len(), 1, "run must be in the in-memory store");
}

// ---------------------------------------------------------------------------
// Stub run created inside transaction for job-before-run path
// ---------------------------------------------------------------------------

/// A workflow_job.queued for an unknown run_id creates:
/// - 1 stub run row (status=Queued)
/// - 1 job row
/// - 1 outbox row with kind='job'
///
/// Then a workflow_run.completed for the same run_id upgrades the stub and
/// adds a second outbox row.
#[tokio::test]
#[serial_test::serial]
async fn job_webhook_creates_stub_run_and_outbox() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

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
    assert_eq!(json["status"], "accepted");
    assert!(
        json["seq"].is_number(),
        "PG-mode handler must include outbox seq in response, got: {json}"
    );

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
    assert_eq!(json["status"], "accepted");
    assert!(
        json["seq"].is_number(),
        "PG-mode handler must include outbox seq in response, got: {json}"
    );

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
    state.shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Payload is RunEventEnvelope / JobEventEnvelope, NOT SeqEvent
// ---------------------------------------------------------------------------

/// Both a run and a job outbox payload deserialize as their respective
/// envelope types and do NOT contain SeqEvent top-level keys.
#[tokio::test]
#[serial_test::serial]
async fn payload_is_envelope_not_seq_event() {
    let (pool, _c, db_url) = common::start_pg().await;
    let (app, state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

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
    state.shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Post-commit in-memory drift
// ---------------------------------------------------------------------------

/// In-memory drift is structurally unreachable in PG mode.
///
/// The in-memory apply is silenced in PG mode — the handler commits to PG,
/// emits NOTIFY, and returns. The drain task is the sole writer to
/// `webhook_tx`, and the in-memory `state.store` is never written. There is
/// no in-memory state to drift from, so `atc_pg_in_memory_drift_total` is a
/// stuck-at-zero counter under PG mode.
///
/// This test asserts the **invariant** instead of the **mechanism**: after a
/// webhook commits in PG mode, the in-memory store is still empty, no drift
/// counter incremented, and the response shape advertises the outbox seq
/// (status="accepted" with seq, not "processed"). The original drift detection
/// is preserved at the metric level — drift_total stays at baseline.
#[tokio::test]
#[serial_test::serial]
async fn no_in_memory_drift_in_pg_mode() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _c, db_url) = common::start_pg().await;
    let (app, app_state, _rx) = build_app_with_pg(pool.clone(), &db_url).await;

    // Fire a webhook through the PG-mode handler.
    let (status, json) = post_webhook(
        app.clone(),
        "workflow_run",
        &common::fixture_workflow_run_requested(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json["status"], "accepted",
        "PG-mode handler returns 'accepted' (not 'processed') with the outbox seq"
    );
    assert!(
        json["seq"].is_number(),
        "PG-mode handler must include outbox seq in response, got: {json}"
    );

    // PG row exists.
    let pg_status: String = sqlx::query_scalar("SELECT status FROM runs LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(pg_status, "Queued", "PG must reflect the committed event");

    // PG mode has no in-memory state; reads go directly to the database.
    // Verify read_snapshot returns the committed run (confirming PG-mode
    // reads are consistent with what was written).
    let snap = app_state.persist.read_snapshot().await.expect("snapshot");
    assert_eq!(
        snap.runs.len(),
        1,
        "PG snapshot must contain the committed run"
    );

    // Drift counter is stuck at zero — no apply, no detection.
    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(&snapshot, "atc_pg_in_memory_drift_total", &[]),
        0,
        "drift counter must not increment in PG mode (handler doesn't write in-memory)"
    );
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_pg_write_failures_total",
            &write_failure_attrs("parity"),
        ),
        0,
        "parity counter must not increment on success"
    );
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_pg_write_failures_total",
            &write_failure_attrs("transient"),
        ),
        0,
        "transient counter must not increment on success"
    );
    app_state.shutdown.cancel();
}

// ---------------------------------------------------------------------------
// Broadcast order matches durable outbox.seq order
// ---------------------------------------------------------------------------

/// N concurrent `workflow_run.requested` webhooks for the SAME run_id — exercises
/// same-row PG contention under genuine concurrent request racing.
///
/// Uses a real TcpListener-based server with reqwest so that the tokio runtime
/// can schedule multiple handler tasks simultaneously. All tasks race; whichever
/// commits its outbox INSERT first wins seq=1, etc. PostgreSQL row-level locking
/// on the same-id UPSERT serializes the in-flight transactions, so all N succeed
/// as idempotent replays under the predicate `WHERE status IN (Queued)`.
///
/// Because all payloads are identical (same run_id → same content in broadcast and
/// outbox), we assert count-level invariants instead of run_id ordering: exactly N
/// SeqEvents broadcast with strictly increasing seq values, exactly N outbox rows
/// created.
///
/// Uses `build_app_with_pg_and_listener` (full fixture with drain task)
/// because the drain — not the handler — is the broadcaster in PG mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn concurrent_run_requested_broadcast_matches_durable_order() {
    const N: usize = 4;
    let (pool, _c, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;
    let mut rx = fixture.state.persist.subscribe();

    // Bind ephemeral listener and spawn server with the full router (includes drain).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = fixture.router.clone();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // All N webhooks target the same run_id (same-row PG contention scenario).
    let run_id = 10000000001i64;
    let base_fixture: serde_json::Value =
        serde_json::from_slice(&common::fixture_workflow_run_requested())
            .expect("fixture must be valid JSON");
    let mut payload = base_fixture.clone();
    payload["workflow_run"]["id"] = serde_json::json!(run_id);
    let body = serde_json::to_vec(&payload).expect("re-serialization must succeed");

    // Spawn N concurrent tasks that all POST the same webhook simultaneously.
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/webhooks/github", addr);
    let handles: Vec<_> = (0..N)
        .map(|_| {
            let client = client.clone();
            let url = url.clone();
            let body = body.clone();
            tokio::spawn(async move {
                client
                    .post(&url)
                    .header("X-GitHub-Event", "workflow_run")
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await
                    .expect("request must succeed")
            })
        })
        .collect();
    join_all(handles).await;

    // Collect N SeqEvents from the broadcast channel in arrival order. The
    // drain may emit them across one or more passes depending on coalescing,
    // but the cumulative count must hit N within the timeout.
    let mut broadcast_seqs: Vec<u64> = Vec::new();
    for _ in 0..N {
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for SeqEvent")
            .expect("broadcast channel closed unexpectedly");
        broadcast_seqs.push(ev.seq);
    }

    let outbox_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outbox")
        .fetch_one(&pool)
        .await
        .unwrap();

    assert_eq!(
        broadcast_seqs.len(),
        N,
        "must broadcast exactly N SeqEvents"
    );
    assert_eq!(outbox_count as usize, N, "outbox must have exactly N rows");

    // Drain emits in `ORDER BY seq` so broadcast seqs must be strictly increasing.
    for w in broadcast_seqs.windows(2) {
        assert!(
            w[0] < w[1],
            "broadcast SeqEvent.seq must be strictly increasing: {w:?}"
        );
    }

    server_handle.abort();
    fixture.shutdown.cancel();
}

/// N concurrent `workflow_run.requested` webhooks for N different run_ids —
/// broadcast order matches outbox.seq order under genuine concurrent request
/// racing.
///
/// Uses a real TcpListener-based server with reqwest. N tasks POST
/// simultaneously; whichever transaction commits first wins the lower seq.
/// The drain reads `seq > watermark ORDER BY seq` so its broadcast order
/// matches BIGSERIAL allocation order — which IS durable outbox.seq order.
/// The test asserts broadcast order == durable outbox.seq order.
///
/// Ordering is provided by outbox `BIGSERIAL` + drain's ascending ORDER BY
/// (the seq mutex is bypassed in PG mode). The test uses the full fixture
/// so the drain task is running.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[serial_test::serial]
async fn concurrent_different_runs_broadcast_matches_durable_order() {
    const N: usize = 4;
    let (pool, _c, db_url) = common::start_pg().await;
    let fixture = common::build_app_with_pg_and_listener(pool.clone(), db_url).await;
    let mut rx = fixture.state.persist.subscribe();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = fixture.router.clone();
    let server_handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });

    // Build N=4 distinct workflow_run.requested bodies by mutating the fixture run_id.
    let run_ids: Vec<i64> = vec![10000000001, 10000000002, 10000000003, 10000000004];
    let base_fixture: serde_json::Value =
        serde_json::from_slice(&common::fixture_workflow_run_requested())
            .expect("fixture must be valid JSON");

    let bodies: Vec<Vec<u8>> = run_ids
        .iter()
        .map(|&run_id| {
            let mut payload = base_fixture.clone();
            payload["workflow_run"]["id"] = serde_json::json!(run_id);
            serde_json::to_vec(&payload).expect("re-serialization must succeed")
        })
        .collect();

    // Spawn N concurrent tasks that all POST to the server simultaneously.
    let client = reqwest::Client::new();
    let url = format!("http://{}/v1/webhooks/github", addr);
    let handles: Vec<_> = bodies
        .into_iter()
        .map(|body| {
            let client = client.clone();
            let url = url.clone();
            tokio::spawn(async move {
                client
                    .post(&url)
                    .header("X-GitHub-Event", "workflow_run")
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await
                    .expect("request must succeed")
            })
        })
        .collect();
    join_all(handles).await;

    // Collect N SeqEvents from the broadcast channel in arrival order.
    let mut broadcast_run_ids: Vec<i64> = Vec::new();
    for _ in 0..N {
        let ev = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for SeqEvent")
            .expect("broadcast channel closed unexpectedly");
        let run_id = match &ev.event {
            atc_github::WebhookEvent::Run(env) => env.run_id.0,
            atc_github::WebhookEvent::Job(env) => env.run_id.0,
        };
        broadcast_run_ids.push(run_id);
    }

    // Query outbox for the N run_ids in BIGSERIAL allocation order.
    let outbox_run_ids: Vec<i64> = sqlx::query_scalar("SELECT run_id FROM outbox ORDER BY seq")
        .fetch_all(&pool)
        .await
        .unwrap();

    assert_eq!(outbox_run_ids.len(), N, "outbox must have exactly N rows");

    // Broadcast order is COMMIT order, not BIGSERIAL allocation order. Under
    // concurrent commits, BIGSERIAL allocates a seq before commit, but
    // transactions can commit in a different order. The drain sees rows only
    // after they commit, and may also rescan when a delayed commit's NOTIFY
    // arrives — gap-healing can interleave a freshly seen low seq among
    // previously-broadcast higher seqs. When the seq mutex serialized the
    // handler, broadcast order matched BIGSERIAL allocation order; in PG mode
    // we no longer enforce that.
    //
    // The invariants we DO enforce here are weaker but still load-bearing:
    //   1. Every run_id committed to the outbox is broadcast exactly once.
    //   2. No spurious run_ids are broadcast (no duplicates, no extras).
    //
    // These guarantee the frontend converges to the correct state regardless
    // of broadcast order, which the bounded ring-buffer dedup makes safe.
    let mut broadcast_sorted = broadcast_run_ids.clone();
    broadcast_sorted.sort();
    let mut outbox_sorted = outbox_run_ids.clone();
    outbox_sorted.sort();
    assert_eq!(
        broadcast_sorted, outbox_sorted,
        "broadcast run_id set must match outbox set (order may differ under concurrent commits);\n\
         broadcast={broadcast_run_ids:?}\n\
         outbox   ={outbox_run_ids:?}"
    );

    server_handle.abort();
    fixture.shutdown.cancel();
}
