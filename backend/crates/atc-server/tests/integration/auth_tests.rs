//! Integration tests for the `auth.github` login + callback endpoints.
//!
//! No `wiremock` precedent exists in this workspace; a small hand-rolled
//! axum router mocks the four GitHub endpoints the flow calls, matching
//! this crate's existing style of building real `Router`s for test
//! fixtures rather than a general-purpose HTTP-mocking dependency. The
//! mock binds a real TCP listener (unlike the app-under-test, which is
//! driven via `tower::ServiceExt::oneshot` — `reqwest` inside
//! `GitHubClient` makes real socket connections, so the mock has to be a
//! real server, not an in-process `Service`).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde_json::json;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tower::ServiceExt;

use atc_core::{Clock, SystemClock};
use atc_server::auth::AuthRuntime;
use atc_server::github_client::GitHubClient;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use atc_store_pg::SessionStore;

use crate::common;

// ---------------------------------------------------------------------------
// Mock GitHub server
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockGitHubConfig {
    base_url: String,
    paginate_installations: bool,
    deny_token_exchange: bool,
    /// Always answer `/user/installations` with a `Link: rel="next"`
    /// pointing at itself, regardless of the `page` param — simulates a
    /// malformed/cycling pagination response for the page-cap test.
    cycle_pagination: bool,
}

async fn spawn_mock_github(config: MockGitHubConfig) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock GitHub listener");
    let addr = listener.local_addr().expect("local_addr");
    let base_url = format!("http://{addr}");
    let config = MockGitHubConfig {
        base_url: base_url.clone(),
        ..config
    };

    let app = axum::Router::new()
        .route("/login/oauth/access_token", post(mock_token_exchange))
        .route("/user", get(mock_get_user))
        .route("/user/installations", get(mock_get_installations))
        .route(
            "/user/installations/{id}/repositories",
            get(mock_get_repositories),
        )
        .with_state(config);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GitHub server");
    });

    base_url
}

/// The common case: no pagination, no forced failures.
fn default_mock_config() -> MockGitHubConfig {
    MockGitHubConfig {
        base_url: String::new(), // filled in by spawn_mock_github
        paginate_installations: false,
        deny_token_exchange: false,
        cycle_pagination: false,
    }
}

/// The `code` value a client must send to authenticate as the mock's
/// second identity (id 99, "otheroctocat") — used by the cross-user session
/// test. Any other `code` authenticates as the default identity (id 42).
const SECOND_USER_CODE: &str = "code-for-second-user";

async fn mock_token_exchange(
    State(config): State<MockGitHubConfig>,
    axum::extract::Form(params): axum::extract::Form<HashMap<String, String>>,
) -> impl IntoResponse {
    if config.deny_token_exchange {
        return Json(json!({
            "error": "bad_verification_code",
            "error_description": "The code passed is incorrect or expired.",
        }));
    }
    let access_token = if params.get("code").map(String::as_str) == Some(SECOND_USER_CODE) {
        "mock-access-token-second-user"
    } else {
        "mock-access-token"
    };
    Json(json!({
        "access_token": access_token,
        "token_type": "bearer",
        "scope": "",
        "expires_in": 28800,
        "refresh_token": "mock-refresh-token",
        "refresh_token_expires_in": 15_897_600,
    }))
}

async fn mock_get_user(headers: HeaderMap) -> impl IntoResponse {
    let is_second_user = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("mock-access-token-second-user"));
    if is_second_user {
        Json(json!({"id": 99, "login": "otherocat"}))
    } else {
        Json(json!({"id": 42, "login": "octocat"}))
    }
}

async fn mock_get_installations(
    State(config): State<MockGitHubConfig>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    let body = if config.cycle_pagination {
        headers.insert(
            header::LINK,
            format!(
                r#"<{}/user/installations?per_page=100&page=next>; rel="next""#,
                config.base_url
            )
            .parse()
            .expect("valid header value"),
        );
        json!({"total_count": 1, "installations": [{"id": 1}]})
    } else if config.paginate_installations && params.get("page").map(String::as_str) != Some("2") {
        headers.insert(
            header::LINK,
            format!(
                r#"<{}/user/installations?per_page=100&page=2>; rel="next""#,
                config.base_url
            )
            .parse()
            .expect("valid header value"),
        );
        json!({"total_count": 2, "installations": [{"id": 1}]})
    } else if config.paginate_installations {
        json!({"total_count": 2, "installations": [{"id": 2}]})
    } else {
        json!({"total_count": 1, "installations": [{"id": 1}]})
    };
    (headers, Json(body))
}

