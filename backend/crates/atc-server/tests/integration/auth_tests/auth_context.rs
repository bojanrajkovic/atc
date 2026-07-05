//! `AuthContext`'s `FromRequestParts` extractor — the per-request auth-mode
//! dispatch that every gated route runs through.

use std::collections::HashSet;

use axum::extract::FromRequestParts;
use axum::response::IntoResponse;

use atc_core::RepoId;
use atc_server::auth::AuthContext;

use super::*;

#[tokio::test]
async fn auth_context_disabled_mode_short_circuits_without_touching_store() {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let shutdown = CancellationToken::new();
    let persist = test_persist(&clock, &shutdown);
    // `auth: None` — there is no SessionStore to touch, so a successful
    // `Disabled` extraction here proves the mode=none path never attempts
    // one.
    let app_state = build_app_state(persist, clock, shutdown, None);

    let mut parts = parts(None);
    let ctx = AuthContext::from_request_parts(&mut parts, &app_state)
        .await
        .expect("mode=none never rejects");
    assert!(matches!(ctx, AuthContext::Disabled));
}

#[tokio::test]
async fn auth_context_missing_cookie_rejects_with_auth_required() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (_app, app_state) = build_auth_test_app(pool, mock_base).await;

    let mut parts = parts(None);
    let err = AuthContext::from_request_parts(&mut parts, &app_state)
        .await
        .expect_err("no cookie must be rejected");
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, json!({"reason": "auth_required"}));
}

#[tokio::test]
async fn auth_context_unknown_session_cookie_rejects_with_auth_required() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (_app, app_state) = build_auth_test_app(pool, mock_base).await;

    let mut parts = parts(Some(("atc_session", "not-a-real-session-id")));
    let err = AuthContext::from_request_parts(&mut parts, &app_state)
        .await
        .expect_err("an unknown session cookie must be rejected");
    let resp = err.into_response();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json, json!({"reason": "auth_required"}));
}

#[tokio::test]
async fn auth_context_valid_session_extracts_identity_and_repo_ids() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (app, app_state) = build_auth_test_app(pool, mock_base).await;
    let session_cookie = login_and_get_session_cookie(&app).await;

    let mut parts = parts(Some(("atc_session", &session_cookie)));
    let ctx = AuthContext::from_request_parts(&mut parts, &app_state)
        .await
        .expect("a valid session cookie must extract");
    let AuthContext::Session(identity) = ctx else {
        panic!("expected AuthContext::Session, got Disabled");
    };
    assert_eq!(identity.github_login, "octocat");
    assert_eq!(identity.repo_ids, HashSet::from([RepoId(1001)]));
}
