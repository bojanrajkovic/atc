//! Integration tests for `auth.mode = "github"` filtering of `GET /v1/state`
//! (#459). Separate from `state_tests.rs` (mode=none snapshot behavior) since
//! this is a distinct concern — seeding multi-repo state through
//! `PersistentStore` directly and comparing an auth-enabled response against
//! an unfiltered one over the SAME underlying data.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{StatusCode, header};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::ServiceExt;

use atc_core::test_support::make_run_event;
use atc_core::{Clock, JobEvent, JobId, RepoId, RunEvent, RunEventEnvelope, RunId, SystemClock};
use atc_persist::PersistentStore;
use atc_server::auth::AuthRuntime;
use atc_server::github_client::GitHubClient;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use atc_store_pg::SessionStore;

use crate::common;

const TEST_PUBLIC_ORIGIN: &str = "http://public.example.test";

/// Build two routers sharing the same underlying `PersistentStore`: one
/// `auth.mode = "none"` (today's unfiltered behavior) and one `auth.mode =
/// "github"` (this ticket's filtering). Comparing responses from both lets
/// tests assert the auth-mode filter is the ONLY difference — `lastSeq`,
/// pool capacities, and `displayTtlSeconds` must be identical either way.
/// Returns `(none_app, auth_app, sessions, persist)` so callers can seed data
/// and mint sessions directly.
async fn build_shared_persist_apps(
    pool: atc_store_pg::TracedPool,
) -> (
    axum::Router,
    axum::Router,
    Arc<SessionStore>,
    Arc<dyn PersistentStore>,
) {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shutdown = CancellationToken::new();
    let persist: Arc<dyn PersistentStore> = InMemoryStore::start(
        Arc::clone(&clock),
        Duration::from_secs(60 * 60),
        Duration::from_secs(60),
        None,
        shutdown.clone(),
    );

    let none_state = Arc::new(AppState {
        persist: Arc::clone(&persist),
        clock: Arc::clone(&clock),
        display_ttl: Duration::from_secs(60 * 60),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: broadcast::channel(16).0,
        shutdown: shutdown.clone(),
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
        auth: None,
    });
    let none_app = atc_server::routes::api_routes(false).with_state(none_state);

    let sessions = SessionStore::start(pool, Arc::clone(&clock), shutdown.clone());
    let github = Arc::new(GitHubClient::with_base_urls(
        "unused-client-id".to_string(),
        "unused-client-secret".to_string(),
        "http://unused.invalid".to_string(),
        "http://unused.invalid".to_string(),
    ));
    let auth_runtime = AuthRuntime {
        github,
        sessions: Arc::clone(&sessions),
        public_origin: TEST_PUBLIC_ORIGIN.to_string(),
        max_session_ttl: Duration::from_secs(30 * 24 * 60 * 60),
        repo_auth_ttl: Duration::from_secs(60 * 60),
    };
    let auth_state = Arc::new(AppState {
        persist: Arc::clone(&persist),
        clock,
        display_ttl: Duration::from_secs(60 * 60),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: broadcast::channel(16).0,
        shutdown,
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
        auth: Some(auth_runtime),
    });
    let auth_app = atc_server::routes::api_routes(true).with_state(auth_state);

    (none_app, auth_app, sessions, persist)
}

fn run_event(run_id: i64, org: &str, repo: &str, repo_id: Option<RepoId>) -> RunEventEnvelope {
    RunEventEnvelope {
        org: org.to_string(),
        repo: repo.to_string(),
        repo_id,
        ..make_run_event(RunId(run_id), RunEvent::Requested)
    }
}

