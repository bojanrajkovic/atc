//! Integration tests for `auth.mode = "github"` filtering of `GET /v1/state`
//! (#459). Separate from `state_tests.rs` (mode=none snapshot behavior) since
//! this is a distinct concern — seeding multi-repo state through
//! `PersistentStore` directly and comparing an auth-enabled response against
//! an unfiltered one over the SAME underlying data.

use std::sync::Arc;
use std::time::Duration;

use axum::http::{StatusCode, header};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use atc_core::test_support::make_run_event;
use atc_core::{Clock, JobEvent, JobId, RepoId, RunEvent, RunEventEnvelope, RunId, SystemClock};
use atc_persist::PersistentStore;
use atc_server::auth::AuthRuntime;
use atc_server::github_client::GitHubClient;
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
    // `SessionStore::start` registers OTel counters against the global
    // meter provider — must run after the harness installs it, mirroring
    // `common::spawn_auth_server` and `auth_tests::build_auth_test_app`'s
    // same call at the top of every fixture that constructs a `SessionStore`.
    common::ensure_recorder_installed();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shutdown = CancellationToken::new();
    let persist: Arc<dyn PersistentStore> = InMemoryStore::start(
        Arc::clone(&clock),
        Duration::from_secs(60 * 60),
        Duration::from_secs(60),
        None,
        shutdown.clone(),
    );

    let none_state = common::TestAppState::new(Arc::clone(&persist), Arc::clone(&clock))
        .with_shutdown(shutdown.clone())
        .build();
    let none_app = common::bare_api_router(false, none_state);

    let sessions = SessionStore::start(pool, Arc::clone(&clock), shutdown.clone());
    let github = Arc::new(GitHubClient::with_base_urls(
        "unused-client-id".to_string(),
        "unused-client-secret".to_string(),
        "http://unused.invalid".to_string(),
        "http://unused.invalid".to_string(),
    ));
    let public_repos = Arc::new(atc_server::public_repo_cache::PublicRepoCache::new(
        Arc::clone(&persist),
        Arc::clone(&github),
        Duration::from_secs(60 * 60),
    ));
    let auth_runtime = AuthRuntime {
        github,
        sessions: Arc::clone(&sessions),
        public_origin: TEST_PUBLIC_ORIGIN.to_string(),
        max_session_ttl: Duration::from_secs(30 * 24 * 60 * 60),
        repo_auth_ttl: Duration::from_secs(60 * 60),
        public_repos,
    };
    let auth_state = common::TestAppState::new(Arc::clone(&persist), clock)
        .with_shutdown(shutdown)
        .with_auth(Some(auth_runtime))
        .build();
    let auth_app = common::bare_api_router(true, auth_state);

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

fn job_event(job_id: i64, run_id: i64, org: &str, repo: &str) -> atc_core::JobEventEnvelope {
    atc_core::test_support::make_job_event(
        JobId(job_id),
        RunId(run_id),
        org,
        repo,
        JobEvent::Queued {
            labels: vec![],
            steps: vec![],
        },
    )
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
        .apply_job_event(job_event(1, 1, "acme", "app-a"))
        .await
        .unwrap();
    persist
        .apply_run_event(run_event(2, "acme", "app-b", Some(RepoId(2000))))
        .await
        .unwrap();
    persist
        .apply_job_event(job_event(2, 2, "acme", "app-b"))
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

/// A job-before-run race (GitHub's `workflow_job` webhook can arrive before
/// `workflow_run`) leaves a job with no matching entry in `snap.runs` — both
/// stores still surface it under mode=none (an "orphan" job), but its
/// repo_id is only ever knowable through the run it has no visible parent
/// for. This asserts the auth-filtered path fails closed on it rather than
/// leaking it to a session that can't be verified as authorized.
#[tokio::test]
async fn job_arriving_before_its_run_is_invisible_to_a_session_but_visible_under_mode_none() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (none_app, auth_app, sessions, persist) = build_shared_persist_apps(pool).await;

    // No apply_run_event at all — the job event alone.
    persist
        .apply_job_event(job_event(1, 1, "acme", "app-a"))
        .await
        .unwrap();

    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let filtered = get_state(&auth_app, Some(&session)).await;
    assert_eq!(
        job_ids(&filtered),
        Vec::<i64>::new(),
        "an orphan job (parent run not yet arrived) must fail closed for an authenticated session"
    );

    let unfiltered = get_state(&none_app, None).await;
    assert_eq!(
        job_ids(&unfiltered),
        vec![1],
        "the orphan job remains visible under mode=none, matching today's behavior"
    );
}

