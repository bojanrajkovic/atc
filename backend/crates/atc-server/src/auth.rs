//! Native GitHub OAuth login/callback/logout/whoami endpoints for
//! `auth.mode = "github"`.
//!
//! **Tokens are used inside the callback handler and discarded — never
//! stored, never logged** (locked decision, ADR-0014). The access token
//! obtained from the exchange lives only as long as the callback request
//! takes to derive identity + the repo-authorization set; nothing
//! GitHub-issued crosses this module's boundary into a log line, span, or
//! the session row.
//!
//! Both routes are mounted only when `auth.mode = "github"` — see
//! [`crate::routes::api_routes`]. There is no runtime mode check inside
//! these handlers; when auth is disabled, the routes simply don't exist in
//! the router, so a request to them 404s the same way any unknown path does.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::{FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use opentelemetry::KeyValue;
use opentelemetry::metrics::{Counter, Histogram};
use sha2::{Digest, Sha256};
use tracing::{Instrument, field, info_span};

use atc_core::RepoId;
use atc_store_pg::metrics::METER_SCOPE;
use atc_store_pg::{Session, SessionStore};

use crate::github_client::{GitHubClient, GitHubClientError};
use crate::public_repo_cache::PublicRepoCache;
use crate::state::AppState;

/// OTel instrumentation for `auth.github`: login outcomes, request
/// rejections, and callback GitHub round-trip duration.
///
/// Mirrors the cached-instrument convention (`docs/architecture/metrics.md`
/// § "Cached instrument convention") used by [`crate::ws::WsMetrics`] — the
/// `Counter`/`Histogram` instruments are registered once and cached; unlike
/// `WsMetrics`'s two fixed label combinations, the label sets here (5 login
/// outcomes, a surface × reason cross product, 2 duration phases) are built
/// as `KeyValue` slices per call rather than pre-built as struct fields —
/// this surface sees at most one emit per HTTP request, not a per-row hot
/// path, so the extra allocation is not worth the field-per-combination
/// sprawl.
pub struct AuthMetrics {
    logins: Counter<u64>,
    rejections: Counter<u64>,
    callback_duration: Histogram<f64>,
}

impl AuthMetrics {
    /// Register OTel instruments against the global meter. Must run after
    /// `otel::init_otel`; safe under the no-op meter.
    #[must_use]
    pub fn register() -> Arc<Self> {
        let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);

        let logins = meter
            .u64_counter("atc_auth_logins_total")
            .with_description(
                "auth.github login attempts by outcome: success, state_mismatch, \
                 missing_flow, exchange_failed, denied, or session_error (a local \
                 SessionStore failure, distinct from the GitHub-side outcomes).",
            )
            .build();
        let rejections = meter
            .u64_counter("atc_auth_rejections_total")
            .with_description(
                "Requests rejected by AuthContext/WS enforcement, labeled by surface \
                 (state, ws, me) and reason (auth_required, stale_authorization, \
                 origin_mismatch).",
            )
            .build();
        let callback_duration = meter
            .f64_histogram("atc_auth_callback_duration_seconds")
            .with_description(
                "Wall time of the callback handler's GitHub round trips, labeled by \
                 phase (exchange = token exchange; repos = identity + authorized-repo-set \
                 derivation).",
            )
            .build();

        Arc::new(Self {
            logins,
            rejections,
            callback_duration,
        })
    }

    fn record_login(&self, outcome: &'static str) {
        self.logins.add(1, &[KeyValue::new("outcome", outcome)]);
    }

    /// Increment `atc_auth_rejections_total{surface, reason}`. `surface` is
    /// which mounted route rejected (`"state"` / `"ws"` / `"me"`); `reason`
    /// is `"auth_required"` / `"stale_authorization"` / `"origin_mismatch"`.
    /// Called from `routes::state_handler`, `ws::ws_handler`, and
    /// `AuthContext::from_request_parts` — the three surfaces that can
    /// produce a rejection.
    pub(crate) fn record_rejection(&self, surface: &'static str, reason: &'static str) {
        self.rejections.add(
            1,
            &[
                KeyValue::new("surface", surface),
                KeyValue::new("reason", reason),
            ],
        );
    }

    fn record_callback_duration(&self, phase: &'static str, seconds: f64) {
        self.callback_duration
            .record(seconds, &[KeyValue::new("phase", phase)]);
    }
}

/// Random bytes in a `state`/PKCE-verifier token, before base64url encoding.
/// Matches `atc-store-pg::session`'s token size (256 bits).
const TOKEN_BYTES: usize = 32;

/// `Max-Age` of the pre-auth flow cookie — matches `auth_flows`' 10-minute
/// TTL (`atc-store-pg::session::FLOW_TTL`).
const FLOW_COOKIE_MAX_AGE_SECS: u64 = 600;

