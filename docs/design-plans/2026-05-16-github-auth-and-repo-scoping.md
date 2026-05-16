# GitHub Authentication and Per-User Repository Scoping

## Context

ATC today is fully open: every HTTP route and WebSocket connection serves
the entire backend state to anyone who can reach the server. Webhook
ingestion is authenticated (HMAC-SHA256 with a shared
`ATC_GITHUB__WEBHOOK_SECRET`,
`backend/crates/atc-github/src/webhook/verify.rs:65-88`), but every read
path returns everything the server can see
(`backend/crates/atc-server/src/routes.rs:89`,
`backend/crates/atc-server/src/ws.rs:50-114`). Production deployment is
multi-replica against Postgres (chart guard at `replicaCount > 1`); the
in-memory storage mode exists only as a dev convenience.

Relevant architecture this work builds on:

- `PersistentStore` in `backend/crates/atc-persist/src/lib.rs:79` is
  the persistence boundary. Its read surface today is `read_snapshot()`
  only.
- Storage is split across `atc-store-mem` (dev), `atc-store-pg`
  (production), and `atc-wire` (serializable wire types).
- The PG backend uses a transactional outbox + LISTEN/NOTIFY drain
  (`backend/crates/atc-store-pg/src/listener.rs:75,183`) as its
  cross-replica event transport. The listener honors
  `ATC_DATABASE_LISTENER_URL` so operators can route NOTIFY traffic
  past PgBouncer in transaction-pooling mode. **`BIGSERIAL` is
  explicitly not used as a commit-order cursor**
  (`backend/crates/atc-store-pg/src/listener.rs:298`); the drain uses
  an atomic backstop + dedup ring buffer instead. Auth-revocation
  catchup follows the same pattern.
- OpenTelemetry, supervision (`TaskTracker`), and cooperative shutdown
  (`CancellationToken`) are first-class.
- The frontend has no router; it's a pure SPA with rune-class
  singleton stores and a single `ConnectionManager`
  (`frontend/src/lib/connection.ts:17`).

This work needs to:

1. Add a user-identity surface (login/logout, OAuth flow, session storage).
2. Add a repo-scoped read path on `PersistentStore`.
3. Add a per-user authorization layer above the storage boundary.
4. Make all of the above safe across multiple replicas, behaving
   consistently for the same `(user, repo)` pair regardless of which
   replica answers.
5. Preserve today's zero-config in-memory + no-auth dev workflow
   without silently turning into the same posture in production
   deployments where the operator actually wanted auth on.

---

## Definition of Done

1. **Operator-supplied GitHub App OAuth identity loads from environment.**
   Two env vars: `ATC_GITHUB__APP__CLIENT_ID` and `ATC_GITHUB__APP__CLIENT_SECRET`.
   New env var: `ATC_AUTH__REQUIRED` (boolean, **default false**) — the
   master switch.
   Validation rule:
   - `auth_required=true`: both creds must be set. Missing one or both
     → **fatal startup error** with descriptive message listing which
     cred(s) are missing.
   - `auth_required=false` (or unset): auth is disabled regardless of
     whether creds are set. Startup proceeds; "AUTH DISABLED — dev only"
     logged. If creds are set, an INFO line notes that they are being
     ignored.
