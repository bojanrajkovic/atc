//! `GET /v1/auth/github/login` + `GET /v1/auth/github/callback` — the OAuth
//! authorization-code exchange, installation/repo enumeration, and the
//! `return_to`/popup redirect contract. Cross-user session handling lives in
//! `session_lifecycle`; logout and whoami have their own submodules.

use super::*;

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

    let logout_resp = do_logout(&app, None).await;
    assert_eq!(
        logout_resp.status(),
        StatusCode::NOT_FOUND,
        "logout route must not be mounted when auth.mode = none"
    );

    let whoami_resp = do_whoami(&app, None).await;
    assert_eq!(
        whoami_resp.status(),
        StatusCode::NOT_FOUND,
        "whoami route must not be mounted when auth.mode = none"
    );
}