const PKCE_METHOD: &str = "S256";

/// Everything the login/callback handlers need, threaded onto [`AppState`]
/// as `Option<AuthRuntime>` — `None` when `auth.mode = "none"`. Constructed
/// once in `main.rs` from the validated `[auth.github]` config (validation
/// already guarantees these fields are present when mode = "github" — see
/// `config::validate_auth_config`).
pub struct AuthRuntime {
    pub github: Arc<GitHubClient>,
    pub sessions: Arc<SessionStore>,
    /// Determines cookie naming/`Secure` (§ `cookie_names`) and is the
    /// OAuth `redirect_uri` base.
    pub public_origin: String,
    pub max_session_ttl: Duration,
    /// Staleness window `GET /v1/auth/me` measures `repos_refreshed_at`
    /// against — independent of `max_session_ttl` (the absolute session
    /// lifetime).
    pub repo_auth_ttl: Duration,
    /// App-wide cache of publicly-visible repo IDs, unioned into every
    /// session's `repo_ids` at callback (ADR-0014, amended decision 2).
    pub public_repos: Arc<PublicRepoCache>,
}

/// The `auth.github` routes. Merged into the router only when
/// `auth.mode = "github"` — see [`crate::routes::api_routes`].
pub fn auth_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/auth/github/login", get(login_handler))
        .route("/v1/auth/github/callback", get(callback_handler))
        .route("/v1/auth/github/logout", post(logout_handler))
        .route("/v1/auth/me", get(whoami_handler))
}

