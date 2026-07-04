//! Session and pre-auth OAuth flow storage for `auth.github` mode.
//!
//! [`SessionStore`] is a concrete struct sharing the same [`TracedPool`] as
//! [`crate::store::PgStore`] — it is deliberately NOT a `PersistentStore`
//! (ADR-0008): sessions are not run-state, and `auth.github` requires
//! Postgres by locked decision, so exactly one implementation exists. See
//! the design doc's "Data model" section.
//!
//! No token columns anywhere (ADR-0014): ATC derives the repo-authorization
//! set at the OAuth callback and discards both the access and refresh token
//! immediately; nothing GitHub-issued is ever persisted here.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use atc_core::Clock;
use atc_persist::join_with_timeout;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::TracedPool;

/// Pre-auth flow rows are single-use and always treated as expired 10
/// minutes after `created_at`, regardless of whether the sweep has reaped
/// them yet — binds the OAuth round-trip to a short window per the design
/// doc's "Login flow" section.
const FLOW_TTL: Duration = Duration::from_secs(10 * 60);

/// Cadence the session-store sweep task uses to delete expired flows and
/// sessions. Matches `store::OUTBOX_SWEEP_INTERVAL`'s cadence, though this
/// sweep needs none of that task's cross-replica `SKIP LOCKED` candidate
/// selection — deleting an already-deleted row is a no-op, so a plain
/// per-replica sweep on the same interval is sufficient.
const SWEEP_INTERVAL: Duration = Duration::from_secs(300);

/// Join budget for the session-store sweep task during
/// [`SessionStore::shutdown`]. Same cooperative-exit shape as
/// `store::SHUTDOWN_TIMEOUT_OUTBOX_SWEEP`.
pub const SHUTDOWN_TIMEOUT_SESSION_SWEEP: Duration = Duration::from_secs(2);

/// Random bytes in a `flow_id` or raw session id, before base64url encoding.
/// 256 bits, matching typical unguessable-session-token practice.
const TOKEN_BYTES: usize = 32;

/// A consumed pre-auth OAuth flow. See [`SessionStore::consume_flow`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Flow {
    pub state: String,
    pub pkce_verifier: String,
    pub return_to: String,
    pub popup: bool,
}

/// A loaded session. See [`SessionStore::load_session`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// `sha256(raw_id)` hex — the value [`SessionStore::refresh_session_repos`]
    /// takes, carried forward here so a caller that already loaded a
    /// session doesn't need to re-derive the hash from the raw cookie value.
    pub id_hash: String,
    pub github_user_id: i64,
    pub github_login: String,
    pub repo_ids: Vec<i64>,
    pub repos_refreshed_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Postgres-backed storage for `auth_flows` + `auth_sessions`. Shares the
/// same pool as `PgStore` but owns its own background sweep task lifecycle
/// (ADR-0006), independent of `PgStore`'s outbox retention tasks — the two
/// stores are not otherwise coupled.
pub struct SessionStore {
    pool: TracedPool,
    clock: Arc<dyn Clock>,
    handle: StdMutex<Option<JoinHandle<()>>>,
}

impl SessionStore {
    /// Construct a [`SessionStore`] over `pool` and spawn its sweep task.
    ///
    /// The sweep task is spawned over cloned `pool`/`clock` handles, not an
    /// `Arc<SessionStore>` back-reference — a task holding a strong `Arc` to
    /// the very struct that owns its `JoinHandle` would be a reference
    /// cycle, leaking the store if `shutdown` were ever skipped. `PgStore`'s
    /// own sweep (`retention::spawn_outbox_sweep`) takes `pool`/`clock`
    /// directly for the same reason.
    pub fn start(
        pool: TracedPool,
        clock: Arc<dyn Clock>,
        shutdown: CancellationToken,
    ) -> Arc<Self> {
        let handle = spawn_sweep(pool.clone(), Arc::clone(&clock), shutdown);
        Arc::new(Self {
            pool,
            clock,
            handle: StdMutex::new(Some(handle)),
        })
    }

    /// Join the sweep task. Callers must cancel the shutdown token passed to
    /// [`SessionStore::start`] first, mirroring `PgStore::shutdown`'s caller
    /// contract — otherwise this waits the full [`SHUTDOWN_TIMEOUT_SESSION_SWEEP`]
    /// budget before aborting.
    pub async fn shutdown(&self) {
        let handle = self
            .handle
            .lock()
            .expect("session sweep handle mutex poisoned")
            .take();
        if let Some(handle) = handle {
            join_with_timeout(handle, SHUTDOWN_TIMEOUT_SESSION_SWEEP, "session_sweep").await;
        }
    }