/// The same "job visible without its run in `snap.runs`" shape, via a
/// different route: a re-run's job arrives at a higher `run_attempt` than
/// its parent run row, whose prior attempt has already aged past
/// `display_ttl` and is excluded from `snap.runs` by the cutoff. Both stores
/// deliberately keep such a job visible under mode=none rather than gating
/// a fresh re-run's queued job on its stale predecessor's cutoff — the same
/// fail-closed tradeoff as the job-before-run case above applies here too.
#[tokio::test]
async fn rerun_job_past_parents_cutoff_is_invisible_to_a_session_but_visible_under_mode_none() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (none_app, auth_app, sessions, persist) = build_shared_persist_apps(pool).await;

    let now = SystemClock.now();
    // Attempt 1: completed long enough ago to fall past the fixture's
    // 1-hour display_ttl cutoff.
    persist
        .apply_run_event(RunEventEnvelope {
            org: "acme".to_string(),
            repo: "app-a".to_string(),
            repo_id: Some(RepoId(1000)),
            completed_at: Some(now - chrono::Duration::hours(2)),
            ..make_run_event(
                RunId(1),
                RunEvent::Completed {
                    conclusion: atc_core::RunConclusion::Success,
                },
            )
        })
        .await
        .unwrap();
    // Attempt 2's job arrives before the run event that would advance the
    // parent row's run_attempt.
    persist
        .apply_job_event(atc_core::JobEventEnvelope {
            run_attempt: 2,
            ..job_event(1, 1, "acme", "app-a")
        })
        .await
        .unwrap();

    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let filtered = get_state(&auth_app, Some(&session)).await;
    assert_eq!(
        job_ids(&filtered),
        Vec::<i64>::new(),
        "a re-run job past its stale predecessor's cutoff must fail closed for a session"
    );

    let unfiltered = get_state(&none_app, None).await;
    assert_eq!(
        job_ids(&unfiltered),
        vec![1],
        "the re-run job remains visible under mode=none, matching today's behavior"
    );
}

#[tokio::test]
async fn session_filtered_response_is_marked_uncacheable_but_mode_none_is_not() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (none_app, auth_app, sessions, _persist) = build_shared_persist_apps(pool).await;

    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let filtered_resp = get_state_response(&auth_app, Some(&session)).await;
    assert_eq!(
        filtered_resp.headers().get(header::CACHE_CONTROL),
        Some(&axum::http::HeaderValue::from_static("private, no-store")),
        "a session-filtered response must never be cached by an intermediary"
    );

    let unfiltered_resp = get_state_response(&none_app, None).await;
    assert_eq!(
        unfiltered_resp.headers().get(header::CACHE_CONTROL),
        None,
        "mode=none must remain byte-for-byte unaffected — no new header"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn unauthenticated_request_to_state_returns_401_auth_required() {
    common::ensure_recorder_installed();
    common::reset_metrics();
    let (pool, _container, _db_url) = common::start_pg().await;
    let (_none_app, auth_app, _sessions, _persist) = build_shared_persist_apps(pool).await;

    let resp = get_state_response(&auth_app, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, serde_json::json!({"reason": "auth_required"}));

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_rejections_total",
            &[
                opentelemetry::KeyValue::new("surface", "state"),
                opentelemetry::KeyValue::new("reason", "auth_required"),
            ],
        ),
        1,
        "surface=state, reason=auth_required must increment atc_auth_rejections_total"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn stale_session_request_to_state_returns_401_stale_authorization() {
    common::ensure_recorder_installed();
    common::reset_metrics();
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

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_rejections_total",
            &[
                opentelemetry::KeyValue::new("surface", "state"),
                opentelemetry::KeyValue::new("reason", "stale_authorization"),
            ],
        ),
        1,
        "surface=state, reason=stale_authorization must increment atc_auth_rejections_total"
    );
}
