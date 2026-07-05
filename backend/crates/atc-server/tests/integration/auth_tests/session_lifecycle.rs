//! Session behavior across successive logins — refresh/reuse semantics that
//! don't fit `login_callback`'s single-flow-at-a-time tests.

use super::*;

#[tokio::test]
#[serial_test::serial]
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
            format!(
                "{}; {}",
                cookie_header("atc_flow", &flow_cookie_2),
                cookie_header("atc_session", &stale_session_cookie)
            ),
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
