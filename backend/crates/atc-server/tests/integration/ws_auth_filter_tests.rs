//! Integration tests for `auth.mode = "github"` on `GET /v1/ws` (#460):
//! pre-upgrade Origin validation, pre-upgrade session freshness, and
//! per-connection event filtering. Separate from `ws_tests.rs` (mode=none
//! behavior, left entirely untouched by this ticket) since this is a
//! distinct concern.

use std::time::Duration;

use atc_core::test_support::make_run_event;
use atc_core::{Clock, RepoId, RunEvent, RunEventEnvelope, RunId, SystemClock};
use atc_wire::CommittedEvent;
use axum::http::{HeaderValue, StatusCode};
use futures_util::stream::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use crate::common;

fn run_event(run_id: i64, org: &str, repo: &str, repo_id: Option<RepoId>) -> RunEventEnvelope {
    RunEventEnvelope {
        org: org.to_string(),
        repo: repo.to_string(),
        repo_id,
        ..make_run_event(RunId(run_id), RunEvent::Requested)
    }
}

/// Build a WS upgrade request with optional `Origin`/session-`Cookie`
/// headers — `tokio_tungstenite::connect_async` accepts anything
/// implementing `IntoClientRequest`, so building the request directly is
/// how tests attach headers a plain URL string can't carry.
fn ws_request(
    url: &str,
    origin: Option<&str>,
    session_cookie: Option<&str>,
) -> tokio_tungstenite::tungstenite::handshake::client::Request {
    let mut request = url.into_client_request().unwrap();
    if let Some(o) = origin {
        request
            .headers_mut()
            .insert("Origin", HeaderValue::from_str(o).unwrap());
    }
    if let Some(cookie) = session_cookie {
        request.headers_mut().insert(
            "Cookie",
            HeaderValue::from_str(&format!("atc_session={cookie}")).unwrap(),
        );
    }
    request
}

/// Assert `connect_async` failed with an HTTP (pre-upgrade) rejection and
/// return its status code — the shape every pre-upgrade check in this file
/// rejects with, distinct from a successful upgrade or a transport-level
/// error. Generic over the `Ok` payload (never inspected — the `Ok` arm
/// always panics) so the signature doesn't need to name `connect_async`'s
/// concrete stream/response types.
fn expect_http_rejection<T>(
    result: Result<T, tokio_tungstenite::tungstenite::Error>,
) -> StatusCode {
    match result {
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            StatusCode::from_u16(response.status().as_u16()).unwrap()
        }
        Err(e) => panic!("expected an HTTP rejection response, got a different error: {e}"),
        Ok(_) => panic!("expected the upgrade to be rejected, but it succeeded"),
    }
}

