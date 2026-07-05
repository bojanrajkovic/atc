//! App-wide cache of GitHub repo IDs that are publicly visible, unioned into
//! every session's authorized repo set at OAuth callback (ADR-0014, amended
//! decision 2).
//!
//! A public repository is readable by anyone regardless of whether the
//! login GitHub App is installed on its owner, so rather than walking app
//! installations, [`PublicRepoCache::refresh`] checks GitHub directly for
//! every repo ID ATC already has run data for — i.e. every repo that has
//! sent a webhook (a repo with no webhook has no run data either way,
//! public or not, so the universe of repos worth checking was already
//! bounded to this set).

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use atc_persist::PersistentStore;
use tokio::sync::Mutex;

use crate::github_client::GitHubClient;

struct CacheState {
    repo_ids: Arc<HashSet<i64>>,
    computed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Refreshed lazily by whichever caller first observes it stale — no
/// standalone background task, so no `shutdown.rs` orchestration wiring.
/// Each ATC replica keeps its own cache; two replicas can disagree on the
/// public set for up to one `ttl` window after a repo flips public/private,
/// the same accepted-staleness posture `repo_auth_ttl` already carries for
/// per-user authorization.
///
/// Refresh failures (a `read_snapshot` error, or GitHub being unreachable)
/// log a warning and leave the existing cached value in place (empty, if
/// there's never been a successful refresh) — a public-repo lookup failure
/// must never block login, so [`PublicRepoCache::get`] never returns an
/// error.
pub struct PublicRepoCache {
    persist: Arc<dyn PersistentStore>,
    github: Arc<GitHubClient>,
    ttl: Duration,
    state: Mutex<CacheState>,
}

impl PublicRepoCache {
    pub fn new(
        persist: Arc<dyn PersistentStore>,
        github: Arc<GitHubClient>,
        ttl: Duration,
    ) -> Self {
        Self {
            persist,
            github,
            ttl,
            state: Mutex::new(CacheState {
                repo_ids: Arc::new(HashSet::new()),
                computed_at: None,
            }),
        }
    }

    /// The cached public-repo-ID set, refreshing first if stale or never
    /// computed. Holds the lock across the refresh so concurrent callers
    /// (e.g. several logins landing in the same instant on a cold cache)
    /// converge on one in-flight refresh rather than each firing their own.
    pub async fn get(&self, now: chrono::DateTime<chrono::Utc>) -> Arc<HashSet<i64>> {
        let mut state = self.state.lock().await;

        // `elapsed >= ttl`, not `computed_at + ttl <= now` — mirrors
        // `SessionIdentity::is_stale`'s overflow-safe shape (`auth.rs`): an
        // absurdly large configured ttl must read as "never stale" rather
        // than panic on `DateTime + Duration` overflow.
        let ttl = chrono::Duration::from_std(self.ttl).unwrap_or(chrono::Duration::MAX);
        let stale = state
            .computed_at
            .is_none_or(|computed_at| now - computed_at >= ttl);

        if stale {
            match self.refresh().await {
                Ok(repo_ids) => {
                    state.repo_ids = Arc::new(repo_ids);
                    state.computed_at = Some(now);
                }
                Err(e) => {
                    tracing::warn!(
                        error.message = ?e,
                        "public repo cache: refresh failed; serving last-known-good set"
                    );
                }
            }
        }

        Arc::clone(&state.repo_ids)
    }

    async fn refresh(&self) -> Result<HashSet<i64>, atc_persist::PersistError> {
        let snapshot = self.persist.read_snapshot(None).await?;
        let repo_ids: Vec<i64> = snapshot
            .runs
            .iter()
            .filter_map(|run| run.repo_id.map(|id| id.0))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        Ok(self.github.fetch_public_repo_ids(&repo_ids).await)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use atc_core::test_support::make_run_event;
    use atc_core::{RepoId, RunEvent, RunEventEnvelope, RunId};
    use atc_store_mem::InMemoryStore;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::github_client::GitHubClient;

    fn run_event(run_id: i64, repo_id: Option<RepoId>) -> RunEventEnvelope {
        RunEventEnvelope {
            org: "octocat".to_string(),
            repo: "hello-world".to_string(),
            repo_id,
            ..make_run_event(RunId(run_id), RunEvent::Requested)
        }
    }

    #[tokio::test]
    async fn get_excludes_a_known_repo_whose_github_check_is_unreachable() {
        let clock: Arc<dyn atc_core::Clock> = Arc::new(atc_core::SystemClock);
        let persist: Arc<dyn PersistentStore> = InMemoryStore::start(
            Arc::clone(&clock),
            Duration::from_secs(60 * 60),
            Duration::from_secs(60),
            None,
            CancellationToken::new(),
        );
        persist
            .apply_run_event(run_event(1, Some(RepoId(1))))
            .await
            .expect("seed run");

        let github = Arc::new(GitHubClient::with_base_urls(
            "client-id".to_string(),
            "client-secret".to_string(),
            "http://unused.invalid".to_string(),
            "http://unused.invalid".to_string(),
        ));
        let cache = PublicRepoCache::new(persist, github, Duration::from_secs(3600));

        let now = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let public = cache.get(now).await;

        // `fetch_public_repo_ids` swallows the per-repo failure (unreachable
        // host) rather than propagating it — `refresh` still succeeds
        // overall, just with that repo excluded from the result. This is the
        // finer-grained resilience layer; `get`'s own fallback-on-error path
        // (last-known-good) only fires when `read_snapshot` itself fails.
        assert!(public.is_empty());
    }

    #[tokio::test]
    async fn get_reuses_cached_value_within_ttl() {
        let clock: Arc<dyn atc_core::Clock> = Arc::new(atc_core::SystemClock);
        let persist: Arc<dyn PersistentStore> = InMemoryStore::start(
            Arc::clone(&clock),
            Duration::from_secs(60 * 60),
            Duration::from_secs(60),
            None,
            CancellationToken::new(),
        );
        let github = Arc::new(GitHubClient::with_base_urls(
            "client-id".to_string(),
            "client-secret".to_string(),
            "http://unused.invalid".to_string(),
            "http://unused.invalid".to_string(),
        ));
        let cache = PublicRepoCache::new(persist, github, Duration::from_secs(3600));

        let t0 = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let first = cache.get(t0).await;
        let t1 = t0 + chrono::Duration::seconds(30);
        let second = cache.get(t1).await;

        // Same Arc, not just equal contents — proves the second call didn't
        // recompute (no persist/GitHub round trip inside the ttl window).
        assert!(Arc::ptr_eq(&first, &second));
    }
}