/// Look up the session for an already-resolved raw session cookie value, if
/// any — `None` covers both "no cookie" and "cookie present but
/// unknown/expired". Takes the raw value rather than `&HeaderMap` so
/// [`AuthContext::from_request_parts`] (its sole caller) resolves the cookie
/// exactly once instead of looking it up again to also determine whether a
/// cookie was present at all.
async fn session_from_cookie(
    auth: &AuthRuntime,
    raw_cookie: Option<&str>,
) -> Result<Option<Session>, impl std::fmt::Display> {
    match raw_cookie {
        Some(raw) => auth.sessions.load_session(raw).await,
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Cookies
// ---------------------------------------------------------------------------

struct CookieNames {
    flow: &'static str,
    session: &'static str,
    /// Whether `public_origin` is https — both the `__Host-` prefix choice
    /// below and the `Secure` attribute on every cookie this module sets
    /// follow this same check, so callers read it off here instead of
    /// recomputing `starts_with("https://")` themselves.
    secure: bool,
}

/// `__Host-` prefixed names + `Secure` require an https origin (the prefix
/// is browser-enforced: a `__Host-` cookie is rejected outright if `Secure`
/// is missing). Dev origins (http, non-TLS) fall back to plain names
/// without `Secure` so local development over plain HTTP still works.
fn cookie_names(public_origin: &str) -> CookieNames {
    if public_origin.starts_with("https://") {
        CookieNames {
            flow: "__Host-atc_flow",
            session: "__Host-atc_session",
            secure: true,
        }
    } else {
        CookieNames {
            flow: "atc_flow",
            session: "atc_session",
            secure: false,
        }
    }
}

/// Build a `Set-Cookie` header value. `secure` mirrors [`cookie_names`]'s
/// https/http split — `__Host-` names require it, plain names omit it.
fn set_cookie_header(name: &str, value: &str, max_age_secs: u64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age_secs}{secure_attr}")
}

/// Parse the request's `Cookie` header for a named value. Cookies arrive as
/// one `name1=value1; name2=value2` header, not one header per cookie.
fn get_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';').find_map(|pair| {
        let (k, v) = pair.trim().split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// 302 Found to `location`. `axum::response::Redirect::to` sends 303 See
/// Other, which the design doc's flow diagrams don't call for — every
/// redirect in this module is spec'd as a plain 302.
fn redirect_302(location: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
}

/// Attach a `Set-Cookie` header to an already-built response.
fn with_set_cookie(mut response: Response, cookie: &str) -> Response {
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie.parse().expect("cookie header is valid"),
    );
    response
}

// ---------------------------------------------------------------------------
// state / PKCE
// ---------------------------------------------------------------------------

/// Generate a token: [`TOKEN_BYTES`] of OS randomness, base64url, no
/// padding. Used for both the OAuth `state` value and the PKCE verifier —
/// same shape as `atc-store-pg::session`'s private `random_token`, which
/// returns `sqlx::Error` (its callers' shared error type) rather than
/// `getrandom::Error`; duplicated rather than unified because sharing one
/// function would mean picking one crate's error type for the other to
/// map into, for a 4-line body.
fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Log + build a `500` for a failed [`random_token`] call. `what` names
/// which token (`"state"`, `"PKCE verifier"`) for the log.
fn random_token_failed(e: getrandom::Error, what: &str) -> Response {
    tracing::warn!(error.message = %e, what, "auth.login: failed to generate token");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}

/// PKCE S256 challenge: base64url(sha256(verifier)), no padding. GitHub
/// only supports `S256`; `plain` is rejected — see the design doc's
/// "Locked decisions" table.
fn pkce_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

/// Validate `return_to`: must be a same-origin relative path (starts with
/// `/`, not `//` — a `//evil.example.com` value is scheme-relative and
/// browsers treat it as an absolute redirect, the classic open-redirect
/// shape). Anything else falls back to `/`.
fn validate_return_to(return_to: Option<&str>) -> String {
    match return_to {
        Some(path) if path.starts_with('/') && !path.starts_with("//") => path.to_string(),
        _ => "/".to_string(),
    }
}

// ---------------------------------------------------------------------------
// GET /v1/auth/github/login
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct LoginQuery {
    return_to: Option<String>,
    popup: Option<String>,
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<LoginQuery>,
) -> Response {
    let Some(auth) = state.auth.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    let return_to = validate_return_to(query.return_to.as_deref());
    let popup = query.popup.as_deref() == Some("1");

    let oauth_state = match random_token() {
        Ok(s) => s,
        Err(e) => return random_token_failed(e, "state"),
    };
    let verifier = match random_token() {
        Ok(v) => v,
        Err(e) => return random_token_failed(e, "PKCE verifier"),
    };
    let challenge = pkce_challenge(&verifier);

    let flow_id = match auth
        .sessions
        .create_flow(&oauth_state, &verifier, &return_to, popup)
        .await
    {
        Ok(id) => id,
        Err(e) => {
            tracing::warn!(error.message = %e, "auth.login: failed to create flow");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let redirect_uri = format!("{}/v1/auth/github/callback", auth.public_origin);
    let mut authorize_url = reqwest::Url::parse("https://github.com/login/oauth/authorize")
        .expect("static URL is valid");
    authorize_url
        .query_pairs_mut()
        .append_pair("client_id", auth.github.client_id())
        .append_pair("state", &oauth_state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", PKCE_METHOD)
        .append_pair("redirect_uri", &redirect_uri);

    let names = cookie_names(&auth.public_origin);
    let flow_cookie =
        set_cookie_header(names.flow, &flow_id, FLOW_COOKIE_MAX_AGE_SECS, names.secure);

    with_set_cookie(redirect_302(authorize_url.as_str()), &flow_cookie)
}

// ---------------------------------------------------------------------------
// GET /v1/auth/github/callback
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Log + build a `502 Bad Gateway` for a failed GitHub-side call. `what`
/// names the call (e.g. `"token exchange"`) for both the log and the body.
fn github_call_failed(e: &GitHubClientError, what: &str) -> Response {
    tracing::warn!(reason = "exchange_failed", error.message = %e, what, "auth.callback: GitHub call failed");
    (StatusCode::BAD_GATEWAY, format!("GitHub {what} failed")).into_response()
}

/// Log + build a `500` for a failed `SessionStore` operation. Kept as its
/// own `reason` (`session_error`), distinct from `exchange_failed` — an
/// operator filtering logs by failure reason needs to tell a GitHub-side
/// outage apart from a local Postgres blip; they need different remediation.
fn session_store_failed(e: impl std::fmt::Display, what: &str) -> Response {
    tracing::warn!(reason = "session_error", error.message = %e, what, "auth: session store operation failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("failed to {what}"),
    )
        .into_response()
}

async fn callback_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<CallbackQuery>,
    headers: HeaderMap,
) -> Response {
    let span = info_span!(
        "auth.callback",
        outcome = field::Empty,
        repo_count = field::Empty,
    );
    async move {
        let Some(auth) = state.auth.as_ref() else {
            return StatusCode::NOT_FOUND.into_response();
        };
        // Records both the span's `outcome` field and the
        // `atc_auth_logins_total{outcome}` counter from one call site, so
        // every exit from this handler reports the same value to both
        // surfaces.
        let record_outcome = |outcome: &'static str| {
            tracing::Span::current().record("outcome", outcome);
            state.auth_metrics.record_login(outcome);
        };
        let names = cookie_names(&auth.public_origin);

        let Some(flow_cookie) = get_cookie(&headers, names.flow) else {
            record_outcome("missing_flow");
            tracing::warn!(reason = "missing_flow", "auth.callback: no flow cookie");
            return (StatusCode::BAD_REQUEST, "missing or expired login attempt").into_response();
        };

        let flow = match auth.sessions.consume_flow(&flow_cookie).await {
            Ok(Some(flow)) => flow,
            Ok(None) => {
                record_outcome("missing_flow");
                tracing::warn!(
                    reason = "missing_flow",
                    "auth.callback: flow not found or expired"
                );
                return (StatusCode::BAD_REQUEST, "missing or expired login attempt")
                    .into_response();
            }
            Err(e) => {
                record_outcome("session_error");
                return session_store_failed(e, "consume login attempt");
            }
        };

        // A denied authorization carries `error` (commonly `access_denied`) and
        // no `code` — home, not the deep-linked `return_to`, since there's
        // nothing to resume.
        if query.error.is_some() {
            record_outcome("denied");
            tracing::warn!(
                reason = "denied",
                "auth.callback: user denied authorization"
            );
            return redirect_302("/?auth_error=denied");
        }

        if query.state.as_deref() != Some(flow.state.as_str()) {
            record_outcome("state_mismatch");
            tracing::warn!(
                reason = "state_mismatch",
                "auth.callback: state parameter mismatch"
            );
            return (StatusCode::BAD_REQUEST, "invalid login attempt").into_response();
        }

        let Some(code) = query.code.as_deref() else {
            record_outcome("missing_flow");
            tracing::warn!(reason = "missing_flow", "auth.callback: no code parameter");
            return (StatusCode::BAD_REQUEST, "missing authorization code").into_response();
        };

        let redirect_uri = format!("{}/v1/auth/github/callback", auth.public_origin);
        let exchange_start = std::time::Instant::now();
        let exchange_result = auth
            .github
            .exchange_code(code, &redirect_uri, &flow.pkce_verifier)
            .await;
        // Recorded before the outcome check, on both the Ok and Err arms — a
        // slow-then-failing exchange (e.g. GitHub degraded) is exactly the
        // case an operator needs this histogram to surface, not just the
        // successful calls.
        state
            .auth_metrics
            .record_callback_duration("exchange", exchange_start.elapsed().as_secs_f64());
        let access_token = match exchange_result {
            Ok(token) => token,
            Err(e) => {
                record_outcome("exchange_failed");
                return github_call_failed(&e, "token exchange");
            }
        };

        // Identity and the repo-authorization set are independent — neither
        // depends on the other — so they run concurrently rather than as two
        // sequential round trips.
        let repos_start = std::time::Instant::now();
        let user_call = async {
            auth.github
                .get_user(&access_token)
                .await
                .map_err(|e| ("identity lookup", e))
        };
        let repos_call = async {
            auth.github
                .get_authorized_repo_ids(&access_token)
                .await
                .map_err(|e| ("repository lookup", e))
        };
        let repos_result = tokio::try_join!(user_call, repos_call);
        state
            .auth_metrics
            .record_callback_duration("repos", repos_start.elapsed().as_secs_f64());
        let (user, repo_ids) = match repos_result {
            Ok((user, repo_ids)) => (user, repo_ids),
            Err((what, e)) => {
                record_outcome("exchange_failed");
                return github_call_failed(&e, what);
            }
        };
        // `access_token` and the token-exchange response are not referenced
        // again past this point — nothing GitHub-issued survives this handler.

        let now = state.clock.now();

        // Deliberately outside the `try_join!` above: a public-repo lookup
        // failure (`PublicRepoCache::get` never errors — see its doc comment)
        // must never block login the way a real `repos_call` failure does.
        let public_repo_ids = auth.public_repos.get(now).await;
        let repo_ids: Vec<i64> = repo_ids
            .into_iter()
            .chain(public_repo_ids.iter().copied())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let existing_raw_id = get_cookie(&headers, names.session);
        let existing_session = match &existing_raw_id {
            Some(raw) => match auth.sessions.load_session(raw).await {
                Ok(session) => session,
                Err(e) => {
                    record_outcome("session_error");
                    return session_store_failed(e, "load existing session");
                }
            },
            None => None,
        };
        // A session cookie present on this request but belonging to a DIFFERENT
        // GitHub user (e.g. a shared browser, or a stale cookie from a prior
        // account) must not be refreshed with the new user's repo set under the
        // old identity — `refresh_session_repos` only ever touches
        // `repo_ids`/`repos_refreshed_at`, never `github_user_id`, so refreshing
        // it here would silently attribute the new user's repo access to the
        // old session's identity. Treat a mismatch the same as no existing
        // session: fall through to `create_session` below.
        let existing_session = existing_session.filter(|s| s.github_user_id == user.id);

        let (session_cookie_value, cookie_max_age_secs) = match existing_session {
            Some(existing) => {
                if let Err(e) = auth
                    .sessions
                    .refresh_session_repos(&existing.id_hash, &repo_ids, now)
                    .await
                {
                    record_outcome("session_error");
                    return session_store_failed(e, "update session");
                }
                // `refresh_session_repos` never extends `expires_at` (it's the
                // absolute session lifetime, independent of the repo-staleness
                // clock it does update — see the design doc's "Data model"
                // section) — so the cookie's Max-Age must reflect the time
                // remaining until the DB row's real expiry, not a fresh
                // `max_session_ttl`. Reissuing the full TTL here would tell the
                // browser to keep a cookie alive well past when the server
                // will actually reject it.
                let remaining = (existing.expires_at - now).num_seconds().max(0);
                let raw_id =
                    existing_raw_id.expect("existing_session implies existing_raw_id is Some");
                (raw_id, u64::try_from(remaining).unwrap_or(0))
            }
            None => match auth
                .sessions
                .create_session(user.id, &user.login, &repo_ids, now, auth.max_session_ttl)
                .await
            {
                Ok(raw_id) => (raw_id, auth.max_session_ttl.as_secs()),
                Err(e) => {
                    record_outcome("session_error");
                    return session_store_failed(e, "create session");
                }
            },
        };

        record_outcome("success");
        tracing::Span::current().record("repo_count", repo_ids.len());
        tracing::info!(
            user = %user.login,
            user_id = user.id,
            repo_count = repo_ids.len(),
            "auth.callback: login succeeded",
        );
        tracing::debug!(repo_ids = ?repo_ids, "auth.callback: authorized repo set");

        let session_cookie = set_cookie_header(
            names.session,
            &session_cookie_value,
            cookie_max_age_secs,
            names.secure,
        );

        let response = if flow.popup {
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                POPUP_CALLBACK_HTML,
            )
                .into_response()
        } else {
            redirect_302(&flow.return_to)
        };
        with_set_cookie(response, &session_cookie)
    }
    .instrument(span)
    .await
}

/// Popup-mode callback response. The `BroadcastChannel` name (`atc-auth`)
/// and message (`session-refreshed`) are a fixed contract consumed by the
/// frontend's silent re-auth flow (#464) — keep them exactly as written.
const POPUP_CALLBACK_HTML: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Signed in</title></head>
<body><script>
new BroadcastChannel('atc-auth').postMessage('session-refreshed');
window.close();
</script></body></html>
"#;

// ---------------------------------------------------------------------------
// POST /v1/auth/github/logout
// ---------------------------------------------------------------------------

/// No CSRF token in v1: a forged cross-site logout can only log the victim
/// out (forcing a re-login), not escalate privilege or read/write anything
/// — an accepted availability-only gap, not an oversight. See the design
/// doc's "Locked decisions" table.
async fn logout_handler(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let Some(auth) = state.auth.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let names = cookie_names(&auth.public_origin);

    // Idempotent: no cookie, or a cookie for an already-gone session, both
    // still clear the cookie and return 204 — there's nothing to undo
    // either way. Only a genuine SessionStore failure surfaces as an error.
    if let Some(raw) = get_cookie(&headers, names.session)
        && let Err(e) = auth.sessions.delete_session(&raw).await
    {
        return session_store_failed(e, "delete session");
    }

    let cleared_cookie = set_cookie_header(names.session, "", 0, names.secure);
    with_set_cookie(StatusCode::NO_CONTENT.into_response(), &cleared_cookie)
}

// ---------------------------------------------------------------------------
// AuthContext extractor + 401 reason contract
// ---------------------------------------------------------------------------

/// Request-time identity, shared by every handler that needs to know who's
/// asking (`/v1/auth/me` here; the read rails `/v1/state`/`/v1/ws` in
/// #459/#460). `Disabled` short-circuits before touching the session store —
/// `mode = "none"` deployments pay zero extra cost per request.
///
/// Staleness (`repos_refreshed_at + repo_auth_ttl` elapsed) is deliberately
/// **not** checked during extraction: `whoami_handler` below reports it as a
/// `stale` body field rather than rejecting (the identity-chrome bootstrap
/// depends on that shape — see issue #463), while the read rails call
/// [`AuthContext::require_fresh`] to reject with [`AuthRejection::Stale`]
/// instead.
#[derive(Debug)]
pub enum AuthContext {
    Disabled,
    Session(SessionIdentity),
}

/// The identity + authorization set carried by a valid session. `repo_ids`
/// uses the `RepoId` newtype (#449), since the enforcement handlers compare
/// it directly against `WorkflowRun::repo_id`. `repos_refreshed_at` and
/// `repo_auth_ttl` are carried alongside it so [`SessionIdentity::is_stale`]
/// needs no extra `AuthRuntime`/session lookup from its callers.
#[derive(Debug)]
pub struct SessionIdentity {
    pub github_login: String,
    pub repo_ids: HashSet<RepoId>,
    pub repos_refreshed_at: chrono::DateTime<chrono::Utc>,
    pub repo_auth_ttl: Duration,
}

impl AuthContext {
    /// `Disabled` sees everything (today's behavior, unfiltered). A
    /// `Session` sees a repo only if it's in its authorized set — `None`
    /// (no `repo_id`, e.g. a pre-migration row) is never visible to an
    /// authenticated session.
    pub fn can_see(&self, repo_id: Option<RepoId>) -> bool {
        match self {
            Self::Disabled => true,
            Self::Session(s) => repo_id.is_some_and(|id| s.repo_ids.contains(&id)),
        }
    }

    /// For the read rails (`/v1/state`, `/v1/ws` — #459/#460), which fail
    /// closed on staleness rather than reporting it the way
    /// `whoami_handler` does: `Disabled` passes through unchanged; a
    /// `Session` is rejected with `AuthRejection::Stale` once
    /// `repos_refreshed_at + repo_auth_ttl` has elapsed. `surface` names the
    /// caller's route (`"state"` / `"ws"`) for the rejection's trace/metric.
    pub fn require_fresh(
        self,
        now: chrono::DateTime<chrono::Utc>,
        surface: &'static str,
    ) -> Result<Self, AuthRejection> {
        if let Self::Session(identity) = &self
            && identity.is_stale(now)
        {
            return Err(AuthRejection::Stale { surface });
        }
        Ok(self)
    }
}

impl SessionIdentity {
    /// An absurdly large configured `repo_auth_ttl` (well past chrono's
    /// representable range) is treated as "never stale" rather than a panic
    /// on a request path — a config-typo edge case, not a real deployment.
    /// Compared as elapsed-vs-ttl (`DateTime - DateTime`, always in-range
    /// since both are real timestamps), not `repos_refreshed_at + ttl` —
    /// adding an out-of-range `Duration::MAX` to a `DateTime` panics on
    /// overflow (chrono's `Add` impl), which is exactly the panic this
    /// guards against.
    pub fn is_stale(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let ttl = chrono::Duration::from_std(self.repo_auth_ttl).unwrap_or(chrono::Duration::MAX);
        now - self.repos_refreshed_at >= ttl
    }
}

/// The 401 reason contract, defined once so `/v1/auth/me` and the read
/// rails (#459/#460) never diverge on the JSON shape. These two strings are
/// the wire contract consumed by #462 — string-exact.
///
/// Tracing fires from [`IntoResponse::into_response`] rather than at each
/// call site, so both the extractor's automatic `Required` rejection and a
/// handler's manual `Stale` check (#459/#460) trace uniformly.
#[derive(Debug)]
pub enum AuthRejection {
    /// No/invalid/expired session. `had_cookie` distinguishes "no cookie
    /// sent" from "cookie sent but unknown/expired" for the trace event —
    /// never reflected in the response body. `surface` names the mounted
    /// route that rejected (`"state"` / `"ws"` / `"me"`).
    Required {
        had_cookie: bool,
        surface: &'static str,
    },
    /// Valid session, but `repos_refreshed_at + repo_auth_ttl` elapsed.
    Stale { surface: &'static str },
    /// `SessionStore` failure while loading the session.
    StoreFailed,
}

/// Build a `401` with `{"reason": reason}` — the shared shape both
/// [`AuthRejection`] 401 variants render.
fn unauthorized(reason: &'static str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"reason": reason})),
    )
        .into_response()
}

impl IntoResponse for AuthRejection {
    fn into_response(self) -> Response {
        match self {
            Self::Required {
                had_cookie,
                surface,
            } => {
                tracing::debug!(
                    reason = "auth_required",
                    had_cookie,
                    surface,
                    "request rejected: no valid session"
                );
                unauthorized("auth_required")
            }
            Self::Stale { surface } => {
                tracing::debug!(
                    reason = "stale_authorization",
                    had_cookie = true,
                    surface,
                    "request rejected: stale authorization"
                );
                unauthorized("stale_authorization")
            }
            // Same shape as `session_store_failed`'s plain-text 500 (used
            // by login/callback/logout) — `what` is always "load session"
            // here, so the body is hardcoded rather than re-deriving it.
            Self::StoreFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, "failed to load session").into_response()
            }
        }
    }
}

