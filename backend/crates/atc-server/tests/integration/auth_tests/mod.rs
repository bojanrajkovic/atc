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
//!
//! Split by concern into submodules — see `docs/implementation-guidance.md`
//! rule 7 (files past ~500 lines / two concerns get split). Shared mock
//! server, fixture builders, and request helpers live here; each submodule
//! pulls them in via `use super::*;`.

mod auth_context;
mod login_callback;
mod logout;
mod public_repos;
mod session_lifecycle;
mod whoami;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::json;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

use atc_core::{Clock, SystemClock};
use atc_persist::PersistentStore;
use atc_server::auth::AuthRuntime;
use atc_server::github_client::GitHubClient;
use atc_server::public_repo_cache::PublicRepoCache;
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
    /// Repo IDs `GET /repositories/{id}` reports as `visibility: "public"`.
    /// Anything else 404s, matching GitHub's real behavior for a private or
    /// nonexistent repo.
    public_repo_ids: Vec<i64>,
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
        .route("/repositories/{id}", get(mock_get_repository_visibility))
        .with_state(config);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("mock GitHub server");
    });

    base_url
}

/// The common case: no pagination, no forced failures, no known public repos.
fn default_mock_config() -> MockGitHubConfig {
    MockGitHubConfig {
        base_url: String::new(), // filled in by spawn_mock_github
        paginate_installations: false,
        deny_token_exchange: false,
        cycle_pagination: false,
        public_repo_ids: Vec::new(),
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

async fn mock_get_repository_visibility(
    State(config): State<MockGitHubConfig>,
    Path(repo_id): Path<i64>,
) -> Response {
    if config.public_repo_ids.contains(&repo_id) {
        Json(json!({"id": repo_id, "visibility": "public"})).into_response()
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

// ---------------------------------------------------------------------------
// App-under-test fixture
// ---------------------------------------------------------------------------

/// Public origin used by every test — plain `http://` so cookies use the
/// non-`__Host-` names and skip `Secure`, keeping assertions on
/// `Set-Cookie` simple without needing a TLS test harness.
const TEST_PUBLIC_ORIGIN: &str = "http://public.example.test";

/// The standard `InMemoryStore` every fixture below shares. Pulled out so
/// callers that need a `public_repos` cache can hold onto the same `Arc`
/// `build_app_state` ultimately wraps into `AppState` — the cache and
/// `AppState.persist` must be the same store instance, not two independent
/// ones, or a test-seeded run would be invisible to one of them.
fn test_persist(clock: &Arc<dyn Clock>, shutdown: &CancellationToken) -> Arc<dyn PersistentStore> {
    InMemoryStore::start(
        Arc::clone(clock),
        Duration::from_secs(60 * 60),
        Duration::from_secs(60),
        None,
        shutdown.clone(),
    )
}

/// Shared `AppState` construction for both the auth-enabled and
/// auth-disabled (`mode = "none"`) fixtures below, so a field added to
/// `AppState` only needs updating in one place.
fn build_app_state(
    persist: Arc<dyn PersistentStore>,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
    auth: Option<AuthRuntime>,
) -> Arc<AppState> {
    common::TestAppState::new(persist, clock)
        .with_shutdown(shutdown)
        .with_auth(auth)
        .build()
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
    let persist = test_persist(&clock, &shutdown);
    let sessions = SessionStore::start(pool, Arc::clone(&clock), shutdown.clone());
    let github = Arc::new(GitHubClient::with_base_urls(
        "test-client-id".to_string(),
        "test-client-secret".to_string(),
        mock_base_url.clone(),
        mock_base_url,
    ));
    let public_repos = Arc::new(PublicRepoCache::new(
        Arc::clone(&persist),
        Arc::clone(&github),
        Duration::from_secs(60 * 60),
    ));
    let auth = Some(AuthRuntime {
        github,
        sessions,
        public_origin: TEST_PUBLIC_ORIGIN.to_string(),
        max_session_ttl: Duration::from_secs(30 * 24 * 60 * 60),
        repo_auth_ttl: Duration::from_secs(60 * 60),
        public_repos,
    });

    let app_state = build_app_state(persist, clock, shutdown, auth);
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
// Request helpers
// ---------------------------------------------------------------------------

fn set_cookie_value(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers.get_all(header::SET_COOKIE).iter().find_map(|v| {
        let s = v.to_str().ok()?;
        let (name, rest) = s.split_once('=')?;
        (name == cookie_name).then(|| rest.split(';').next().unwrap_or("").to_string())
    })
}

/// Build a single `name=value` `Cookie` header pair — shared by every helper
/// below that attaches a flow/session cookie to a request.
fn cookie_header(name: &str, value: &str) -> String {
    format!("{name}={value}")
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
        builder = builder.header(header::COOKIE, cookie_header("atc_flow", cookie));
    }
    let req = builder.body(axum::body::Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn do_logout(app: &axum::Router, session_cookie: Option<&str>) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("POST")
        .uri("/v1/auth/github/logout");
    if let Some(cookie) = session_cookie {
        builder = builder.header(header::COOKIE, cookie_header("atc_session", cookie));
    }
    let req = builder.body(axum::body::Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

async fn do_whoami(app: &axum::Router, session_cookie: Option<&str>) -> axum::response::Response {
    let mut builder = axum::http::Request::builder()
        .method("GET")
        .uri("/v1/auth/me");
    if let Some(cookie) = session_cookie {
        builder = builder.header(header::COOKIE, cookie_header("atc_session", cookie));
    }
    let req = builder.body(axum::body::Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

/// Complete a full real login flow and return the resulting session cookie.
async fn login_and_get_session_cookie(app: &axum::Router) -> String {
    let (flow_cookie, state) = start_real_flow(app, "").await;
    let resp = do_callback(
        app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND, "login should succeed");
    set_cookie_value(resp.headers(), "atc_session").expect("login should set a session cookie")
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

fn parts(cookie: Option<(&str, &str)>) -> axum::http::request::Parts {
    let mut builder = axum::http::Request::builder().uri("/v1/auth/me");
    if let Some((name, value)) = cookie {
        builder = builder.header(header::COOKIE, cookie_header(name, value));
    }
    builder.body(()).unwrap().into_parts().0
}
