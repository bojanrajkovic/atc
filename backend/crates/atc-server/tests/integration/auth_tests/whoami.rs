//! `GET /v1/auth/me` — identity shape and staleness reporting.

use super::*;

#[tokio::test]
#[serial_test::serial]
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
#[serial_test::serial]
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
#[serial_test::serial]
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

/// `load_session` best-effort-deletes an expired row via a bare
/// `tokio::spawn` it doesn't await — regression coverage for the bug fixed
/// alongside this test: that spawn didn't carry the caller's ambient span
/// in, so the `TracedPool`-instrumented DELETE exported as a disconnected
/// root instead of nesting under the request's `http.request` span (see
/// docs/architecture/metrics.md § "Background-task boundaries").
#[tokio::test]
#[serial_test::serial]
async fn expired_session_delete_nests_under_request_span() {
    let (pool, _container, app) = setup_default().await;
    let session_cookie = login_and_get_session_cookie(&app).await;

    // Backdate expires_at into the past directly via SQL — deterministic,
    // no sleep, same pattern `whoami_with_stale_session_reports_stale_true`
    // uses for repos_refreshed_at.
    sqlx::query!(
        "UPDATE auth_sessions SET expires_at = now() - interval '1 minute' WHERE github_user_id = 42"
    )
    .execute(&pool)
    .await
    .unwrap();

    // Reset now, after setup/login's own DB writes — only spans from the
    // expired-session request below should be in the captured set.
    common::reset_spans();

    let resp = do_whoami(&app, Some(&session_cookie)).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let _ = axum::body::to_bytes(resp.into_body(), usize::MAX).await;

    // The cleanup delete isn't awaited by the request — give the spawned
    // task a moment to finish and its span to close before reading spans.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let spans = common::read_finished_spans();
    let delete = common::span_named(&spans, "sqlx.execute")
        .expect("the expired-session cleanup DELETE must emit a sqlx.execute span");
    assert_eq!(
        common::parent_of(&spans, delete).map(|p| p.name.as_ref()),
        Some("http.request"),
        "the cleanup delete must nest under the request's http.request span, not export as a disconnected root"
    );
}