async fn mock_get_repositories(Path(installation_id): Path<i64>) -> impl IntoResponse {
    Json(json!({
        "total_count": 1,
        "repository_selection": "selected",
        "repositories": [{"id": 1000 + installation_id}],
    }))
}

// ---------------------------------------------------------------------------
// App-under-test fixture
// ---------------------------------------------------------------------------

/// Public origin used by every test — plain `http://` so cookies use the
/// non-`__Host-` names and skip `Secure`, keeping assertions on
/// `Set-Cookie` simple without needing a TLS test harness.
const TEST_PUBLIC_ORIGIN: &str = "http://public.example.test";

/// Shared `AppState` construction for both the auth-enabled and
/// auth-disabled (`mode = "none"`) fixtures below, so a field added to
/// `AppState` only needs updating in one place.
fn build_app_state(
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
    auth: Option<AuthRuntime>,
) -> Arc<AppState> {
    let persist = InMemoryStore::start(
        Arc::clone(&clock),
        Duration::from_secs(60 * 60),
        Duration::from_secs(60),
        None,
        shutdown.clone(),
    );
    Arc::new(AppState {
        persist,
        clock,
        display_ttl: Duration::from_secs(60 * 60),
        webhook_secret: None,
        runner_pool_capacities: tokio::sync::RwLock::new(Vec::new()),
        config_events_tx: broadcast::channel(16).0,
        shutdown,
        ws_tracker: TaskTracker::new(),
        ws_metrics: atc_server::ws::WsMetrics::register(),
        auth,
    })
}

/// Build a full app router with `auth.mode = "github"` wired to a mock
/// GitHub server. Uses `InMemoryStore` for `persist` — these tests exercise
/// only the auth surface, not the run/job domain, so the lighter store is
/// enough; `SessionStore` still runs against a real Postgres pool (`pool`),
/// matching the crate's existing convention of testing PG-backed
/// components against a real database rather than a fake.
async fn build_auth_test_app(
    pool: atc_store_pg::TracedPool,
    mock_base_url: String,
) -> (axum::Router, Arc<AppState>) {
    common::ensure_recorder_installed();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shutdown = CancellationToken::new();
    let sessions = SessionStore::start(pool, Arc::clone(&clock), shutdown.clone());
    let github = Arc::new(GitHubClient::with_base_urls(
        "test-client-id".to_string(),
        "test-client-secret".to_string(),
        mock_base_url.clone(),
        mock_base_url,
    ));
    let auth = Some(AuthRuntime {
        github,
        sessions,
        public_origin: TEST_PUBLIC_ORIGIN.to_string(),
        max_session_ttl: Duration::from_secs(30 * 24 * 60 * 60),
    });

    let app_state = build_app_state(clock, shutdown, auth);
    let app = atc_server::routes::api_routes(true).with_state(app_state.clone());
    (app, app_state)
}

