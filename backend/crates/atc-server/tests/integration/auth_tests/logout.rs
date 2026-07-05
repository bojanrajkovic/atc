//! `POST /v1/auth/github/logout` — session deletion and idempotency.

use super::*;

#[tokio::test]
async fn logout_deletes_session_and_subsequent_whoami_is_401() {
    let (_pool, _container, app) = setup_default().await;
    let session_cookie = login_and_get_session_cookie(&app).await;

    let whoami_before = do_whoami(&app, Some(&session_cookie)).await;
    assert_eq!(
        whoami_before.status(),
        StatusCode::OK,
        "session should be valid before logout"
    );

    let logout_resp = do_logout(&app, Some(&session_cookie)).await;
    assert_eq!(logout_resp.status(), StatusCode::NO_CONTENT);
    let cleared = set_cookie_value(logout_resp.headers(), "atc_session");
    assert_eq!(
        cleared,
        Some(String::new()),
        "logout should clear the session cookie value"
    );
    assert!(
        logout_resp
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|v| v.to_str().unwrap_or("").contains("Max-Age=0")),
        "cleared cookie must carry Max-Age=0"
    );

    let whoami_after = do_whoami(&app, Some(&session_cookie)).await;
    assert_eq!(
        whoami_after.status(),
        StatusCode::UNAUTHORIZED,
        "session must be invalid immediately after logout"
    );
}

#[tokio::test]
async fn logout_is_idempotent_without_a_session_cookie() {
    let (_pool, _container, app) = setup_default().await;

    let resp = do_logout(&app, None).await;
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "logout with no session cookie must still succeed"
    );
}

#[tokio::test]
async fn logout_is_idempotent_for_an_unknown_session_cookie() {
    let (_pool, _container, app) = setup_default().await;

    let resp = do_logout(&app, Some("not-a-real-session-id")).await;
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "logout for an unknown session must still succeed"
    );
}
