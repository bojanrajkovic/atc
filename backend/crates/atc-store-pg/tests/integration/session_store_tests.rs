//! Integration tests for [`atc_store_pg::SessionStore`].
//!
//! Boots an ephemeral Postgres via testcontainers (see `common::start_pg`)
//! and exercises every `SessionStore` operation against a real database.

use std::sync::Arc;
use std::time::Duration;

use atc_core::{Clock, TestClock, fixed_test_timestamp};
use atc_store_pg::{SessionStore, TracedPool};
use chrono::TimeDelta;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;
use tokio_util::sync::CancellationToken;

use crate::common;

/// Boot Postgres, start a `SessionStore` over a `TestClock` at
/// `fixed_test_timestamp()`, and hand back everything a test might need:
/// the store, the clock (for `.advance()`), a separate pool handle (for
/// assertions that read tables directly), and the container (keep it alive
/// for the test's duration — dropping it tears down the container).
async fn setup() -> (
    Arc<SessionStore>,
    Arc<TestClock>,
    TracedPool,
    ContainerAsync<Postgres>,
) {
    let (pool, container) = common::start_pg().await;
    let clock = Arc::new(TestClock::new(fixed_test_timestamp()));
    let store = SessionStore::start(
        pool.clone(),
        Arc::clone(&clock) as Arc<dyn Clock>,
        CancellationToken::new(),
    );
    (store, clock, pool, container)
}

#[tokio::test]
async fn flow_round_trip_is_single_use() {
    let (store, _clock, _pool, _container) = setup().await;

    let flow_id = store
        .create_flow("some-state", "some-verifier", "/dashboard", true)
        .await
        .expect("create_flow should succeed");

    let flow = store
        .consume_flow(&flow_id)
        .await
        .expect("consume_flow should succeed")
        .expect("first consume should return the flow");
    assert_eq!(flow.state, "some-state");
    assert_eq!(flow.pkce_verifier, "some-verifier");
    assert_eq!(flow.return_to, "/dashboard");
    assert!(flow.popup);

    let second = store
        .consume_flow(&flow_id)
        .await
        .expect("consume_flow should succeed");
    assert!(second.is_none(), "a second consume must return None");
}

#[tokio::test]
async fn flow_older_than_ttl_reads_as_absent() {
    let (store, clock, _pool, _container) = setup().await;

    let flow_id = store
        .create_flow("state", "verifier", "/", false)
        .await
        .expect("create_flow should succeed");

    // Past the 10-minute flow TTL.
    clock.advance(TimeDelta::minutes(11));

    let flow = store
        .consume_flow(&flow_id)
        .await
        .expect("consume_flow should succeed");
    assert!(flow.is_none(), "a flow past its TTL must read as absent");
}

#[tokio::test]
async fn consume_flow_missing_id_returns_none() {
    let (store, _clock, _pool, _container) = setup().await;

    let result = store
        .consume_flow("does-not-exist")
        .await
        .expect("consume_flow should succeed");
    assert!(result.is_none());
}

#[tokio::test]
async fn session_round_trip_via_raw_id() {
    let (store, clock, _pool, _container) = setup().await;
    let now = clock.now();

    let repo_ids = vec![111i64, 222i64];
    let raw_id = store
        .create_session(
            42,
            "octocat",
            &repo_ids,
            now,
            Duration::from_secs(30 * 24 * 60 * 60),
        )
        .await
        .expect("create_session should succeed");

    let session = store
        .load_session(&raw_id)
        .await
        .expect("load_session should succeed")
        .expect("session should be loaded");
    assert_eq!(session.github_user_id, 42);
    assert_eq!(session.github_login, "octocat");
    assert_eq!(session.repo_ids, repo_ids);
    assert_eq!(session.repos_refreshed_at, now);
    assert_eq!(session.created_at, now);
}

#[tokio::test]
async fn raw_session_id_is_never_persisted() {
    let (store, clock, pool, _container) = setup().await;
    let now = clock.now();

    let raw_id = store
        .create_session(1, "user", &[1], now, Duration::from_secs(3600))
        .await
        .expect("create_session should succeed");

    // Read every text-typed column back directly and assert the raw id
    // string never appears verbatim anywhere in the row.
    let row: (String, i64, String) =
        sqlx::query_as("SELECT id_hash, github_user_id, github_login FROM auth_sessions")
            .fetch_one(&pool)
            .await
            .expect("row should exist");

    assert_ne!(row.0, raw_id, "id_hash must not equal the raw session id");
    assert_eq!(row.0.len(), 64, "id_hash should be a sha256 hex digest");
}