/// The common fixture: real Postgres, a non-paginating/non-denying mock
/// GitHub, and the app router. Covers every test that doesn't need a
/// specific mock behavior.
async fn setup_default() -> (
    atc_store_pg::TracedPool,
    ContainerAsync<Postgres>,
    axum::Router,
) {
    let (pool, container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (app, _state) = build_auth_test_app(pool.clone(), mock_base).await;
    (pool, container, app)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn set_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers.get_all(header::SET_COOKIE).iter().find_map(|v| {
        let s = v.to_str().ok()?;
        let (name, rest) = s.split_once('=')?;
        (name == cookie_name).then(|| rest.split(';').next().unwrap_or("").to_string())
    })
}

async fn do_login(app: &axum::Router, query: &str) -> (StatusCode, HeaderMap) {
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/v1/auth/github/login{query}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    (status, headers)
}

async fn do_callback(
    app: &axum::Router,
    query: &str,
    flow_cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("GET")
        .uri(format!("/v1/auth/github/callback{query}"));
    if let Some(cookie) = flow_cookie {
        builder = builder.header(header::COOKIE, format!("atc_flow={cookie}"));
    }
    let req = builder.body(axum::body::Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// Drive `/v1/auth/github/login` and extract the flow cookie + `state` query
/// param from the resulting redirect, so callback tests can round-trip a
/// real flow instead of hand-constructing one.
async fn start_real_flow(app: &axum::Router, query: &str) -> (String, String) {
    let (status, headers) = do_login(app, query).await;
    assert_eq!(status, StatusCode::FOUND, "login should redirect");
    let flow_cookie =
        set_cookie_value(&headers, "atc_flow").expect("login should set atc_flow cookie");
    let location = headers.get(header::LOCATION).unwrap().to_str().unwrap();
    let url = reqwest::Url::parse(location).expect("Location should be a valid URL");
    let state = url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .expect("authorize URL should carry a state param");
    (flow_cookie, state)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn happy_path_creates_session_with_identity_and_repo_ids_no_token_material() {
    let (pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "?return_to=/dashboard").await;

    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::FOUND,
        "successful callback should redirect"
    );
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "/dashboard"
    );
    let session_cookie = set_cookie_value(resp.headers(), "atc_session")
        .expect("callback should set atc_session cookie");

    // The row contains identity + repo_ids and nothing token-shaped.
    let row: (i64, String, Vec<i64>) =
        sqlx::query_as("SELECT github_user_id, github_login, repo_ids FROM auth_sessions")
            .fetch_one(&pool)
            .await
            .expect("session row should exist");
    assert_eq!(row.0, 42);
    assert_eq!(row.1, "octocat");
    assert_eq!(row.2, vec![1001]); // installation id 1 -> mock repo id 1000+1

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name FROM information_schema.columns WHERE table_name = 'auth_sessions' AND column_name ILIKE '%token%'",
    )
    .fetch_all(&pool)
    .await
    .expect("schema query should succeed");
    assert!(columns.is_empty(), "no token-shaped column should exist");

    assert!(!session_cookie.is_empty());
}

#[tokio::test]
async fn pagination_across_two_pages_collects_every_installation() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(MockGitHubConfig {
        paginate_installations: true,
        ..default_mock_config()
    })
    .await;
    let (app, _state) = build_auth_test_app(pool.clone(), mock_base).await;

    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);

    let repo_ids: Vec<i64> = sqlx::query_scalar("SELECT repo_ids FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("session row should exist");
    // Page 1 -> installation 1 -> repo 1001; page 2 -> installation 2 -> repo 1002.
    let mut sorted = repo_ids;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![1001, 1002],
        "both pages' installations must be collected"
    );
}

#[tokio::test]
async fn cycling_pagination_is_bounded_not_infinite() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(MockGitHubConfig {
        cycle_pagination: true,
        ..default_mock_config()
    })
    .await;
    let (app, _state) = build_auth_test_app(pool.clone(), mock_base).await;

    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = tokio::time::timeout(
        Duration::from_secs(30),
        do_callback(
            &app,
            &format!("?code=good-code&state={state}"),
            Some(&flow_cookie),
        ),
    )
    .await
    .expect(
        "a cycling Link header must not hang the request indefinitely — the page cap must bound it",
    );
    assert_eq!(
        resp.status(),
        StatusCode::BAD_GATEWAY,
        "exceeding the page cap should surface as a GitHub-call failure"
    );

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        count, 0,
        "a bounded pagination failure must not create a session"
    );
}

#[tokio::test]
async fn state_mismatch_is_rejected_without_a_session() {
    let (pool, _container, app) = setup_default().await;

    let (flow_cookie, _state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        "?code=good-code&state=wrong-state",
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "a state mismatch must not create a session");
}

#[tokio::test]
async fn missing_flow_cookie_is_rejected() {
    let (_pool, _container, app) = setup_default().await;

    let resp = do_callback(&app, "?code=good-code&state=some-state", None).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn reused_flow_cookie_is_rejected_on_second_use() {
    let (_pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let first = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        first.status(),
        StatusCode::FOUND,
        "first use should succeed"
    );

    let second = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        second.status(),
        StatusCode::BAD_REQUEST,
        "a reused flow cookie must be rejected"
    );
}