#[tokio::test]
#[serial_test::serial]
async fn sessions_with_disjoint_repo_sets_receive_only_their_own_events() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, app_state, sessions) = common::spawn_auth_server(pool).await;

    let now = SystemClock.now();
    let session_a = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();
    let session_b = sessions
        .create_session(2, "user-b", &[2000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let ws_url = format!("ws://{addr}/v1/ws");
    let req_a = ws_request(
        &ws_url,
        Some(common::AUTH_TEST_PUBLIC_ORIGIN),
        Some(&session_a),
    );
    let req_b = ws_request(
        &ws_url,
        Some(common::AUTH_TEST_PUBLIC_ORIGIN),
        Some(&session_b),
    );

    let (mut socket_a, _) = tokio_tungstenite::connect_async(req_a)
        .await
        .expect("session A's upgrade should succeed");
    let (mut socket_b, _) = tokio_tungstenite::connect_async(req_b)
        .await
        .expect("session B's upgrade should succeed");

    common::consume_server_hello(&mut socket_a).await;
    common::consume_server_hello(&mut socket_b).await;

    app_state
        .persist
        .apply_run_event(run_event(1, "acme", "app-a", Some(RepoId(1000))))
        .await
        .unwrap();
    app_state
        .persist
        .apply_run_event(run_event(2, "acme", "app-b", Some(RepoId(2000))))
        .await
        .unwrap();

    let frame_a = tokio::time::timeout(Duration::from_secs(2), socket_a.next())
        .await
        .expect("timed out waiting for session A's event")
        .expect("socket A next() should return Some")
        .expect("socket A frame should be Ok");
    let event_a: CommittedEvent = match frame_a {
        Message::Text(t) => serde_json::from_str(&t).expect("CommittedEvent JSON"),
        other => panic!("expected text frame, got: {other:?}"),
    };
    assert_eq!(
        event_a.seq, 1,
        "session A must receive its own repo's run event"
    );

    let frame_b = tokio::time::timeout(Duration::from_secs(2), socket_b.next())
        .await
        .expect("timed out waiting for session B's event")
        .expect("socket B next() should return Some")
        .expect("socket B frame should be Ok");
    let event_b: CommittedEvent = match frame_b {
        Message::Text(t) => serde_json::from_str(&t).expect("CommittedEvent JSON"),
        other => panic!("expected text frame, got: {other:?}"),
    };
    assert_eq!(
        event_b.seq, 2,
        "session B must receive its own repo's run event"
    );

    // Neither socket receives a second frame — the other repo's event was
    // filtered out, not merely delayed.
    let extra_a = tokio::time::timeout(Duration::from_millis(300), socket_a.next()).await;
    assert!(
        extra_a.is_err(),
        "session A must not receive session B's event"
    );
    let extra_b = tokio::time::timeout(Duration::from_millis(300), socket_b.next()).await;
    assert!(
        extra_b.is_err(),
        "session B must not receive session A's event"
    );
}

/// Config/hello frames are global operator data — visible to any
/// authenticated user regardless of their repo set, unlike `Committed`
/// events. Only the `committed_rx` branch checks `ctx.can_see`.
#[tokio::test]
#[serial_test::serial]
async fn config_update_reaches_authenticated_clients_regardless_of_repo_set() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, app_state, sessions) = common::spawn_auth_server(pool).await;

    let now = SystemClock.now();
    let session_a = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();
    let session_b = sessions
        .create_session(2, "user-b", &[2000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let ws_url = format!("ws://{addr}/v1/ws");
    let req_a = ws_request(
        &ws_url,
        Some(common::AUTH_TEST_PUBLIC_ORIGIN),
        Some(&session_a),
    );
    let req_b = ws_request(
        &ws_url,
        Some(common::AUTH_TEST_PUBLIC_ORIGIN),
        Some(&session_b),
    );
    let (mut socket_a, _) = tokio_tungstenite::connect_async(req_a).await.unwrap();
    let (mut socket_b, _) = tokio_tungstenite::connect_async(req_b).await.unwrap();
    common::consume_server_hello(&mut socket_a).await;
    common::consume_server_hello(&mut socket_b).await;

    app_state
        .config_events_tx
        .send(atc_server::config_watcher::ConfigEvent::Update(vec![]))
        .unwrap();

    for socket in [&mut socket_a, &mut socket_b] {
        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("timed out waiting for ConfigUpdate")
            .expect("socket next() should return Some")
            .expect("socket frame should be Ok");
        let json: serde_json::Value = match frame {
            Message::Text(t) => serde_json::from_str(&t).unwrap(),
            other => panic!("expected text frame, got: {other:?}"),
        };
        assert_eq!(json["kind"], "ConfigUpdate");
    }
}

#[tokio::test]
#[serial_test::serial]
async fn upgrade_rejected_for_cross_origin() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, _app_state, sessions) = common::spawn_auth_server(pool).await;
    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let ws_url = format!("ws://{addr}/v1/ws");
    let req = ws_request(&ws_url, Some("http://evil.example.test"), Some(&session));
    let status = expect_http_rejection(tokio_tungstenite::connect_async(req).await);
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial_test::serial]
async fn upgrade_rejected_for_missing_origin() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, _app_state, sessions) = common::spawn_auth_server(pool).await;
    let now = SystemClock.now();
    let session = sessions
        .create_session(1, "user-a", &[1000], now, Duration::from_secs(3600))
        .await
        .unwrap();

    let ws_url = format!("ws://{addr}/v1/ws");
    let req = ws_request(&ws_url, None, Some(&session));
    let status = expect_http_rejection(tokio_tungstenite::connect_async(req).await);
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
#[serial_test::serial]
async fn upgrade_rejected_for_missing_session() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, _app_state, _sessions) = common::spawn_auth_server(pool).await;

    let ws_url = format!("ws://{addr}/v1/ws");
    let req = ws_request(&ws_url, Some(common::AUTH_TEST_PUBLIC_ORIGIN), None);
    let status = expect_http_rejection(tokio_tungstenite::connect_async(req).await);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
#[serial_test::serial]
async fn upgrade_rejected_for_stale_session() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let (addr, _app_state, sessions) = common::spawn_auth_server(pool.clone()).await;
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

    let ws_url = format!("ws://{addr}/v1/ws");
    let req = ws_request(
        &ws_url,
        Some(common::AUTH_TEST_PUBLIC_ORIGIN),
        Some(&session),
    );
    let status = expect_http_rejection(tokio_tungstenite::connect_async(req).await);
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
