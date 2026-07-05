//! `GET /v1/auth/me` — identity shape and staleness reporting.

use super::*;

#[tokio::test]
async fn whoami_without_session_returns_401_with_exact_reason() {
    let (_pool, _container, app) = setup_default().await;

    let resp = do_whoami(&app, None).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json,
        json!({"reason": "auth_required"}),
        "401 body must be exactly {{\"reason\": \"auth_required\"}}"
    );
}

#[tokio::test]
async fn whoami_with_fresh_session_returns_expected_shape() {
    let (_pool, _container, app) = setup_default().await;
    let session_cookie = login_and_get_session_cookie(&app).await;

    let resp = do_whoami(&app, Some(&session_cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["login"], "octocat");
    assert_eq!(json["repoCount"], 1);
    assert_eq!(
        json["stale"], false,
        "a just-created session must not be stale"
    );
    assert!(
        json["reposRefreshedAt"].is_string(),
        "reposRefreshedAt must be an ISO-8601 string"
    );
}

#[tokio::test]
async fn whoami_with_stale_session_reports_stale_true() {
    let (pool, _container, app) = setup_default().await;
    let session_cookie = login_and_get_session_cookie(&app).await;

    // Backdate repos_refreshed_at past the fixture's 1-hour repo_auth_ttl
    // (build_auth_test_app) directly via SQL — deterministic, no sleep.
    // Scoped to this test's own session (github_user_id 42, the mock's
    // default identity) rather than the whole table, so this stays correct
    // if a future test in this fixture creates more than one session.
    sqlx::query!(
        "UPDATE auth_sessions SET repos_refreshed_at = now() - interval '2 hours' WHERE github_user_id = 42"
    )
    .execute(&pool)
    .await
    .unwrap();

    let resp = do_whoami(&app, Some(&session_cookie)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["stale"], true,
        "repos_refreshed_at older than repo_auth_ttl must report stale"
    );
}