    /// Create a pre-auth flow row and return its `flow_id` — the value bound
    /// into the short-lived `__Host-atc_flow` cookie.
    pub async fn create_flow(
        &self,
        state: &str,
        pkce_verifier: &str,
        return_to: &str,
        popup: bool,
    ) -> Result<String, sqlx::Error> {
        let flow_id = random_token()?;
        let now = self.clock.now();
        sqlx::query!(
            r#"
            INSERT INTO auth_flows (flow_id, state, pkce_verifier, return_to, popup, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            flow_id,
            state,
            pkce_verifier,
            return_to,
            popup,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(flow_id)
    }

    /// Single-use consume: deletes the row and returns it, or `None` if it
    /// doesn't exist or is older than [`FLOW_TTL`] (treated as expired even
    /// if the sweep hasn't reaped it yet).
    pub async fn consume_flow(&self, flow_id: &str) -> Result<Option<Flow>, sqlx::Error> {
        let row = sqlx::query!(
            r#"
            DELETE FROM auth_flows WHERE flow_id = $1
            RETURNING state, pkce_verifier, return_to, popup, created_at
            "#,
            flow_id,
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        if self.clock.now() - row.created_at > flow_ttl_chrono() {
            return Ok(None);
        }

        Ok(Some(Flow {
            state: row.state,
            pkce_verifier: row.pkce_verifier,
            return_to: row.return_to,
            popup: row.popup,
        }))
    }

    /// Create a session row and return the raw session id — the value that
    /// goes in the `__Host-atc_session` cookie. Only `sha256(raw)` hex is
    /// persisted; the raw value is never stored.
    pub async fn create_session(
        &self,
        github_user_id: i64,
        github_login: &str,
        repo_ids: &[i64],
        now: DateTime<Utc>,
        max_ttl: Duration,
    ) -> Result<String, sqlx::Error> {
        let raw_id = random_token()?;
        let id_hash = sha256_hex(&raw_id);
        let expires_at = now
            + chrono::Duration::from_std(max_ttl).expect("max_session_ttl fits chrono::Duration");

        sqlx::query!(
            r#"
            INSERT INTO auth_sessions
                (id_hash, github_user_id, github_login, repo_ids, repos_refreshed_at, created_at, expires_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            id_hash,
            github_user_id,
            github_login,
            repo_ids,
            now,
            now,
            expires_at,
        )
        .execute(&self.pool)
        .await?;

        Ok(raw_id)
    }

    /// Load a session by its raw cookie value. Returns `None` (and
    /// best-effort deletes the row) if it doesn't exist or has expired.
    pub async fn load_session(&self, raw_id: &str) -> Result<Option<Session>, sqlx::Error> {
        let id_hash = sha256_hex(raw_id);
        let row = sqlx::query!(
            r#"
            SELECT github_user_id, github_login,
                   repo_ids AS "repo_ids!: Vec<i64>",
                   repos_refreshed_at, created_at, expires_at
            FROM auth_sessions WHERE id_hash = $1
            "#,
            id_hash.clone(),
        )
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        if row.expires_at <= self.clock.now() {
            // Best-effort, fire-and-forget: the caller sees an absent
            // session either way, so the cleanup delete doesn't need to
            // block this call — the periodic sweep would reap the row
            // regardless if this spawned task loses a race or the process
            // exits first. Not awaited: this is very plausibly a
            // per-request hot path once wired into auth middleware (#455),
            // and an expired-cookie request shouldn't pay a second
            // round-trip before it can be rejected.
            let pool = self.pool.clone();
            tokio::spawn(async move {
                let _ = sqlx::query!(r#"DELETE FROM auth_sessions WHERE id_hash = $1"#, id_hash)
                    .execute(&pool)
                    .await;
            });
            return Ok(None);
        }

        Ok(Some(Session {
            id_hash,
            github_user_id: row.github_user_id,
            github_login: row.github_login,
            repo_ids: row.repo_ids,
            repos_refreshed_at: row.repos_refreshed_at,
            created_at: row.created_at,
            expires_at: row.expires_at,
        }))
    }

    /// Update a session's authorized repo set and refresh clock. Used by
    /// silent re-auth: the callback already holds `id_hash` from the
    /// session it's refreshing, so this takes the hash directly rather than
    /// re-deriving it from a raw cookie value.
    pub async fn refresh_session_repos(
        &self,
        id_hash: &str,
        repo_ids: &[i64],
        now: DateTime<Utc>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query!(
            r#"
            UPDATE auth_sessions
               SET repo_ids = $2, repos_refreshed_at = $3
             WHERE id_hash = $1
            "#,
            id_hash,
            repo_ids,
            now,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a session by its raw cookie value. Used by logout.
    pub async fn delete_session(&self, raw_id: &str) -> Result<(), sqlx::Error> {
        let id_hash = sha256_hex(raw_id);
        sqlx::query!(r#"DELETE FROM auth_sessions WHERE id_hash = $1"#, id_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete expired flows and sessions. Returns `(flows_deleted,
    /// sessions_deleted)`. Called on [`SWEEP_INTERVAL`] by the task spawned
    /// in [`SessionStore::start`]; also directly callable (e.g. by tests
    /// that want a synchronous sweep instead of waiting on the interval).
    pub async fn sweep_expired(&self, now: DateTime<Utc>) -> Result<(u64, u64), sqlx::Error> {
        sweep_expired_impl(&self.pool, now).await
    }
}

/// Shared by [`SessionStore::sweep_expired`] and the spawned sweep task —
/// the task calls this directly over a cloned `pool` rather than through
/// `&SessionStore`, so it never needs an `Arc<SessionStore>` back-reference
/// (see [`SessionStore::start`]'s doc comment).
async fn sweep_expired_impl(
    pool: &TracedPool,
    now: DateTime<Utc>,
) -> Result<(u64, u64), sqlx::Error> {
    let flow_cutoff = now - flow_ttl_chrono();

    let flows_deleted = sqlx::query!(
        r#"DELETE FROM auth_flows WHERE created_at < $1"#,
        flow_cutoff,
    )
    .execute(pool)
    .await?
    .rows_affected();

    // `<=` matches `load_session`'s expiry check exactly: a row whose
    // `expires_at` equals `now` on the nose is expired by both paths, not
    // just skipped here and caught a tick later.
    let sessions_deleted = sqlx::query!(r#"DELETE FROM auth_sessions WHERE expires_at <= $1"#, now)
        .execute(pool)
        .await?
        .rows_affected();

    Ok((flows_deleted, sessions_deleted))
}

/// Spawn the session-store sweep task over cloned `pool`/`clock` handles.
/// No first-iter unconditional run — mirrors `retention::spawn_outbox_sweep`'s
/// quiet-start rationale, there's no urgency to sweep at startup.
fn spawn_sweep(
    pool: TracedPool,
    clock: Arc<dyn Clock>,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                () = tokio::time::sleep(SWEEP_INTERVAL) => {}
            }

            let now = clock.now();
            if let Err(e) = sweep_expired_impl(&pool, now).await {
                tracing::warn!(error.message = %e, "session sweep tick failed");
            }
        }
    })
}

/// [`FLOW_TTL`] as a `chrono::Duration`, for arithmetic against `DateTime<Utc>`.
fn flow_ttl_chrono() -> chrono::Duration {
    chrono::Duration::from_std(FLOW_TTL).expect("FLOW_TTL fits chrono::Duration")
}

/// Generate a token: [`TOKEN_BYTES`] of OS randomness, base64url, no padding.
fn random_token() -> Result<String, sqlx::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| sqlx::Error::Io(std::io::Error::other(e)))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// SHA-256 hex digest of a raw session id. One-way: the raw value is only
/// ever the input, never derivable from the stored hash.
fn sha256_hex(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    const_hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_token_is_url_safe_and_unpadded() {
        let token = random_token().expect("random_token should succeed");
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "token should be URL-safe base64, got: {token}"
        );
        assert!(!token.contains('='), "token should have no padding");
        // 32 bytes base64url-encodes to 43 chars (ceil(32*4/3) with no padding).
        assert_eq!(token.len(), 43);
    }

    #[test]
    fn random_token_is_not_repeated() {
        let a = random_token().expect("random_token should succeed");
        let b = random_token().expect("random_token should succeed");
        assert_ne!(a, b, "two calls should not produce the same token");
    }

    #[test]
    fn sha256_hex_is_deterministic_and_one_way_looking() {
        let raw = "some-raw-session-id";
        let hash_a = sha256_hex(raw);
        let hash_b = sha256_hex(raw);
        assert_eq!(hash_a, hash_b, "hashing the same input twice must agree");
        assert_ne!(hash_a, raw, "the hash must not equal the raw input");
        assert_eq!(hash_a.len(), 64, "sha256 hex digest is 64 chars");
    }
}