#[tokio::test]
async fn denied_authorization_redirects_home_with_auth_error() {
    let (pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        &format!("?error=access_denied&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "/?auth_error=denied"
    );

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0, "a denied authorization must not create a session");
}

#[tokio::test]
async fn popup_mode_returns_html_with_exact_broadcast_channel_contract() {
    let (_pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "?popup=1").await;
    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "popup mode should return 200, not a redirect"
    );
    assert!(set_cookie_value(resp.headers(), "atc_session").is_some());

    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let html = String::from_utf8(body.to_vec()).unwrap();
    assert!(html.contains("new BroadcastChannel('atc-auth')"));
    assert!(html.contains("postMessage('session-refreshed')"));
    assert!(html.contains("window.close()"));
}

#[tokio::test]
async fn token_exchange_failure_is_surfaced_without_a_session() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(MockGitHubConfig {
        deny_token_exchange: true,
        ..default_mock_config()
    })
    .await;
    let (app, _state) = build_auth_test_app(pool.clone(), mock_base).await;

    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        &format!("?code=bad-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);

    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn return_to_rejects_absolute_url() {
    let (_pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "?return_to=https://evil.example.com").await;
    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "/",
        "an absolute return_to must fall back to /"
    );
}

#[tokio::test]
async fn return_to_rejects_scheme_relative_url() {
    let (_pool, _container, app) = setup_default().await;

    let (flow_cookie, state) = start_real_flow(&app, "?return_to=//evil.example.com").await;
    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(
        resp.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "/",
        "a scheme-relative return_to must fall back to /"
    );
}

#[tokio::test]
async fn existing_session_for_a_different_user_is_not_refreshed() {
    let (pool, _container, app) = setup_default().await;

    // Log in as the mock's default identity (id 42, "octocat") to obtain a
    // real session cookie — simulating a browser that already has a
    // session for one account.
    let (flow_cookie_1, state_1) = start_real_flow(&app, "").await;
    let first_login = do_callback(
        &app,
        &format!("?code=good-code&state={state_1}"),
        Some(&flow_cookie_1),
    )
    .await;
    assert_eq!(first_login.status(), StatusCode::FOUND);
    let stale_session_cookie = set_cookie_value(first_login.headers(), "atc_session")
        .expect("first login should set a session cookie");

    // A second, independent login flow completes as a DIFFERENT GitHub user
    // (id 99, via SECOND_USER_CODE) with the first user's session cookie
    // still attached to the request, as a shared browser or stale tab
    // would send it.
    let (flow_cookie_2, state_2) = start_real_flow(&app, "").await;
    let req = axum::http::Request::builder()
        .method("GET")
        .uri(format!(
            "/v1/auth/github/callback?code={SECOND_USER_CODE}&state={state_2}"
        ))
        .header(
            header::COOKIE,
            format!("atc_flow={flow_cookie_2}; atc_session={stale_session_cookie}"),
        )
        .body(axum::body::Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FOUND);
    let second_session_cookie = set_cookie_value(resp.headers(), "atc_session")
        .expect("second login should set a session cookie");

    assert_ne!(
        second_session_cookie, stale_session_cookie,
        "a login as a different user must not reuse the first user's session cookie"
    );

    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT github_user_id, github_login FROM auth_sessions ORDER BY github_user_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(42, "octocat".to_string()), (99, "otherocat".to_string())],
        "both users must have their own session row — the second login must not overwrite the first user's identity"
    );
}

#[tokio::test]
async fn mode_none_leaves_auth_routes_unmounted() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shutdown = CancellationToken::new();
    let app_state = build_app_state(clock, shutdown, None);
    let app = atc_server::routes::api_routes(false).with_state(app_state);

    let (status, _headers) = do_login(&app, "").await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "login route must not be mounted when auth.mode = none"
    );

    let resp = do_callback(&app, "?code=x&state=y", None).await;
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "callback route must not be mounted when auth.mode = none"
    );
}