#[tokio::test]
async fn session_expires_on_read() {
    let (store, clock, pool, _container) = setup().await;
    let now = clock.now();

    let raw_id = store
        .create_session(1, "user", &[1], now, Duration::from_secs(60))
        .await
        .expect("create_session should succeed");

    clock.advance(TimeDelta::seconds(61));

    let session = store
        .load_session(&raw_id)
        .await
        .expect("load_session should succeed");
    assert!(session.is_none(), "an expired session must read as absent");

    // The cleanup delete is fire-and-forget (spawned, not awaited) — give
    // it a moment to land before asserting the row is gone.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    assert_eq!(
        remaining, 0,
        "load_session should best-effort delete the expired row"
    );
}

#[tokio::test]
async fn refresh_session_repos_updates_set_and_clock() {
    let (store, clock, _pool, _container) = setup().await;
    let now = clock.now();

    let raw_id = store
        .create_session(1, "user", &[1], now, Duration::from_secs(3600))
        .await
        .expect("create_session should succeed");
    let session = store
        .load_session(&raw_id)
        .await
        .expect("load_session should succeed")
        .expect("session should exist");
    assert_eq!(session.repo_ids, vec![1]);

    clock.advance(TimeDelta::minutes(5));
    let refreshed_at = clock.now();

    // `session.id_hash` is exactly the value `refresh_session_repos` takes
    // — a real caller (the OAuth callback, #455) holds it the same way,
    // from a prior `load_session` on the same raw cookie value.
    store
        .refresh_session_repos(&session.id_hash, &[1, 2, 3], refreshed_at)
        .await
        .expect("refresh_session_repos should succeed");

    let session = store
        .load_session(&raw_id)
        .await
        .expect("load_session should succeed")
        .expect("session should still exist");
    assert_eq!(session.repo_ids, vec![1, 2, 3]);
    assert_eq!(session.repos_refreshed_at, refreshed_at);
}

#[tokio::test]
async fn delete_session_removes_row() {
    let (store, clock, _pool, _container) = setup().await;
    let now = clock.now();

    let raw_id = store
        .create_session(1, "user", &[1], now, Duration::from_secs(3600))
        .await
        .expect("create_session should succeed");

    store
        .delete_session(&raw_id)
        .await
        .expect("delete_session should succeed");

    let session = store
        .load_session(&raw_id)
        .await
        .expect("load_session should succeed");
    assert!(session.is_none(), "a deleted session must read as absent");
}

#[tokio::test]
async fn sweep_expired_deletes_flows_and_sessions_and_counts_them() {
    let (store, clock, pool, _container) = setup().await;
    let now = clock.now();

    // One flow and one session that will be expired at sweep time; one of
    // each that should survive.
    let _expiring_flow = store
        .create_flow("state-1", "verifier-1", "/", false)
        .await
        .expect("create_flow should succeed");
    let _expiring_session = store
        .create_session(1, "user-1", &[1], now, Duration::from_secs(60))
        .await
        .expect("create_session should succeed");

    clock.advance(TimeDelta::minutes(11));
    let later = clock.now();

    let _fresh_flow = store
        .create_flow("state-2", "verifier-2", "/", false)
        .await
        .expect("create_flow should succeed");
    let _fresh_session = store
        .create_session(2, "user-2", &[2], later, Duration::from_secs(3600))
        .await
        .expect("create_session should succeed");

    let (flows_deleted, sessions_deleted) = store
        .sweep_expired(later)
        .await
        .expect("sweep_expired should succeed");
    assert_eq!(flows_deleted, 1, "only the stale flow should be swept");
    assert_eq!(
        sessions_deleted, 1,
        "only the expired session should be swept"
    );

    let remaining_flows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_flows")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    let remaining_sessions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("count query should succeed");
    assert_eq!(
        remaining_flows, 1,
        "the fresh flow should survive the sweep"
    );
    assert_eq!(
        remaining_sessions, 1,
        "the fresh session should survive the sweep"
    );
}

/// Cheap guard for the no-tokens-stored invariant (ADR-0014): neither table
/// may ever grow a column whose name contains "token".
#[tokio::test]
async fn schema_has_no_token_columns() {
    let (pool, _container) = common::start_pg().await;

    let columns: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT column_name FROM information_schema.columns
        WHERE table_name IN ('auth_flows', 'auth_sessions')
          AND column_name ILIKE '%token%'
        "#,
    )
    .fetch_all(&pool)
    .await
    .expect("information_schema query should succeed");

    assert!(
        columns.is_empty(),
        "auth_flows/auth_sessions must never have a token-named column, found: {columns:?}"
    );
}
