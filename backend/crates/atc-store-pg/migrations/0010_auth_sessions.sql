-- Pre-auth OAuth flow state and post-login sessions for native GitHub auth
-- (auth.github mode). No token columns anywhere: ADR-0014 locks in that
-- ATC never stores a GitHub access or refresh token — the repo-authorization
-- set is derived at callback and both tokens are discarded immediately.
--
-- auth_flows binds an in-progress OAuth round-trip to the browser that
-- started it (via the __Host-atc_flow cookie carrying `flow_id`) and the
-- `state`/PKCE verifier GitHub's callback must be checked against. Rows are
-- single-use (deleted on `consume_flow`) and treated as expired 10 minutes
-- after `created_at` regardless of whether they were consumed.
--
-- auth_sessions is the post-login session. `id_hash` is the SHA-256 hex
-- digest of the opaque session id that lives in the browser's
-- __Host-atc_session cookie — the raw value is never persisted, so a
-- database dump alone cannot forge a session cookie. `repos_refreshed_at`
-- is the staleness clock `repo_auth_ttl` measures against; `expires_at` is
-- the absolute `max_session_ttl` cutoff independent of that staleness.
--
-- `created_at`/`expires_at`/`repos_refreshed_at` have no DEFAULT now() on
-- purpose: every timestamp here is bound Rust-side from `Clock::now()` so
-- `TestClock`-driven tests can advance time deterministically, matching the
-- convention established for `outbox_watermarks.updated_at`.
CREATE TABLE auth_flows (
    flow_id       TEXT PRIMARY KEY,
    state         TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    return_to     TEXT NOT NULL DEFAULT '/',
    popup         BOOLEAN NOT NULL DEFAULT FALSE,
    created_at    TIMESTAMPTZ NOT NULL
);

CREATE TABLE auth_sessions (
    id_hash            TEXT PRIMARY KEY,
    github_user_id     BIGINT NOT NULL,
    github_login       TEXT NOT NULL,
    repo_ids           BIGINT[] NOT NULL,
    repos_refreshed_at TIMESTAMPTZ NOT NULL,
    created_at         TIMESTAMPTZ NOT NULL,
    expires_at         TIMESTAMPTZ NOT NULL
);

-- Supports the sweep's `DELETE ... WHERE expires_at < $1`. auth_flows has no
-- equivalent index: flow rows are single-use and 10-minute-bounded, so the
-- table stays small enough that the sweep's `created_at` scan needs no
-- index of its own.
CREATE INDEX auth_sessions_expires_at_idx ON auth_sessions (expires_at);