impl FromRequestParts<Arc<AppState>> for AuthContext {
    type Rejection = AuthRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(auth) = state.auth.as_ref() else {
            return Ok(Self::Disabled);
        };
        // Which mounted route rejected — distinguishes otherwise-identical
        // `auth_required`/`stale_authorization` rejections across
        // `/v1/state`, `/v1/ws`, and `/v1/auth/me` for the trace/metric.
        let surface = match parts.uri.path() {
            "/v1/state" => "state",
            "/v1/ws" => "ws",
            "/v1/auth/me" => "me",
            _ => "unknown",
        };

        let names = cookie_names(&auth.public_origin);
        let raw_cookie = get_cookie(&parts.headers, names.session);
        let had_cookie = raw_cookie.is_some();

        let session = session_from_cookie(auth, raw_cookie.as_deref())
            .await
            .map_err(|e| {
                tracing::warn!(
                    reason = "session_error",
                    error.message = %e,
                    what = "load session",
                    surface,
                    "auth: session store operation failed"
                );
                AuthRejection::StoreFailed
            })?
            .ok_or_else(|| {
                state
                    .auth_metrics
                    .record_rejection(surface, "auth_required");
                AuthRejection::Required {
                    had_cookie,
                    surface,
                }
            })?;

        Ok(Self::Session(SessionIdentity {
            github_login: session.github_login,
            repo_ids: session.repo_ids.into_iter().map(RepoId).collect(),
            repos_refreshed_at: session.repos_refreshed_at,
            repo_auth_ttl: auth.repo_auth_ttl,
        }))
    }
}