async fn get_state_response(
    app: &axum::Router,
    session_cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("GET")
        .uri("/v1/state");
    if let Some(cookie) = session_cookie {
        builder = builder.header(header::COOKIE, format!("atc_session={cookie}"));
    }
    let req = builder.body(axum::body::Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn get_state(app: &axum::Router, session_cookie: Option<&str>) -> serde_json::Value {
    let resp = get_state_response(app, session_cookie).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn run_ids(state: &serde_json::Value) -> Vec<i64> {
    let mut ids: Vec<i64> = state["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

fn job_ids(state: &serde_json::Value) -> Vec<i64> {
    let mut ids: Vec<i64> = state["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["id"].as_i64().unwrap())
        .collect();
    ids.sort_unstable();
    ids
}

#[tokio::test]
async fn sessions_with_disjoint_repo_sets_see_disjoint_runs_and_jobs() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (_none_app, auth_app, sessions, persist) = build_shared_persist_apps(pool).await;

    persist
        .apply_run_event(run_event(1, "acme", "app-a", Some(RepoId(1000))))
        .await
        .unwrap();
    persist
        .apply_job_event(atc_core::test_support::make_job_event(
            JobId(1),
            RunId(1),
            "acme",
            "app-a",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();
    persist
        .apply_run_event(run_event(2, "acme", "app-b", Some(RepoId(2000))))
        .await
        .unwrap();
    persist
        .apply_job_event(atc_core::test_support::make_job_event(
            JobId(2),
            RunId(2),
            "acme",
            "app-b",
            JobEvent::Queued {
                labels: vec![],
                steps: vec![],
            },
        ))
        .await
        .unwrap();

    let now = SystemClock.now();
    let session_a = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();
    let session_b = sessions
        .create_session(2, "user-b", &[2000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let state_a = get_state(&auth_app, Some(&session_a)).await;
    assert_eq!(
        run_ids(&state_a),
        vec![1],
        "session A sees only its repo's run"
    );
    assert_eq!(
        job_ids(&state_a),
        vec![1],
        "session A sees only its repo's job"
    );

    let state_b = get_state(&auth_app, Some(&session_b)).await;
    assert_eq!(
        run_ids(&state_b),
        vec![2],
        "session B sees only its repo's run"
    );
    assert_eq!(
        job_ids(&state_b),
        vec![2],
        "session B sees only its repo's job"
    );
}

#[tokio::test]
async fn none_repo_id_run_invisible_to_session_but_visible_under_mode_none() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (none_app, auth_app, sessions, persist) = build_shared_persist_apps(pool).await;

    // A pre-migration-shaped row: no repo_id at all.
    persist
        .apply_run_event(run_event(1, "acme", "legacy", None))
        .await
        .unwrap();

    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let filtered = get_state(&auth_app, Some(&session)).await;
    assert_eq!(
        run_ids(&filtered),
        Vec::<i64>::new(),
        "a None-repo_id run must be invisible to an authenticated session"
    );

    let unfiltered = get_state(&none_app, None).await;
    assert_eq!(
        run_ids(&unfiltered),
        vec![1],
        "the same run must remain visible under mode=none"
    );

    // Global data — untouched by filtering either way.
    assert_eq!(filtered["lastSeq"], unfiltered["lastSeq"]);
    assert_eq!(
        filtered["runnerPoolCapacities"],
        unfiltered["runnerPoolCapacities"]
    );
    assert_eq!(
        filtered["displayTtlSeconds"],
        unfiltered["displayTtlSeconds"]
    );
}

#[tokio::test]
async fn unauthenticated_request_to_state_returns_401_auth_required() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (_none_app, auth_app, _sessions, _persist) = build_shared_persist_apps(pool).await;

    let resp = get_state_response(&auth_app, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!({"reason": "auth_required"}));
}

#[tokio::test]
async fn stale_session_request_to_state_returns_401_stale_authorization() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (_none_app, auth_app, sessions, _persist) = build_shared_persist_apps(pool.clone()).await;

    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    // Backdate repos_refreshed_at past the fixture's 1-hour repo_auth_ttl —
    // deterministic, no sleep. Mirrors auth_tests.rs's whoami staleness test.
    sqlx::query!(
        "UPDATE auth_sessions SET repos_refreshed_at = now() - interval '2 hours' WHERE github_user_id = 1"
    )
    .execute(&pool)
    .await
    .unwrap();

    let resp = get_state_response(&auth_app, Some(&session)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!({"reason": "stale_authorization"}));
}