2. **Users can log in via GitHub (App's user OAuth with PKCE) and log out**,
   with sessions surviving server restart in PG mode.
3. **Authenticated `GET /v1/state` and `/v1/ws` return only data for repos
   visible to the user** — the intersection of (repos sending webhooks to
   ATC) and (repos the user can see within ATC's App installations).
4. **Access cache freshness is bounded at 60s** for any authenticated reader.
   Cache refreshed (a) inline on `GET /v1/state` when stale, AND (b) on a
   60s tick for every user with at least one active WS connection on any
   replica. The 60s bound holds regardless of whether the user has a WS
   connection.
5. **Mid-session revocation propagates within ~60s.** When the refresh
   detects a previously-accessible repo is no longer visible, all open WS
   connections for the user (across all replicas) are closed; client
   reconnects, re-fetches scoped snapshot, gets the new set.
6. **All replicas reach the same authz decision** for the same `(user, repo)`
   tuple.
7. **Frontend ships:** login button, OAuth callback (server-handled, returns
   to `/`), logout (sign out everywhere), "not logged in" gate, "no repos
   accessible" empty state, `authStore`, auth-aware `ConnectionManager`.
8. **Dev mode (auth disabled, in-memory storage) keeps working:** `just dev`
   + curl/smee.io flow unchanged, no login UI shown.
9. **Webhook ingestion path is untouched** — HMAC + shared secret,
   server-wide, repo-blind.
10. **Misconfigured production fails fast at startup, not silently.** When
    an operator sets `ATC_AUTH__REQUIRED=true` and one or both creds are
    missing, the server exits non-zero with a descriptive error. The Helm
    chart wires `ATC_AUTH__REQUIRED={{ .Values.auth.required }}` from
    values.yaml so operators choose explicitly. **No chart-time render
    guard** — operators behind a network proxy may legitimately set
    `auth.required: false`, and the chart must not block that.
11. **Multi-session per user supported:** browser A and browser B can both
    be logged in concurrently with independent per-session tokens stored in
    PG. Logout deletes **all** of the user's sessions (sign out everywhere).
    When GitHub-initiated revocation invalidates a session's refresh token
    (`invalid_grant`), only that one session is invalidated; other sessions
    remain valid as long as their refresh tokens are usable.
12. **Concurrent refresh is single-flighted.** Only one replica at a time
    performs the GitHub API portion of `refresh_user(user_id)`; other
    replicas attempting to refresh in the same window either skip (per-tick)
    or wait briefly (inline path). No spurious `invalid_grant` from
    concurrent refresh-token rotation.
13. **Integration tests cover:** auth flow happy path (with PKCE), callback
    CSRF, callback precondition failures (missing/malformed state cookie,
    direct hit), session lifecycle (multi-session, logout-everywhere),
    scoped snapshot, WS upgrade auth, WS event filtering, WS revocation on
    cache refresh, multi-replica consistency, dev-mode bypass, fail-fast
    startup when `auth_required=true` and creds missing, GitHub API
    pagination, token-expiry + session-invalidation flow, advisory-lock
    single-flight under concurrent refresh.

---

## Locked Decisions

| # | Decision |
|---|----------|
| L1 | Auth-revocation fanout reuses the existing Postgres outbox + LISTEN/NOTIFY transport, **with the same atomic backstop + dedup pattern as the outbox drain** (`backend/crates/atc-store-pg/src/listener.rs:298`); `BIGSERIAL > cursor` is NOT used as a commit-order cursor. |
| L2 | Webhook ingestion stays unauthenticated (HMAC-verified, not user-scoped). |
| L3 | GitHub App user-to-server tokens (with PKCE). Only `CLIENT_ID` + `CLIENT_SECRET` needed — no `APP__ID` or `APP__PRIVATE_KEY` because no endpoint uses App-level JWTs. |
| L4 | Frontend ships in this work; no repo picker (filtering is automatic). |
| L5 | **`ATC_AUTH__REQUIRED` is the master switch.** Default `false` (auth disabled regardless of cred presence; bare-binary dev workflows zero-config). When `true`, both creds must be set or startup fails fatally. |
| L6 | Access cache: refreshed inline on `/v1/state` when stale (>60s); refreshed every 60s while user has active WS connection. |
| L7 | `read_snapshot_for_repos(&[RepoKey])` method on `PersistentStore`; both impls update; PG gets a new `(org, repo)` index on `runs`. |
| L8 | New `SessionStore` trait in `atc-persist`, paired impls in `atc-store-pg` and `atc-store-mem`. PG impl reuses the existing pool **and owns the `atc_auth_invalidation` PG NOTIFY listener** (so it honors `ATC_DATABASE_LISTENER_URL`). |
| L9 | Per-request PG read for auth state (no replica cache). |
| L10 | Per-user refresh task; **distributed single-flight via PG advisory lock** keyed by `user_id` for the entire `refresh_user` body (GitHub API calls AND the cache UPDATE). Tasks that fail to acquire the lock skip the tick. Revocation propagates via NOTIFY on `atc_auth_invalidation`; payload is the `auth_revocation_log` `seq`. |
| L11 | Cookie at WS upgrade (`HttpOnly; Secure; SameSite=Lax`). Same-origin SPA + WS. Cross-origin out of scope. |
| L12 | Auth-disabled dev mode = implicit synthetic dev user with `AccessScope::All`; log "AUTH DISABLED — dev only" at startup. **Production safety lives at the server**: `ATC_AUTH__REQUIRED=true` + missing creds → fatal startup error. The Helm chart does NOT block `auth.required: false`. |
| L13 | **Multi-session per user supported; logout = "sign out everywhere"** (delete all of the user's sessions). Per-session tokens. A session row is deleted only when its refresh token is rejected with `invalid_grant` or when logout-everywhere fires for the user — tokens that simply age out without being used are NOT treated as session invalidation, because ATC's API calls use whichever session's token is currently freshest. |
| L14 | **PKCE required** on the OAuth login. State cookie carries `{state, code_verifier}` JSON. Missing/malformed state cookie or direct callback hit → **302 redirect to `/v1/auth/github/login`** (re-initiate the flow). |
| L15 | **Catchup heartbeat retention: 1 hour.** `auth_revocation_log` rows older than 1h are swept; listener catchup heartbeat uses a bounded-window rescan with a ring-buffer dedup (no `seq > cursor` arithmetic). |
| L16 | **Refresh tasks joined on shutdown via `TaskTracker`** owned by `RefreshSupervisor`. Per-task budget folded into the existing shutdown orchestration. |

---

## Architecture

### Module map

```
backend/crates/atc-persist/
  src/
    lib.rs              # PersistentStore trait (gains read_snapshot_for_repos)
    session.rs          # SessionStore trait + Session, AccessCacheRow,
                        # RevocationNotice, RevocationScope, AccessScope,
                        # RefreshLockGuard, UserId, SessionId types

backend/crates/atc-store-pg/
  migrations/
    0005_sessions.sql            # new
    0006_user_access_cache.sql   # new
    0007_runs_org_repo_idx.sql   # new
    0008_auth_revocation_log.sql # new
  src/
    reads.rs            # add read_snapshot_for_repos (EXISTING FILE)
    session/
      mod.rs            # PgSessionStore (impls SessionStore)
                        # owns the atc_auth_invalidation PgListener task
                        # honors ATC_DATABASE_LISTENER_URL
      access_cache.rs   # user_access_cache reads + optimistic UPDATE
      listener.rs       # PG NOTIFY listener + bounded-window catchup
      advisory_lock.rs  # pg_try_advisory_xact_lock helper

backend/crates/atc-store-mem/
  src/
    session.rs          # InMemorySessionStore (impls SessionStore)
                        # broadcast::Sender for in-process revocations
                        # in-process Mutex for the "advisory lock"
    lib.rs              # add read_snapshot_for_repos impl

backend/crates/atc-wire/
  src/
    user.rs             # User wire type (id, login, name, avatar_url)
    snapshot.rs         # add accessible_repos_count: u64 to StateSnapshot
                        # (#[serde(default)] for rolling-deploy tolerance)

backend/crates/atc-github/
  src/
    oauth/
      mod.rs            # OAuth code exchange, token refresh, PKCE helpers
      user.rs           # GET /user
      installations.rs  # paginated /user/installations,
                        # paginated /user/installations/{id}/repositories
      errors.rs         # OAuthError (InvalidGrant, RefreshExpired,
                        # Unauthenticated, RateLimited, Other variants)

backend/crates/atc-server/
  src/
    auth/
      mod.rs            # AuthService (orchestrates OAuth + SessionStore)
      middleware.rs     # extract session from cookie, attach to request
      routes.rs         # /v1/auth/{login, callback, logout, me}
      refresh_task.rs   # per-user refresh tick supervisor (TaskTracker)
      ws_registry.rs    # per-replica (user_id, session_id) → CancellationToken
      revocation_consumer.rs  # subscribes to SessionStore::subscribe_revocations()
                              # and cancels matching tokens in WsRegistry
    config.rs           # add OAuthConfig + auth_required + validation
    routes.rs           # gate /v1/state on scoped reads + inline freshness;
                        # compose accessible_repos_count onto response
    ws.rs               # gate upgrade on session; per-event filter
    state.rs            # AppState gains auth: Arc<AuthService>,
                        # ws_registry: Arc<WsRegistry>

frontend/src/lib/
  stores/
    auth.svelte.ts      # new authStore rune class
  components/
    LoginScreen.svelte  # shown when !authenticated
    NoReposAccessible.svelte  # empty state when intersection is empty
    TopBar.svelte       # add user avatar + logout button (EXISTING FILE)
  connection.ts         # add 401-on-/v1/state handler to trigger
                        # authStore.init() (EXISTING FILE)
  api/
    auth.ts             # /v1/auth/me wrapper
```

### Data model

#### `sessions` table

```sql
CREATE TABLE sessions (
    id                UUID        PRIMARY KEY,
    user_id           BIGINT      NOT NULL,
    user_login        TEXT        NOT NULL,
    user_name         TEXT,
    user_avatar_url   TEXT,
    access_token      TEXT        NOT NULL,    -- per-session, plaintext (initial)
    refresh_token     TEXT        NOT NULL,    -- per-session
    token_expires_at  TIMESTAMPTZ NOT NULL,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_active_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX sessions_user_id_idx ON sessions(user_id);
CREATE INDEX sessions_token_expires_at_idx ON sessions(token_expires_at);
```

Each successful login inserts a new row. Multiple rows per `user_id`
allowed. Logout deletes **all rows for that user_id**.

#### `user_access_cache` table

```sql
CREATE TABLE user_access_cache (
    user_id            BIGINT      PRIMARY KEY,
    accessible_repos   JSONB       NOT NULL,
    refreshed_at       TIMESTAMPTZ NOT NULL
);
```

#### `auth_revocation_log` table

```sql
CREATE TABLE auth_revocation_log (
    seq          BIGSERIAL   PRIMARY KEY,
    user_id      BIGINT      NOT NULL,
    scope        TEXT        NOT NULL CHECK (scope IN ('all', 'session')),
    session_id   UUID,
    notified_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX auth_revocation_log_notified_at_idx ON auth_revocation_log(notified_at);
```

Retention: rows older than **1 hour** are deleted by a periodic sweep
task (hourly).

#### `runs(org, repo)` index

```sql
CREATE INDEX runs_org_repo_idx ON runs(org, repo);
```

### Trait surface — `atc-persist`

```rust
#[async_trait]
pub trait PersistentStore: Send + Sync + 'static {
    // ... existing methods
    async fn read_snapshot_for_repos(
        &self,
        repos: &[RepoKey],
    ) -> Result<StateSnapshot, PersistError>;
}

pub enum AccessScope {
    /// Synthetic dev user in auth-disabled mode; no filtering.
    All,
    /// Authenticated user; filter by these repos.
    Scoped(Vec<RepoKey>),
}

#[async_trait]
pub trait SessionStore: Send + Sync + 'static {
    // Session CRUD
    async fn create_session(&self, new: NewSession) -> Result<Session, SessionError>;
    async fn get_session(&self, id: SessionId) -> Result<Option<Session>, SessionError>;
    async fn touch_session(&self, id: SessionId) -> Result<(), SessionError>;
    async fn update_session_tokens(
        &self,
        id: SessionId,
        access_token: &str,
        refresh_token: &str,
        token_expires_at: DateTime<Utc>,
    ) -> Result<(), SessionError>;

    // Session deletion
    async fn delete_session(&self, id: SessionId) -> Result<(), SessionError>;
    async fn delete_user_sessions(&self, user_id: UserId)
        -> Result<Vec<SessionId>, SessionError>;
    async fn list_user_sessions(&self, user_id: UserId)
        -> Result<Vec<Session>, SessionError>;

    // Access cache
    async fn read_access_cache(
        &self,
        user_id: UserId,
    ) -> Result<Option<AccessCacheRow>, SessionError>;
    async fn refresh_access_cache(
        &self,
        user_id: UserId,
        new_accessible_repos: Vec<RepoKey>,
        ttl: Duration,
    ) -> Result<Option<(Vec<RepoKey>, Vec<RepoKey>)>, SessionError>;
    async fn clear_access_cache(&self, user_id: UserId) -> Result<(), SessionError>;

    // Distributed single-flight for refresh_user
    /// Acquire an advisory lock keyed by user_id for the lifetime of the
    /// returned guard. PG impl uses `pg_try_advisory_xact_lock`; in-memory
    /// uses a `tokio::sync::Mutex<HashSet<UserId>>`. Returns `None` if the
    /// lock is held by another caller (replica or task).
    async fn try_acquire_refresh_lock(
        &self,
        user_id: UserId,
    ) -> Result<Option<RefreshLockGuard>, SessionError>;

    // Revocation fanout
    fn subscribe_revocations(&self) -> broadcast::Receiver<RevocationNotice>;
    async fn broadcast_revocation(
        &self,
        notice: RevocationNotice,
    ) -> Result<(), SessionError>;
}

pub struct RevocationNotice {
    pub user_id: UserId,
    pub scope: RevocationScope,
}

pub enum RevocationScope {
    AllSessions,
    Session(SessionId),
}
```

### OAuth flow (with PKCE)

```
1. GET /v1/auth/github/login
   - Server generates:
     - state: 32-byte random hex
     - code_verifier: 64-byte random base64url
     - code_challenge: SHA256(code_verifier), base64url
   - Server sets short-lived (10min) cookie containing JSON {state, code_verifier}
     (HttpOnly, Secure, SameSite=Lax, Path=/v1/auth)
   - Server 302-redirects to:
     https://github.com/login/oauth/authorize?
       client_id=<CLIENT_ID>&
       redirect_uri=<server-origin>/v1/auth/github/callback&
       state=<state>&
       code_challenge=<code_challenge>&
       code_challenge_method=S256

2. GitHub 302-redirects to:
   GET /v1/auth/github/callback?code=<code>&state=<state>

   Callback-precondition failures (per L14):
   - State cookie missing → 302 redirect to /v1/auth/github/login
   - State cookie malformed (JSON parse failure, missing fields) → 302
   - State value mismatch between cookie and query → 302
   - Direct callback hit (no code in query) → 302
   (Logged at INFO with the specific precondition that failed. Re-initiating
   the flow regenerates the state cookie + code_verifier.)

   Happy path:
   - Server POSTs to https://github.com/login/oauth/access_token with:
       client_id, client_secret, code, code_verifier (from state cookie)
   - Receives: access_token, refresh_token, expires_in
   - Server GET https://api.github.com/user with Bearer access_token
     → {id, login, name, avatar_url}
   - Server fetches user's installations + per-installation repos (paginated)
   - In a single PG transaction:
     - INSERT INTO sessions (id, user_id, ..., access_token, refresh_token,
       token_expires_at)
     - INSERT INTO user_access_cache (user_id, accessible_repos,
       refreshed_at) ON CONFLICT (user_id) DO UPDATE
   - Server sets SID cookie (id from above; HttpOnly, Secure, SameSite=Lax,
     Path=/), clears the state cookie
   - Server 302-redirects to "/"

3. Per-request auth (middleware)
   - Read SID cookie → session_store.get_session(SID)
   - If valid: insert AuthenticatedUser into request extensions; fire-and-forget
     touch_session (telemetry only)
   - If invalid: 401

4. POST /v1/auth/logout
   - Read SID cookie → look up session → user_id
   - session_store.delete_user_sessions(user_id) → returns Vec<SessionId>
   - broadcast_revocation(RevocationNotice {user_id, scope: AllSessions})
   - Clear SID cookie (Set-Cookie with Max-Age=0)
   - Returns 204
```

### GitHub API pagination

Both `/user/installations` and `/user/installations/{id}/repositories`
paginate. The OAuth client must:
- Request `per_page=100` on the initial call.
- Follow the `Link: <...>; rel="next"` header until absent.
- Aggregate all pages.

Tests cover multi-page fixtures (e.g., 250 installations × 250
repositories).

### Token lifecycle

GitHub Apps must be configured with **expiring user tokens enabled** (a
documented prerequisite in `docs/architecture/deployment.md`). Access
tokens expire after 8h; refresh tokens expire after 6 months of non-use.

**Refresh trigger:** A session's access token is refreshed when:
- `token_expires_at < now() + 5min`, OR
- Any GitHub API call returns 401 (after the inline single-flight has
  attempted to refresh).

**Refresh failure handling (`invalid_grant`):**
- `delete_session(session_id)` + `broadcast_revocation(Session(session_id))`.
- If `list_user_sessions(user_id)` returns empty after that:
  `clear_access_cache(user_id)` + broadcast `AllSessions`.
- 401 from any user-info or installation-listing call with a freshly
  refreshed token → treat as `invalid_grant` per above.

**Per-session expiry semantics (L13 clarification):** A session row is
deleted only when its refresh token is rejected by GitHub
(`invalid_grant` from the refresh-token endpoint) or when
logout-everywhere fires. A session whose access token simply expires
without being used is NOT deleted — its `access_token` field becomes
useless, but the SID cookie continues to identify the user via
`get_session`, and ATC continues serving them using whichever of the
user's other sessions' tokens is currently freshest (refreshed via the
per-user refresh task). The aged-out session's refresh token remains
valid for 6 months; if it ever needs to be used (because all fresher
sessions are gone), the refresh task will refresh it then.

### Auth middleware

```rust
pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if state.auth.is_disabled() {
        request.extensions_mut().insert(AuthenticatedUser::dev_user());
        return Ok(next.run(request).await);
    }

    let session_id = parse_sid_cookie(&headers)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let session = state.auth.session_store.get_session(session_id)
        .await
        .map_err(internal_error)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let store = state.auth.session_store.clone();
    tokio::spawn(async move { let _ = store.touch_session(session_id).await; });

    request.extensions_mut().insert(AuthenticatedUser { session });
    Ok(next.run(request).await)
}
```

Applied to: `/v1/state`, `/v1/ws`, `/v1/auth/logout`, `/v1/auth/me`. Not
applied to: `/healthz`, `/readyz`, `/v1/webhooks/github`,
`/v1/auth/github/login`, `/v1/auth/github/callback`.

`/v1/auth/me` is special: in auth-disabled mode, the middleware
short-circuits with the dev user, so `/me` returns 200 with
`{user: dev_user, auth_disabled: true}`. In auth-enabled mode, no cookie
→ 401 (the frontend uses this 401 to render the login screen).

### REST `/v1/state` scoping with inline freshness check

```rust
async fn state_handler(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> impl IntoResponse {
    let snapshot = match user.access_scope() {
        AccessScope::All => state.persist.read_snapshot().await?,
        AccessScope::Scoped(_) => {
            state.auth.ensure_fresh_access_cache(user.user_id()).await?;
            let access_set = state.auth.load_accessible_repos(user.user_id()).await?;
            state.persist.read_snapshot_for_repos(&access_set).await?
        }
    };
    // ... existing runner_pool_capacities composition (added by the handler,
    //     not the store; matches the existing pattern at routes.rs:96)
    // ... NEW: snapshot.accessible_repos_count = access_set.len() as u64
    //     also composed at the handler (matches existing pattern)
    Json(snapshot)
}
```

`ensure_fresh_access_cache` reads the cache row; if `refreshed_at <
now() - 60s`, calls the per-user refresh path (which acquires the
advisory lock for single-flight). If another replica holds the lock, the
inline call **waits** (up to ~5s) for that replica to finish, then
re-reads the cache row.

### Per-user refresh task

**Rate-limit note.** This task is **per-user, not per-session**. Multiple
concurrent browser sessions for the same user share a single refresh task.
For a user with N installations, each tick is approximately `1 + N` calls
(`/user/installations` + N per-installation `/repositories`), plus an
occasional token refresh. ~420 calls/hour per active user with N=5
installations — well under GitHub's 5000/hour user-token budget.

```rust
// One task per (user_id) on each replica that has at least one active WS
// for that user. NOT per-session. Spawned by RefreshSupervisor on first
// WS upgrade; aborted on last disconnect for the user.
async fn refresh_task_loop(
    user_id: UserId,
    auth: Arc<AuthService>,
    shutdown: CancellationToken,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(60));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                if let Err(e) = auth.refresh_user(user_id).await {
                    tracing::warn!(?e, %user_id, "refresh tick failed");
                }
            }
        }
    }
}
```

`auth.refresh_user(user_id)` (called by both the refresh task and the
inline freshness check):

1. **Single-flight lock:** `session_store.try_acquire_refresh_lock(user_id)`.
   If `None`, another replica/task is in flight — log at `debug`, return
   `Ok(Skipped)`. (The inline path may wait briefly and re-read the
   cache instead of returning empty.)
2. `list_user_sessions(user_id)` → pick the session with the highest
   `token_expires_at`.
3. If chosen session's token expires within 5min, refresh via POST
   `/login/oauth/access_token` (refresh-token grant). On `invalid_grant`:
   `delete_session(session_id)` + `broadcast_revocation(Session(session_id))`,
   loop to next session. If no sessions remain: `clear_access_cache` +
   `broadcast_revocation(AllSessions)`, return.
4. Fetch `/user/installations` (paginated) + `/user/installations/{id}/repositories`
   (paginated for each). On 401 with a fresh token: treat as `invalid_grant`
   per step 3.
5. Compute new accessible_repos set.
6. `refresh_access_cache(user_id, new_set, 60s)`. If returns
   `Some((prior, new))`: compute `revoked = prior - new`; if non-empty,
   broadcast `RevocationNotice { user_id, scope: AllSessions }`.
7. Release the advisory lock (automatic via guard drop).

**Why single-flight covers the entire body**: protects against two
replicas independently rotating the same session's refresh token (the
second one would see `invalid_grant` and wrongly delete a healthy
session), and avoids `N`× wasted GitHub API calls under multi-replica
deployments. Lock hold time is small (typically <500ms for users with a
handful of installations).

**Shutdown:** `RefreshSupervisor` owns a `TaskTracker` for all spawned
refresh tasks. `run_shutdown_orchestration` calls
`refresh_supervisor.shutdown()` which `close()`s and `wait()`s the
tracker with a 2s budget (matching existing per-task patterns). Tasks
that are mid-tick observe `shutdown.cancelled()` between API calls and
exit cleanly.

### WS auth and revocation

```rust
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<impl IntoResponse, StatusCode> {
    state.auth.ensure_fresh_access_cache(user.user_id()).await
        .map_err(internal_error)?;

    let access_set: Arc<HashSet<RepoKey>> = Arc::new(
        state.auth.load_accessible_repos(user.user_id()).await
            .map_err(internal_error)?
    );

    let conn_cancel = CancellationToken::new();
    state.ws_registry.register(user.user_id(), user.session_id(),
        conn_cancel.clone());
    state.auth.refresh_supervisor.register(user.user_id());

    let rx = state.persist.subscribe();
    let shutdown = state.shutdown.clone();

    Ok(ws.on_upgrade(move |socket| {
        state.ws_tracker.track_future(handle_socket(
            socket, rx, shutdown, conn_cancel, access_set, user, state,
        ))
    }))
}
```

`WsRegistry`:

```rust
pub struct WsRegistry {
    by_user: RwLock<HashMap<UserId, HashMap<SessionId, Vec<CancellationToken>>>>,
}

impl WsRegistry {
    pub fn register(&self, user: UserId, session: SessionId,
        cancel: CancellationToken);
    pub fn unregister(&self, user: UserId, session: SessionId,
        cancel: &CancellationToken);
    pub fn cancel_user(&self, user: UserId);
    pub fn cancel_session(&self, user: UserId, session: SessionId);
}
```

The `RevocationConsumer` task subscribes to
`session_store.subscribe_revocations()` and routes notices:
- `RevocationScope::AllSessions` → `registry.cancel_user(user_id)`
- `RevocationScope::Session(sid)` → `registry.cancel_session(user_id, sid)`

The PG NOTIFY listener is **inside `PgSessionStore`** (honors
`ATC_DATABASE_LISTENER_URL`); the in-memory session store fires the
broadcast directly. `atc-server` consumes the same `broadcast::Receiver`
regardless of transport.

### Multi-replica consistency

Per L9, replicas read auth state from PG per request (no replica
cache). Every authenticated request reads the session row + access
cache row from PG. The authz decision is therefore identical across
replicas at any moment.

Revocation propagates via NOTIFY on `atc_auth_invalidation`. Following
L1 and the outbox listener pattern at
`backend/crates/atc-store-pg/src/listener.rs:298`, the catchup heartbeat
uses a **bounded-window rescan with dedup**, not a `BIGSERIAL > cursor`
arithmetic.

### Catchup heartbeat for revocations

`PgSessionStore`'s listener task:
- Subscribes to PG NOTIFY on `atc_auth_invalidation`. Payload is the
  `auth_revocation_log.seq` value.
- On NOTIFY: SELECT the row by seq, broadcast `RevocationNotice`,
  insert seq into a 1024-entry dedup ring buffer.
- On 10s heartbeat tick (or wakeup signal from the listener):
  `SELECT seq, user_id, scope, session_id FROM auth_revocation_log WHERE
  notified_at > now() - INTERVAL '1 hour' ORDER BY seq` → broadcast any
  seqs not already in the ring buffer. The 1-hour window provides
  resilience against listener outages of up to 1h.
- A separate hourly sweep deletes rows older than 1h.

Revocation writes (from `refresh_user` and from `logout`) INSERT into
`auth_revocation_log` AND `pg_notify('atc_auth_invalidation', seq::text)`
inside the same transaction. Aborted transactions silently drop both
(consistent with the outbox pattern).

### Frontend

```typescript
export class AuthStore {
  user = $state<User | null>(null);
  authDisabled = $state<boolean>(false);
  accessibleRepoCount = $state<number>(0);
  loading = $state<boolean>(true);

  async init() {
    this.loading = true;
    try {
      const res = await fetch('/v1/auth/me', { credentials: 'same-origin' });
      if (res.status === 401) {
        this.user = null;
      } else if (res.status === 200) {
        const data = await res.json();
        this.user = data.user;
        this.authDisabled = data.auth_disabled === true;
      }
    } finally {
      this.loading = false;
    }
  }

  get authenticated(): boolean { return this.user !== null; }
  get loginUrl(): string { return '/v1/auth/github/login'; }
  async logout() {
    await fetch('/v1/auth/logout', { method: 'POST', credentials: 'same-origin' });
    this.user = null;
    location.href = '/';
  }
}

export const authStore = new AuthStore();
```

`App.svelte`:

```svelte
{#if authStore.loading}
  <LoadingScreen />
{:else if !authStore.authenticated && !authStore.authDisabled}
  <LoginScreen />
{:else}
  <ConnectionManager />
  <AriaLiveRegion />
  <RovingFocusProvider>
    <AppShell>
      {#if runStore.totalRuns === 0 && authStore.accessibleRepoCount === 0
            && !authStore.authDisabled}
        <NoReposAccessible />
      {:else}
        <KanbanBoard />
      {/if}
    </AppShell>
    <CommandPalette />
    <RunDetailPanel />
  </RovingFocusProvider>
{/if}
```

`ConnectionManager` change (one place): on 401 from the snapshot fetch
in the existing reconnect path, call `authStore.init()` (which will
transition the store to `!authenticated` and trigger the `LoginScreen`
render).

`StateSnapshot.accessible_repos_count: u64` is composed by
`routes::state_handler` after the snapshot read (mirroring the existing
`runner_pool_capacities` composition pattern at `routes.rs:96`); the
store does not compose it. The field is `#[serde(default)]` for
rolling-deploy tolerance.

### Spans + metrics

Spans:
- `auth.login` (root, in `/v1/auth/github/callback`) — attribute
  `outcome` (success | callback_precondition_failed | code_exchange_failed
   | github_user_fetch_failed)
- `auth.refresh.user` (root, in refresh task or inline) — attribute
  `outcome` (refreshed | skipped_locked | skipped_fresh | refresh_token_invalid
   | revocation_fired), `trigger` (tick | inline)
- `auth.notify.recv` (root, in `PgSessionStore` listener) — attribute
  `scope` (all | session), `source` (notify | catchup)

Counters (no high-cardinality labels):
- `atc_auth_logins_total{outcome}`
- `atc_auth_refreshes_total{outcome, trigger}`
- `atc_auth_revocations_total{scope}`
- `atc_github_api_calls_total{endpoint, outcome=ok|401|429|other_error}`
- `atc_auth_advisory_lock_contention_total`

Gauges (per-replica, unlabeled):
- `atc_auth_sessions_active`
- `atc_auth_ws_connections_active`

---

## Implementation Steps

Each step: failing tests first, then implementation, then refactor.

### Branch and plan commit

Create the feature branch (e.g., `gh-auth`). Copy
`~/.claude/plans/design-phase-11-github-curried-pony.md` to
`docs/design-plans/2026-05-16-github-auth-and-repo-scoping.md`. Commit
and push (no PR yet — implementation lands on this branch and a single
PR opens at the end).

### Config scaffolding

- Failing tests for `Config` loading:
  - `auth_required=true` + both creds set → `Some`, auth enabled.
  - `auth_required=true` + both creds absent → fatal startup error.
  - `auth_required=true` + one cred set, one absent → fatal startup
    error identifying the missing cred.
  - `auth_required=false` (default) + both creds absent → `None`, auth
    disabled, "AUTH DISABLED — dev only" logged.
  - `auth_required=false` + both creds set → `None`, auth disabled
    (master switch wins); INFO log notes creds ignored.
  - `auth_required=false` + one cred set, one absent → `None`, auth
    disabled.
- Add `pub oauth: Option<OAuthConfig>` to `Config.github`. Add
  `OAuthConfig { client_id, client_secret }`. Add `pub auth_required:
  bool` to `Config` (env: `ATC_AUTH__REQUIRED`, **default false**).
- `Config::load` implements the matrix above. `auth_required` is the
  master switch: when `false`, `oauth` is `None` regardless of what the
  cred env vars contain.
- Helm chart wiring (no render guard — see L12):
  - Add `auth.required: true` default to `deploy/helm/atc/values.yaml`
    (safe default; operators behind proxies can override to `false`).
  - Add `github.app.clientId: ""` and `github.app.clientSecretRef: ""`
    defaults.
  - Pod env wires `ATC_AUTH__REQUIRED={{ .Values.auth.required }}`,
    `ATC_GITHUB__APP__CLIENT_ID`, `ATC_GITHUB__APP__CLIENT_SECRET` from
    the secret ref.
  - Update `deploy/helm/atc/values.schema.json` with new fields.
  - Update `deploy/helm/atc/CLAUDE.md` and `README.md` with operator
    setup instructions.

### GitHub OAuth client in `atc-github`

- Failing tests against a `mockito` HTTP server:
  - OAuth code exchange happy path (with PKCE `code_verifier`).
  - OAuth code exchange `invalid_grant` → `OAuthError::InvalidGrant`.
  - Token refresh happy path (rotates refresh_token).
  - Token refresh `invalid_grant` → `OAuthError::RefreshExpired`.
  - GET `/user` happy path; 401 → `OAuthError::Unauthenticated`.
  - GET `/user/installations` single-page + multi-page (3 pages × 100).
  - GET `/user/installations/{id}/repositories` single + multi-page.
  - PKCE helper: `generate_pkce_pair() -> (verifier, challenge)`.
- New module `atc-github::oauth`. Add `reqwest` crate dep
  (`cargo add reqwest --no-default-features --features rustls-tls`).

### `SessionStore` trait + paired impls

- Failing tests for both impls:
  - All CRUD methods.
  - `refresh_access_cache` race (two concurrent attempts → only one wins).
  - `try_acquire_refresh_lock`: concurrent acquires → only one succeeds;
    second call returns `None`; releases on guard drop.
  - `subscribe_revocations` + `broadcast_revocation` round-trip.
  - `auth_revocation_log`: NOTIFY happy path; simulated dropped NOTIFY
    → catchup heartbeat re-emits within 10s; ring-buffer dedup avoids
    duplicate broadcasts.
  - Sweep task: rows older than 1h are deleted; rows within 1h are
    retained.
- Define `SessionStore`, `Session`, `AccessCacheRow`, `RevocationNotice`,
  `RevocationScope`, `AccessScope`, `RefreshLockGuard`, `UserId`,
  `SessionId` in `atc-persist::session`.
- Add migrations 0005, 0006, 0008 to `atc-store-pg/migrations/`.
- Implement `PgSessionStore` in `atc-store-pg::session::mod`:
  - Reuses the existing `PgPool`.
  - Owns the `atc_auth_invalidation` listener task using
    `PgListener::connect_with(&listener_pool)` (honors
    `ATC_DATABASE_LISTENER_URL`).
  - Listener: NOTIFY → SELECT by seq → broadcast (dedup ring buffer);
    10s heartbeat → bounded-window rescan (`WHERE notified_at > now() -
    INTERVAL '1 hour'`).
  - Sweep task (hourly) deletes `auth_revocation_log` rows older than 1h.
  - `try_acquire_refresh_lock` uses
    `pg_try_advisory_xact_lock(hashtext('atc.refresh.' || $user_id))`
    inside a transaction held by the `RefreshLockGuard`; guard drop
    rolls back the noop transaction, releasing the lock.
- Implement `InMemorySessionStore` in `atc-store-mem::session`:
  - `Mutex<HashMap<SessionId, Session>>` for sessions.
  - `Mutex<HashMap<UserId, AccessCacheRow>>` for access cache.
  - `broadcast::Sender<RevocationNotice>` for in-process fanout.
  - `Mutex<HashSet<UserId>>` for the "advisory lock" set;
    `try_acquire_refresh_lock` inserts + returns a guard that removes
    on drop.
- Run `cargo sqlx prepare --workspace`.

### OAuth routes + auth middleware (with PKCE)

- Failing route tests (`tower::ServiceExt::oneshot`):
  - `/v1/auth/github/login` → 302 with `code_challenge`,
    `code_challenge_method=S256`, `state`, `client_id`. State cookie
    set with `{state, code_verifier}` JSON, `Path=/v1/auth`, 10min
    Max-Age.
  - `/v1/auth/github/callback` happy path: state matches, code exchange
    succeeds (mock GitHub), session + cache row created in one txn,
    SID cookie set, redirect to `/`.
  - Callback precondition failures (per L14):
    - Missing state cookie → 302 to `/v1/auth/github/login`.
    - Malformed state cookie → 302.
    - State mismatch → 302.
    - Direct hit with no `code` → 302.
  - Code exchange returns error → 502 with sanitized body.
  - `/v1/auth/logout` happy path: `delete_user_sessions(user_id)` called,
    `AllSessions` revocation broadcast, cookie cleared, returns 204.
  - `/v1/auth/me` (no cookie, auth enabled) → 401.
  - `/v1/auth/me` (no cookie, auth disabled) → 200 with
    `{user: dev_user, auth_disabled: true}`.
  - `/v1/auth/me` (valid cookie) → 200 with `{user, auth_disabled: false}`.
  - Protected route with no cookie (auth enabled) → 401.
- Implement handlers in `atc-server::auth::routes`; implement
  `auth_middleware`. Register on the Axum router. Construct
  `AuthService` in `main.rs` only when OAuth config is `Some`; in
  auth-disabled mode, use the disabled variant.

### `read_snapshot_for_repos` on `PersistentStore`

- Failing tests for both impls: empty repo set → empty snapshot; subset
  → filtered; non-existent repos → empty.
- Add `read_snapshot_for_repos` to `PersistentStore` trait.
- Add migration `0007_runs_org_repo_idx.sql`.
- Implement in `PgStore::read_snapshot_for_repos` (file is at
  `backend/crates/atc-store-pg/src/reads.rs`, not `src/store/reads.rs`):
  use parameter arrays with `unnest` for a stable query plan
  (`WHERE (org, repo) = ANY(...)`).
- Implement in `InMemoryStore::read_snapshot_for_repos`: HashSet filter.
- Run `cargo sqlx prepare --workspace`.

### REST scoping + inline freshness + WS upgrade gating

- Failing tests:
  - `/v1/state` authenticated with empty access set → empty snapshot,
    `accessible_repos_count: 0`.
  - `/v1/state` authenticated with subset → filtered snapshot.
  - `/v1/state` authenticated with stale cache (>60s) → inline refresh
    fires (one GitHub call observable via mock), then scoped snapshot
    returned.
  - `/v1/state` authenticated with stale cache + another replica holds
    refresh lock → inline path waits briefly, then re-reads cache.
  - `/v1/state` auth-disabled mode → full snapshot (regression).
  - `/v1/ws` upgrade with valid cookie + non-empty access set → upgrade
    succeeds; events for accessible repos delivered; events for
    inaccessible repos filtered.
  - `/v1/ws` upgrade without cookie (auth enabled) → 401.
  - `/v1/ws` upgrade without cookie (auth disabled) → upgrade succeeds.
  - `POST /v1/webhooks/github` with valid HMAC and no cookie + auth
    enabled → 200 `{status: "accepted"}` (behavioral regression for AC9).
- Modify `routes::state_handler` to dispatch to
  `read_snapshot_for_repos` with inline freshness check; compose
  `accessible_repos_count` onto the response.
- Modify `ws::ws_handler` to load access set at upgrade, pass to socket
  loop, filter per event.

### Refresh tick + revocation propagation + advisory lock + shutdown

- Failing tests (multi-replica integration tests using two `PgStore`
  instances against the same DB):
  - Refresh tick on Replica A → cache row updated, NOTIFY fires →
    Replica B observes new value on next read.
  - Refresh concurrency: both replicas attempt in the same window. Only
    one acquires the advisory lock; the other returns `skipped_locked`
    and does NOT call GitHub.
  - Concurrent refresh + GitHub `invalid_grant` → loser does NOT delete
    the session (because it never made the call, since the lock
    prevented it).
  - Revocation NOTIFY: refresh detects drop → NOTIFY fires +
    `auth_revocation_log` row written → both replicas' WsRegistry
    cancel matching connections.
  - Catchup: simulated dropped NOTIFY → 10s heartbeat tick → bounded
    rescan finds the row → broadcast emits the revocation.
  - Token-refresh `invalid_grant` → session row deleted, `Session(id)`
    revocation broadcast, that session's WS connection dropped.
  - All sessions expire → `clear_access_cache` + `AllSessions` broadcast.
  - Refresh task shutdown: `RefreshSupervisor.shutdown()` closes the
    `TaskTracker`; in-flight ticks observe `shutdown.cancelled()` and
    exit; tracker join completes within 2s.
  - WS handler observes `conn_cancel.cancelled()` → sends Close (code
    1000, "access revoked") → exits cleanly.
  - In-memory mode: same flows with single replica + in-process
    broadcast + in-process lock.
- Implement `RefreshSupervisor` (owns a `TaskTracker`),
  `refresh_task_loop`, `WsRegistry`, `RevocationConsumer`.
- Hook `RefreshSupervisor.register/unregister` into `ws_handler`.
- Extend `run_shutdown_orchestration` to join the
  `RevocationConsumer` task AND the `RefreshSupervisor`'s tracker.

### Frontend

- Failing tests (Vitest + Playwright):
  - `authStore.init()` happy path: `/v1/auth/me` returns user → store
    transitions to authenticated.
  - `authStore.init()` 401 → store stays unauthenticated → `LoginScreen`
    renders.
  - `authStore.init()` auth disabled → `authDisabled: true` → main app
    renders without login screen.
  - `LoginScreen` click → location.href = `/v1/auth/github/login`.
  - Logout button → POST `/v1/auth/logout` → redirect to `/`.
  - `NoReposAccessible` renders when authenticated +
    `accessible_repos_count` === 0 + runs === 0 + !authDisabled.
  - 401 from `/v1/state` mid-session → `authStore.init()` → fall back
    to `LoginScreen`.
- Add `authStore` rune class; `LoginScreen.svelte`,
  `NoReposAccessible.svelte`.
- Modify `App.svelte` for auth gate.
- Modify `TopBar.svelte` for avatar + logout.
- Modify `ConnectionManager` for 401 handling.
- Update `window.__stores` export.
- Update `e2e/lib/` helpers for cookie-based scenarios.

**Parallelism notes (per `docs/implementation-guidance.md` rule 14):**
- The OAuth client step and the SessionStore step touch disjoint
  crates and can run in parallel after Config scaffolding.
- The `read_snapshot_for_repos` step is independent of OAuth client,
  SessionStore, and OAuth routes work.
- The Frontend step can start in parallel with REST scoping once the
  `/v1/auth/me` contract is stable from OAuth routes.

---

## Acceptance Criteria

### AC1 — Config (DoD #1)

- **AC1.1 Success:** `auth_required=true` + both creds set → server
  starts, auth enabled.
- **AC1.2 Failure:** `auth_required=true` + both creds absent → fatal
  startup error.
- **AC1.3 Failure:** `auth_required=true` + one cred set, one absent →
  fatal startup error identifying the missing cred.
- **AC1.4 Success:** `auth_required=false` (default) + both creds absent
  → server starts, auth disabled, "AUTH DISABLED — dev only" logged.
- **AC1.5 Success:** `auth_required=false` + both creds set → server
  starts, auth disabled (master switch wins); INFO log notes creds are
  being ignored.
- **AC1.6 Success:** `auth_required=false` + partial creds → server
  starts, auth disabled.

### AC2 — Login / logout / callback failures (DoD #2 + #11 + #13)

- **AC2.1 Success:** `/v1/auth/github/login` returns 302 with
  `code_challenge`, `code_challenge_method=S256`, `state`, `client_id`.
  State cookie set with `{state, code_verifier}` JSON,
  `Path=/v1/auth`, 10min Max-Age.
- **AC2.2 Success:** Callback with matching state creates session row +
  access cache row in one transaction, sets SID cookie, 302-redirects
  to `/`. The `code_verifier` from the state cookie is posted to
  GitHub's token endpoint.
- **AC2.3 Success:** Restarting PG-mode server preserves sessions;
  subsequent `/v1/auth/me` with cookie still returns user.
- **AC2.4 Failure (callback precondition):** Missing state cookie →
  302 to `/v1/auth/github/login`.
- **AC2.5 Failure (callback precondition):** Malformed state cookie →
  302 to `/v1/auth/github/login`.
- **AC2.6 Failure (callback precondition):** State value mismatch →
  302 to `/v1/auth/github/login`.
- **AC2.7 Failure (callback precondition):** Direct hit with no `code`
  → 302 to `/v1/auth/github/login`.
- **AC2.8 Failure:** Code exchange returns error → 502 with sanitized
  body.
- **AC2.9 Success:** Logout deletes ALL sessions for the user (verified
  by `SELECT count(*) FROM sessions WHERE user_id = X` = 0), broadcasts
  `AllSessions`, clears SID cookie, returns 204.
- **AC2.10 Success:** Multi-session: log in twice (two cookies / two
  rows). Both work. Logout via either invalidates both (the other's
  next request returns 401).

### AC3 — Scoped reads (DoD #3)

- **AC3.1 Success:** `/v1/state` authenticated with access set
  `[{a, b}, {c, d}]` returns only runs/jobs in that set, plus
  `accessible_repos_count: 2`.
- **AC3.2 Success:** `/v1/state` authenticated with empty access set →
  empty runs/jobs + `accessible_repos_count: 0`.
- **AC3.3 Success:** `/v1/state` auth-disabled mode → full snapshot
  (regression).
- **AC3.4 Success:** WS connection authenticated with access set
  `[{a, b}]` receives only events for `(a, b)`.

### AC4 — Cache freshness (DoD #4)

- **AC4.1 Success:** After login, `user_access_cache` row exists with
  `refreshed_at` set to login time.
- **AC4.2 Success:** `/v1/state` with cache age 30s → inline check
  no-ops; zero GitHub calls observed.
- **AC4.3 Success:** `/v1/state` with cache age 90s → inline refresh
  fires; cache `refreshed_at` advances before snapshot returned.
- **AC4.4 Success:** Refresh tick at 60s + 1ms → UPDATE succeeds;
  follow-up tick at +30s no-ops.

### AC5 — Revocation + catchup (DoD #5)

- **AC5.1 Success:** Refresh detects user lost access to `{x, y}` →
  INSERT INTO `auth_revocation_log` (returning seq) AND
  `pg_notify('atc_auth_invalidation', seq::text)` inside one
  transaction.
- **AC5.2 Success:** Other replicas' listeners receive NOTIFY → SELECT
  the row by seq → broadcast `RevocationNotice` →
  `WsRegistry::cancel_user` cancels matching tokens.
- **AC5.3 Success:** WS handler observes cancel → sends Close
  (code 1000, "access revoked") → exits cleanly.
- **AC5.4 Success:** Client reconnects, calls `/v1/state` → returns new
  scoped snapshot.
- **AC5.5 Success:** Simulated dropped NOTIFY (block listener for
  >10s) → catchup heartbeat re-emits from bounded-window rescan;
  replicas eventually receive it. Ring-buffer dedup prevents duplicate
  broadcasts when the NOTIFY also lands later.
- **AC5.6 Success:** Token refresh returns `invalid_grant` → session
  row deleted, `Session(id)` revocation broadcast, that session's WS
  dropped (other sessions for the same user keep working).
- **AC5.7 Success:** All sessions for a user expire →
  `clear_access_cache` + `AllSessions` broadcast.
- **AC5.8 Success:** `auth_revocation_log` rows older than 1h are
  swept (hourly).

### AC6 — Multi-replica consistency (DoD #6)

- **AC6.1 Success:** Two PG-mode replicas: authenticated request to
  either returns identical scoped state for the same user.
- **AC6.2 Success:** Session created on Replica A is usable on Replica B
  immediately.
- **AC6.3 Success:** Logout on Replica A invalidates session on
  Replica B's next request.

### AC7 — Frontend (DoD #7)

- **AC7.1 Success:** Cold load no cookie + auth enabled → spinner →
  `LoginScreen`.
- **AC7.2 Success:** Cold load no cookie + auth disabled → spinner →
  main app, no login UI.
- **AC7.3 Success:** Cold load valid cookie → spinner → main app, user
  avatar in top bar.
- **AC7.4 Success:** Authenticated + `accessible_repos_count: 0` →
  `NoReposAccessible` instead of `KanbanBoard`.
- **AC7.5 Success:** Logout → 204 → redirect to `/` → `LoginScreen`.
- **AC7.6 Success:** 401 from `/v1/state` mid-session →
  `authStore.init()` → `LoginScreen`.

### AC8 — Dev mode (DoD #8)

- **AC8.1 Success:** `just dev` with no env vars + `auth_required` unset
  (default false) → server starts, "AUTH DISABLED" logged, all routes
  work without cookies.
- **AC8.2 Success:** `curl http://localhost:8080/v1/state` returns full
  snapshot in dev mode.
- **AC8.3 Success:** smee.io webhook to local dev → state updates via
  curl.

### AC9 — Webhook untouched (DoD #9)

- **AC9.1 Success:** All existing webhook integration tests pass
  unmodified.
- **AC9.2 Success:** `POST /v1/webhooks/github` with valid HMAC works
  in both modes.
- **AC9.3 (behavioral regression):** Auth enabled + no session cookie
  → POST valid webhook → 200 `{status: "accepted"}`.

### AC10 — Production fail-fast (DoD #10)

- **AC10.1 Failure:** Server started with `ATC_AUTH__REQUIRED=true` +
  one or both creds missing → exit non-zero with descriptive log line
  identifying the missing cred(s).
- **AC10.2 Success:** Helm chart with `auth.required: true` + both
  creds provided via secret ref → renders successfully; pod env
  includes `ATC_AUTH__REQUIRED=true` and both cred env vars.
- **AC10.3 Success:** Helm chart with `auth.required: false` + no
  creds → renders successfully (proxy-fronted deployment path);
  pod env includes `ATC_AUTH__REQUIRED=false`.

### AC11 — Single-flight refresh (DoD #12)

- **AC11.1 Success:** Two replicas attempt `refresh_user(U)` in same
  window. One acquires the advisory lock and performs GitHub calls;
  the other returns `skipped_locked` and increments
  `atc_auth_advisory_lock_contention_total`.
- **AC11.2 Success:** Replica that lost the lock then re-reads
  `user_access_cache` and sees the winner's update.
- **AC11.3 Success:** Inline `/v1/state` lock-contention path: waits
  up to 5s for the lock holder, then re-reads cache (does not return
  empty).
- **AC11.4 Success:** Advisory lock is released on guard drop even
  when GitHub call panics or returns an error.

### AC12 — Tests (DoD #13)

- **AC12.1 Success:** `cargo nextest run -p atc-server` passes.
- **AC12.2 Success:** `cargo nextest run -p atc-store-pg` passes
  (CRUD, refresh race, NOTIFY+catchup, advisory lock, log sweep).
- **AC12.3 Success:** `cargo nextest run -p atc-store-mem` passes.
- **AC12.4 Success:** `cargo nextest run -p atc-github` passes
  multi-page pagination.
- **AC12.5 Success:** Multi-replica integration test passes (AC6,
  AC11 invariants).
- **AC12.6 Success:** `cd frontend && pnpm vitest run` passes.
- **AC12.7 Success:** `cd frontend && pnpm playwright test` passes
  E2E auth flow.

---

## Documents to Update

| File | Change |
|---|---|
| `backend/crates/atc-persist/CLAUDE.md` | Add `SessionStore` trait; document `Session`, `AccessCacheRow`, `RevocationNotice`, `RevocationScope`, `AccessScope`, `RefreshLockGuard` types. |
| `backend/crates/atc-store-pg/CLAUDE.md` | Add `PgSessionStore`; migrations 0005/0006/0007/0008; document `atc_auth_invalidation` NOTIFY channel (payload = `auth_revocation_log.seq`); document the bounded-window catchup pattern; document the advisory-lock mechanism; document listener uses `ATC_DATABASE_LISTENER_URL`. |
| `backend/crates/atc-store-mem/CLAUDE.md` | Add `InMemorySessionStore`; dev-only. |
| `backend/crates/atc-wire/CLAUDE.md` | Add `User` wire type; add `accessible_repos_count: u64` field on `StateSnapshot` (composed at the handler, not the store). |
| `backend/crates/atc-github/CLAUDE.md` | Add `oauth` module; document PKCE; document pagination handling; document the "expiring user tokens" prerequisite. |
| `backend/crates/atc-server/CLAUDE.md` | Add `auth/` submodule; document `AuthService`, `WsRegistry`, `RefreshSupervisor` (with `TaskTracker`), `RevocationConsumer`; the new routes; the middleware. Extend the supervision/shutdown section with the new consumer task AND the refresh-supervisor join. Document the `ATC_AUTH__REQUIRED` env var (default false). |
| `frontend/CLAUDE.md` | Add `authStore`; document the `App.svelte` auth gate; document the cookie+WS same-origin invariant. |
| `docs/architecture/backend-server.md` | New section "Authentication and Authorization" covering: OAuth flow with PKCE + callback re-init behavior, session storage, per-user refresh tick + inline freshness + advisory-lock single-flight, revocation NOTIFY + bounded-window catchup, dev-mode bypass + server-side fail-fast. |
| `docs/architecture/frontend-app.md` | New section covering `authStore`, the auth gate in App.svelte, OAuth-callback navigation. |
| `docs/architecture/deployment.md` | Add `ATC_GITHUB__APP__CLIENT_ID`, `ATC_GITHUB__APP__CLIENT_SECRET`, `ATC_AUTH__REQUIRED` to the operator-config reference. Document the same-origin requirement; document App-setup prerequisites (expiring user tokens enabled, redirect URL exact match — and that **redirect URL must be updated when the deployment URL changes**). Document the operator-visible caveat that GitHub access tokens are stored plaintext in `sessions` (backups carry live tokens). Document that proxy-fronted deployments can set `auth.required: false`. |
| `docs/architecture/metrics.md` | Add spans (`auth.login`, `auth.refresh.user`, `auth.notify.recv`) and counters (`atc_auth_*`, `atc_github_api_calls_total`, `atc_auth_advisory_lock_contention_total`). |
| `docs/architecture-decisions/0009-github-app-user-auth.md` | New ADR: GitHub App + user-to-server tokens, PKCE, per-request PG consistency, synthetic dev user, multi-session + logout-everywhere, server-side fail-fast (no chart guard), advisory-lock single-flight, bounded-window catchup. |
| `scripts/doc-mapping.sh` | Verify existing catch-alls (`backend/crates/atc-server/src/**`, `backend/crates/atc-github/src/**`, `frontend/src/**`) already cover new files. Add only entries for the new migrations and the new `docs/architecture/frontend-app.md` ↔ `frontend/src/**` mapping if not already present. |
| `deploy/helm/atc/values.yaml` | Add `auth.required: true`; `github.app.clientId: ""`; `github.app.clientSecretRef: ""`. |
| `deploy/helm/atc/values.schema.json` | Add schema entries for the new fields. |
| `deploy/helm/atc/CLAUDE.md` | Document the new `auth.required` value (operator-toggleable; no render guard) and the OAuth secret refs. |
| `deploy/helm/atc/README.md` | Add operator-facing setup: registering a GitHub App, configuring `auth.required`, providing client ID + secret, the proxy-fronted alternative. |
| `deploy/helm/atc/templates/deployment.yaml` | Wire env vars from secret refs. (No render-time guard.) |
| `deploy/helm/atc/templates/NOTES.txt` | Post-install pointers to App setup steps. |
| `docs/architecture-decisions/0006-*.md` (existing) | Annotation sweep: add `> **See ADR-0009 for authentication concerns.**` cross-reference at the bottom. |

---

## Out of Scope

- Org/team admin UI (user→repo is GitHub's existing model).
- Webhook payload authorization (HMAC-only stays).
- Audit logging of access decisions.
- Rate limiting / abuse mitigation on auth endpoints.
- Session retention / cleanup on user departure.
- App-managed webhook delivery (operators keep current webhook URLs).
- Repo picker UI (filtering is automatic).
- **Per-device "sign out of this device"** — only the everywhere variant
  is supported.
- **At-rest encryption of stored GitHub access tokens** — plaintext
  initially; column-level encryption is a follow-up hardening item.
  Operator-visible caveat documented in deployment.md.
- **Cross-origin deployment** — same-origin only.
  Subdomain / cross-origin would require `SameSite=None`, CORS,
  credentialed fetch + WS configuration. Future design.
- **GitHub Apps with expiring user tokens disabled** — modern default
  is required; non-expiring path is documented as unsupported.
- **Helm chart-time render guard for auth misconfig** — intentionally
  omitted so proxy-fronted deployments can opt out of auth. Server-side
  fail-fast (AC10.1) catches the case where the operator did opt in
  but provided incomplete creds.

---

## Verification (end-to-end)

After implementation, an operator should be able to:

1. **Local dev (auth disabled).** `just setup && just dev` (no env
   vars) — server boots, logs "AUTH DISABLED — dev only", curl/smee.io
   flow unchanged.
2. **Local dev (auth enabled).** Set `ATC_GITHUB__APP__CLIENT_ID`,
   `ATC_GITHUB__APP__CLIENT_SECRET`, `ATC_AUTH__REQUIRED=true` pointing
   at a personal GitHub App registered against `http://localhost:8080`
   with redirect URL `http://localhost:8080/v1/auth/github/callback`
   and expiring user tokens enabled. Install on a test repo.
   `just dev` — visit `http://localhost:8080`, see login screen, click
   sign-in, GitHub auth flow completes, see dashboard scoped to test
   repo.
3. **Multi-replica (production shape).** `just helm-install` against a
   local k3d cluster with `replicaCount: 2`, `auth.required: true`,
   and PG. Two users log in, verify each sees only their accessible
   repos. Revoke one user's access from GitHub; within ~60s, that
   user's WS drops and reconnect shows the new (reduced) set.
4. **Proxy-fronted deployment (auth off at app layer).**
   `just helm-install` with `auth.required: false` + no OAuth creds.
   Chart renders. Server starts. All routes serve the full snapshot.
   Operator's reverse proxy enforces auth in front.

Automated coverage of these flows is in AC12.

---

## Glossary

- **GitHub App user-to-server token** — Short-lived (8h) access token
  issued when a user authenticates via the App's OAuth handshake.
  Requires the App to have "expiring user tokens" enabled (prerequisite).
  Refreshable for up to 6 months via the paired refresh token; refresh
  rotates both tokens.
- **Installation** — A GitHub App's authorization scope on a specific
  org or user account. Created on Install.
- **PKCE** — Proof Key for Code Exchange (RFC 7636). Client generates
  high-entropy `code_verifier`, sends `SHA256(verifier)` as
  `code_challenge` on the authorize step, presents the raw `verifier`
  on the token-exchange step. Prevents authorization-code interception.
- **SID cookie** — Session ID cookie. UUID stored
  `HttpOnly; Secure; SameSite=Lax`, mapping to a `sessions` row.
- **`AccessScope`** — Enum (`AccessScope::All | Scoped(Vec<RepoKey>)`).
  `All` is the synthetic dev-user marker; `Scoped` is the real
  authenticated user's repo intersection.
- **Access cache** — `user_access_cache` table; per-user accessible
  repo list + refresh timestamp.
- **Refresh tick** — 60s-interval per-USER (not per-session) background
  task that refreshes the access cache. Single-flighted across replicas
  via PG advisory lock.
- **Inline freshness check** — On `/v1/state`, refresh the access
  cache before serving if `refreshed_at < now() - 60s`. Covers users
  with valid sessions but no WS connection.
- **Advisory lock** — `pg_try_advisory_xact_lock(hashtext('atc.refresh.'
  || user_id))`. PG impl; in-memory impl uses a process-local mutex
  set. Held for the duration of `refresh_user`.
- **`atc_auth_invalidation`** — PG NOTIFY channel. Payload is a `seq`
  pointing into `auth_revocation_log`. Listeners SELECT by seq and
  broadcast.
- **`auth_revocation_log`** — Append-only PG table. Catchup heartbeat
  rescans rows from the last 1h (bounded window, dedup ring buffer);
  does NOT use a `seq > cursor` arithmetic. Rows older than 1h are
  swept hourly.
- **`RevocationScope::AllSessions`** — Revocation affecting all of
  the user's sessions (logout, access-cache rewrite with drops). All
  replicas cancel all WS connections for the user.
- **`RevocationScope::Session(SessionId)`** — Revocation affecting one
  specific session (token-refresh `invalid_grant`). All replicas
  cancel only that session's connections.
- **Synthetic dev user** — A constant `AuthenticatedUser` value used
  when auth is disabled; has `AccessScope::All` which short-circuits
  the intersection filter.
- **Auth-disabled mode** — Server mode when `ATC_AUTH__REQUIRED=false`
  (default). Creds are ignored if provided. All routes accept
  unauthenticated requests; the synthetic dev user is attached.
- **Same-origin requirement** — The SPA and the HTTP/WebSocket API are
  served from the same Axum origin. Cross-origin out of scope.