// ---------------------------------------------------------------------------
// GET /v1/auth/me
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
struct WhoamiResponse {
    login: String,
    repo_count: usize,
    repos_refreshed_at: chrono::DateTime<chrono::Utc>,
    stale: bool,
}

async fn whoami_handler(ctx: AuthContext, State(state): State<Arc<AppState>>) -> Response {
    let AuthContext::Session(identity) = ctx else {
        // Unreachable in practice — this route is only mounted when
        // `auth.mode = "github"` (see `routes::api_routes`), which is
        // exactly when `AuthContext` never produces `Disabled`. Mirrors the
        // defensive 404 the sibling handlers in this module use.
        return StatusCode::NOT_FOUND.into_response();
    };

    let stale = identity.is_stale(state.clock.now());

    Json(WhoamiResponse {
        login: identity.github_login,
        repo_count: identity.repo_ids.len(),
        repos_refreshed_at: identity.repos_refreshed_at,
        stale,
    })
    .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_names_uses_host_prefix_for_https_origin() {
        let names = cookie_names("https://atc.example.com");
        assert_eq!(names.flow, "__Host-atc_flow");
        assert_eq!(names.session, "__Host-atc_session");
    }

    #[test]
    fn cookie_names_uses_plain_names_for_http_origin() {
        let names = cookie_names("http://localhost:8080");
        assert_eq!(names.flow, "atc_flow");
        assert_eq!(names.session, "atc_session");
    }

    #[test]
    fn set_cookie_header_includes_secure_when_requested() {
        let cookie = set_cookie_header("atc_session", "abc123", 3600, true);
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));
        assert!(cookie.contains("Max-Age=3600"));
    }

    #[test]
    fn set_cookie_header_omits_secure_for_dev() {
        let cookie = set_cookie_header("atc_session", "abc123", 3600, false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn get_cookie_finds_named_value_among_several() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; atc_flow=xyz; b=2".parse().unwrap());
        assert_eq!(get_cookie(&headers, "atc_flow"), Some("xyz".to_string()));
    }

    #[test]
    fn get_cookie_returns_none_when_absent() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; b=2".parse().unwrap());
        assert_eq!(get_cookie(&headers, "atc_flow"), None);
    }

    #[test]
    fn validate_return_to_accepts_relative_path() {
        assert_eq!(validate_return_to(Some("/dashboard")), "/dashboard");
    }

    #[test]
    fn validate_return_to_rejects_absolute_url() {
        assert_eq!(validate_return_to(Some("https://evil.example.com")), "/");
    }

    #[test]
    fn validate_return_to_rejects_scheme_relative_url() {
        assert_eq!(validate_return_to(Some("//evil.example.com")), "/");
    }

    #[test]
    fn validate_return_to_defaults_when_absent() {
        assert_eq!(validate_return_to(None), "/");
    }

    #[test]
    fn pkce_challenge_is_deterministic_and_url_safe() {
        let challenge = pkce_challenge("some-verifier-value");
        assert_eq!(challenge, pkce_challenge("some-verifier-value"));
        assert!(!challenge.contains('='), "no base64 padding");
        assert!(
            challenge
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn popup_callback_html_contains_exact_channel_contract() {
        assert!(POPUP_CALLBACK_HTML.contains("new BroadcastChannel('atc-auth')"));
        assert!(POPUP_CALLBACK_HTML.contains("postMessage('session-refreshed')"));
        assert!(POPUP_CALLBACK_HTML.contains("window.close()"));
    }

    fn test_identity(
        repos_refreshed_at: chrono::DateTime<chrono::Utc>,
        repo_auth_ttl: Duration,
    ) -> SessionIdentity {
        SessionIdentity {
            github_login: "octocat".to_string(),
            repo_ids: HashSet::from([RepoId(1), RepoId(2)]),
            repos_refreshed_at,
            repo_auth_ttl,
        }
    }

    #[test]
    fn can_see_disabled_sees_every_repo_including_none() {
        let ctx = AuthContext::Disabled;
        assert!(ctx.can_see(Some(RepoId(1))));
        assert!(ctx.can_see(Some(RepoId(999))));
        assert!(ctx.can_see(None));
    }

    #[test]
    fn can_see_session_checks_repo_id_membership() {
        let refreshed_at = chrono::DateTime::from_timestamp(0, 0).unwrap();
        let ctx = AuthContext::Session(test_identity(refreshed_at, Duration::from_secs(3600)));
        assert!(ctx.can_see(Some(RepoId(1))));
        assert!(!ctx.can_see(Some(RepoId(999))), "repo outside the set");
        assert!(
            !ctx.can_see(None),
            "no repo_id (e.g. a pre-migration row) is never visible to an authenticated session"
        );
    }

    #[test]
    fn is_stale_false_within_ttl() {
        let refreshed_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let identity = test_identity(refreshed_at, Duration::from_secs(3600));
        let now = refreshed_at + chrono::Duration::seconds(30);
        assert!(!identity.is_stale(now));
    }

    #[test]
    fn is_stale_true_once_ttl_elapsed() {
        let refreshed_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let identity = test_identity(refreshed_at, Duration::from_secs(3600));
        let now = refreshed_at + chrono::Duration::hours(2);
        assert!(identity.is_stale(now));
    }

    /// Regression test: an operator config with a `repo_auth_ttl` past
    /// chrono's representable range must report "never stale", not panic.
    /// `repos_refreshed_at + ttl` would overflow chrono's `DateTime +
    /// Duration` (which panics rather than saturating); `is_stale` computes
    /// `elapsed >= ttl` instead, which never adds an out-of-range `Duration`
    /// to a `DateTime`.
    #[test]
    fn is_stale_absurd_ttl_never_stale_without_panicking() {
        let refreshed_at = chrono::DateTime::from_timestamp(0, 0).expect("valid timestamp");
        let identity = test_identity(refreshed_at, Duration::from_secs(u64::MAX));
        assert!(!identity.is_stale(refreshed_at));
    }

    #[test]
    fn require_fresh_disabled_passes_through() {
        let ctx = AuthContext::Disabled;
        let now = chrono::DateTime::from_timestamp(0, 0).unwrap();
        assert!(matches!(
            ctx.require_fresh(now, "state"),
            Ok(AuthContext::Disabled)
        ));
    }

    #[test]
    fn require_fresh_fresh_session_passes_through() {
        let refreshed_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let ctx = AuthContext::Session(test_identity(refreshed_at, Duration::from_secs(3600)));
        let now = refreshed_at + chrono::Duration::seconds(30);
        assert!(matches!(
            ctx.require_fresh(now, "state"),
            Ok(AuthContext::Session(_))
        ));
    }

    #[test]
    fn require_fresh_stale_session_rejects() {
        let refreshed_at = chrono::DateTime::from_timestamp(1_000, 0).unwrap();
        let ctx = AuthContext::Session(test_identity(refreshed_at, Duration::from_secs(3600)));
        let now = refreshed_at + chrono::Duration::hours(2);
        assert!(matches!(
            ctx.require_fresh(now, "state"),
            Err(AuthRejection::Stale { surface: "state" })
        ));
    }

    #[tokio::test]
    async fn auth_rejection_required_body_is_exact() {
        let resp = AuthRejection::Required {
            had_cookie: false,
            surface: "state",
        }
        .into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"reason": "auth_required"}));
    }

    #[tokio::test]
    async fn auth_rejection_stale_body_is_exact() {
        let resp = AuthRejection::Stale { surface: "state" }.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"reason": "stale_authorization"}));
    }

    /// `StoreFailed` mirrors `session_store_failed`'s plain-text 500 shape
    /// (used by login/callback/logout) rather than a distinct JSON body, so
    /// there's exactly one 500-on-session-store-failure rendering
    /// convention in this file.
    #[tokio::test]
    async fn auth_rejection_store_failed_matches_session_store_failed_shape() {
        let resp = AuthRejection::StoreFailed.into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body, "failed to load session");
    }
}
