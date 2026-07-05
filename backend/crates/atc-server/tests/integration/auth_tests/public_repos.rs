//! The public-repo visibility widening (ADR-0014, amended decision 2): a
//! session's `repo_ids` additionally includes every repo ATC already has
//! run data for that GitHub reports as publicly visible — regardless of
//! whether the logging-in user is a collaborator on it.

use atc_core::test_support::make_run_event;
use atc_core::{RepoId, RunEvent, RunEventEnvelope, RunId};

use super::*;

/// A run for `repo_id`, owned by someone other than the mock's default
/// identity (id 42, "octocat") — the mock's `/user/installations` set only
/// ever reports repo `1001` (installation id 1 -> `1000 + 1`), so any other
/// repo_id seeded here is, by construction, one the logging-in user is not
/// a collaborator on.
fn run_event_for(run_id: i64, repo_id: i64) -> RunEventEnvelope {
    RunEventEnvelope {
        org: "someone-else".to_string(),
        repo: "public-repo".to_string(),
        repo_id: Some(RepoId(repo_id)),
        ..make_run_event(RunId(run_id), RunEvent::Requested)
    }
}

#[tokio::test]
#[serial_test::serial]
async fn public_repo_with_run_history_is_unioned_into_session_repo_ids() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(MockGitHubConfig {
        public_repo_ids: vec![9999],
        ..default_mock_config()
    })
    .await;
    let (app, state) = build_auth_test_app(pool.clone(), mock_base).await;

    state
        .persist
        .apply_run_event(run_event_for(1, 9999))
        .await
        .expect("seed run");

    login_and_get_session_cookie(&app).await;

    let repo_ids: Vec<i64> = sqlx::query_scalar("SELECT repo_ids FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("session row should exist");
    let mut sorted = repo_ids;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![1001, 9999],
        "session repo_ids must include both the collaborator-accessible repo \
         (1001) and the public repo (9999) the user has no collaborator access to"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn a_multi_repo_known_set_keeps_only_the_public_ones() {
    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(MockGitHubConfig {
        public_repo_ids: vec![9999, 9998],
        ..default_mock_config()
    })
    .await;
    let (app, state) = build_auth_test_app(pool.clone(), mock_base).await;

    // Four known repos, checked concurrently in one cache refresh: two
    // public (9999, 9998), one private/gone (8888), and the one the user
    // already has collaborator access to via installations (1001, seeded
    // separately below by the login itself).
    for (run_id, repo_id) in [(1, 9999), (2, 9998), (3, 8888)] {
        state
            .persist
            .apply_run_event(run_event_for(run_id, repo_id))
            .await
            .expect("seed run");
    }

    login_and_get_session_cookie(&app).await;

    let repo_ids: Vec<i64> = sqlx::query_scalar("SELECT repo_ids FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("session row should exist");
    let mut sorted = repo_ids;
    sorted.sort_unstable();
    assert_eq!(
        sorted,
        vec![1001, 9998, 9999],
        "only the two public repos (9998, 9999) join the collaborator-accessible \
         one (1001); the private/gone repo (8888) must not"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn known_repo_that_github_reports_as_not_public_is_not_unioned_in() {
    let (pool, _container, _db_url) = common::start_pg().await;
    // No `public_repo_ids` configured — the mock's `GET /repositories/{id}`
    // 404s for everything, matching GitHub's real behavior for a private repo.
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (app, state) = build_auth_test_app(pool.clone(), mock_base).await;

    state
        .persist
        .apply_run_event(run_event_for(1, 9999))
        .await
        .expect("seed run");

    login_and_get_session_cookie(&app).await;

    let repo_ids: Vec<i64> = sqlx::query_scalar("SELECT repo_ids FROM auth_sessions")
        .fetch_one(&pool)
        .await
        .expect("session row should exist");
    assert_eq!(
        repo_ids,
        vec![1001],
        "a known repo GitHub reports as private/nonexistent must not be added"
    );
}
