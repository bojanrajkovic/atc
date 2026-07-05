//! GitHub OAuth token exchange + REST API client for `auth.github` mode.
//!
//! Base URLs are constructor parameters, not hardcoded, so tests can point
//! this at a local mock server instead of the real `github.com`/`api.github.com`.
//!
//! **No token is ever returned to a caller beyond this module's own
//! functions.** `exchange_code` returns the bare access-token string, used
//! transiently by the caller for the identity/repo-set calls that follow in
//! the same request and then dropped — never persisted, logged, or placed
//! in a trace/error context (ADR-0014).

use std::collections::HashSet;

use reqwest::header::{ACCEPT, LINK};

const API_VERSION: &str = "2022-11-28";

/// GitHub's REST API rejects any request with no `User-Agent` header (403
/// Forbidden) — see
/// <https://docs.github.com/en/rest/using-the-rest-api/getting-started-with-the-rest-api#user-agent-required>.
/// `reqwest::Client::new()` sends none by default, so it must be set
/// explicitly on the shared client rather than per-call.
const USER_AGENT: &str = concat!("atc-server/", env!("CARGO_PKG_VERSION"));

/// Hard ceiling on pages [`GitHubClient::fetch_all_pages`] will follow.
/// GitHub's real pagination always terminates, but a malformed or
/// misbehaving response (a `Link: rel="next"` that cycles back to an
/// already-fetched page) would otherwise loop forever inside a request
/// handler. 500 pages (up to 100 items each) is far beyond any legitimate
/// installation/repository count; hitting it means something is wrong with
/// the response, not that the user has a lot of repos.
const MAX_PAGES: u32 = 500;

/// A GitHub identity: `id` and `login` only. Neither is sensitive — `login`
/// is stored as display data (see the design doc's "Data model" section).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GitHubUser {
    pub id: i64,
    pub login: String,
}

/// Errors from a GitHub OAuth/REST call. Never carries token material —
/// `TokenExchange`'s `String` is GitHub's `error_description`, not the
/// request/response body.
#[derive(Debug)]
pub enum GitHubClientError {
    /// Transport-level failure (connect, timeout, TLS, decode).
    Http(reqwest::Error),
    /// The token-exchange endpoint returned `{"error": "...", ...}` — GitHub
    /// uses 200 OK for this, so callers must check the body, not just the
    /// status.
    TokenExchange(String),
    /// A REST call returned a non-2xx status.
    UnexpectedStatus(reqwest::StatusCode),
    /// Pagination exceeded [`MAX_PAGES`] — a malformed or cycling `Link`
    /// header, not a legitimately large result set.
    TooManyPages,
}

impl std::fmt::Display for GitHubClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "GitHub request failed: {e}"),
            Self::TokenExchange(desc) => write!(f, "GitHub token exchange failed: {desc}"),
            Self::UnexpectedStatus(status) => {
                write!(f, "GitHub returned unexpected status {status}")
            }
            Self::TooManyPages => write!(f, "GitHub pagination exceeded {MAX_PAGES} pages"),
        }
    }
}

impl std::error::Error for GitHubClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(e) => Some(e),
            Self::TokenExchange(_) | Self::UnexpectedStatus(_) | Self::TooManyPages => None,
        }
    }
}

/// GitHub's token-exchange response. Manual `Debug` redacts both token
/// fields — see the same discipline in `atc-store-pg::session`'s
/// `AuthGitHubConfig`/`GitHubConfig` redaction.
#[derive(serde::Deserialize)]
struct TokenExchangeResponse {
    access_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

impl std::fmt::Debug for TokenExchangeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenExchangeResponse")
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("error", &self.error)
            .field("error_description", &self.error_description)
            .finish()
    }
}

#[derive(serde::Deserialize)]
struct InstallationsResponse {
    installations: Vec<InstallationItem>,
}

#[derive(serde::Deserialize)]
struct InstallationItem {
    id: i64,
}

#[derive(serde::Deserialize)]
struct RepositoriesResponse {
    repositories: Vec<RepositoryItem>,
}

#[derive(serde::Deserialize)]
struct RepositoryItem {
    id: i64,
}

/// `GET /repositories/{id}` response — only the field
/// [`GitHubClient::is_repo_public`] needs. `visibility` is `"public"`,
/// `"private"`, or `"internal"` (GitHub Enterprise org-wide, not the public
/// internet) — deliberately not the `private: bool` field, which is `true`
/// for both `"private"` and `"internal"`.
#[derive(serde::Deserialize)]
struct RepoVisibilityResponse {
    visibility: String,
}

/// Cheaply cloneable — `reqwest::Client` is `Arc`-backed internally, and the
/// remaining fields are small strings — so [`GitHubClient::fetch_public_repo_ids`]
/// can hand each spawned check task its own owned copy.
#[derive(Clone)]
pub struct GitHubClient {
    http: reqwest::Client,
    client_id: String,
    client_secret: String,
    oauth_base: String,
    api_base: String,
}

impl GitHubClient {
    /// Construct a client against the real `github.com`/`api.github.com`.
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::with_base_urls(
            client_id,
            client_secret,
            "https://github.com".to_string(),
            "https://api.github.com".to_string(),
        )
    }

    /// Construct a client against arbitrary base URLs — the seam tests use
    /// to point at a local mock server.
    pub fn with_base_urls(
        client_id: String,
        client_secret: String,
        oauth_base: String,
        api_base: String,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .build()
                .expect("reqwest client builder should not fail for a static user agent"),
            client_id,
            client_secret,
            oauth_base,
            api_base,
        }
    }

    /// The configured GitHub App client id — needed by the login handler to
    /// build the `authorize` redirect URL (a client_id is not sensitive;
    /// it's meant to be visible in that URL).
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// Exchange an authorization code for an access token. Returns the bare
    /// token string; the response's `refresh_token` (if present — only
    /// expiring-token apps return one) is decoded, never propagated, and
    /// dropped when this function returns.
    #[tracing::instrument(name = "auth.callback.exchange", skip_all)]
    pub async fn exchange_code(
        &self,
        code: &str,
        redirect_uri: &str,
        code_verifier: &str,
    ) -> Result<String, GitHubClientError> {
        let resp = self
            .http
            .post(format!("{}/login/oauth/access_token", self.oauth_base))
            .header(ACCEPT, "application/json")
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("code", code),
                ("redirect_uri", redirect_uri),
                ("code_verifier", code_verifier),
            ])
            .send()
            .await
            .map_err(GitHubClientError::Http)?;

        // GitHub returns 200 OK even for a rejected exchange (bad code,
        // mismatched redirect_uri, ...) — the failure shows up as an
        // `error` field in an otherwise-200 body, not as an HTTP error
        // status. Check the body regardless of status.
        let body: TokenExchangeResponse = resp.json().await.map_err(GitHubClientError::Http)?;

        if let Some(error) = body.error {
            return Err(GitHubClientError::TokenExchange(
                body.error_description.unwrap_or(error),
            ));
        }

        body.access_token.ok_or_else(|| {
            GitHubClientError::TokenExchange("no access_token in response".to_string())
        })
    }

    /// `GET /user` — the authenticated identity.
    pub async fn get_user(&self, access_token: &str) -> Result<GitHubUser, GitHubClientError> {
        let resp = self
            .rest_get(&format!("{}/user", self.api_base), access_token)
            .await?;
        resp.json().await.map_err(GitHubClientError::Http)
    }

    /// The full set of repository IDs this user can access through the
    /// metadata-only auth app: every installation the app∩user intersection
    /// includes, across every page of both the installations list and each
    /// installation's repositories list.
    #[tracing::instrument(
        name = "auth.callback.repos",
        skip_all,
        fields(pages = tracing::field::Empty),
    )]
    pub async fn get_authorized_repo_ids(
        &self,
        access_token: &str,
    ) -> Result<Vec<i64>, GitHubClientError> {
        let (installations, mut pages): (Vec<InstallationItem>, u32) = self
            .fetch_all_pages(
                format!("{}/user/installations?per_page=100", self.api_base),
                access_token,
                |page: InstallationsResponse| page.installations,
            )
            .await?;

        let mut repo_ids = Vec::new();
        for installation in installations {
            let (repos, repo_pages): (Vec<RepositoryItem>, u32) = self
                .fetch_all_pages(
                    format!(
                        "{}/user/installations/{}/repositories?per_page=100",
                        self.api_base, installation.id
                    ),
                    access_token,
                    |page: RepositoriesResponse| page.repositories,
                )
                .await?;
            pages += repo_pages;
            repo_ids.extend(repos.into_iter().map(|r| r.id));
        }

        tracing::Span::current().record("pages", pages);
        Ok(repo_ids)
    }

    /// The subset of `repo_ids` that are publicly-visible GitHub repositories
    /// (`visibility == "public"`, not merely `private == false` — GitHub
    /// Enterprise "internal" repos report `private: true`, so `visibility`
    /// is the only field that actually distinguishes public from
    /// org-internal). Checked directly against GitHub rather than inferred
    /// from login-app installation: a public repository is readable by
    /// anyone regardless of whether the login app is installed on its owner
    /// (ADR-0014, decision 2).
    ///
    /// Best-effort per repo: a failed check (network error, unexpected
    /// status) is logged and excluded from the result rather than failing
    /// the whole batch — one flaky repo must not suppress every other
    /// repo's already-known public status this cycle. Checks run fully
    /// concurrently — bounded only by how many repos ATC has run data for,
    /// which is realistically dozens, not the scale where GitHub's
    /// abuse-detection would care about burst size.
    pub async fn fetch_public_repo_ids(&self, repo_ids: &[i64]) -> HashSet<i64> {
        let mut handles = Vec::with_capacity(repo_ids.len());
        for &repo_id in repo_ids {
            let client = self.clone();
            handles.push(tokio::spawn(async move {
                (repo_id, client.is_repo_public(repo_id).await)
            }));
        }

        let mut public = HashSet::new();
        for handle in handles {
            match handle.await {
                Ok((repo_id, Ok(true))) => {
                    public.insert(repo_id);
                }
                Ok((_, Ok(false))) => {}
                Ok((repo_id, Err(e))) => {
                    tracing::warn!(
                        repo_id,
                        error.message = %e,
                        "public-repo visibility check failed; excluding from this cycle"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error.message = %e,
                        "public-repo visibility check task panicked"
                    );
                }
            }
        }
        public
    }

    /// Unauthenticated `GET /repositories/{repo_id}` — deliberately no auth
    /// header. The `client_id`/`client_secret` Basic-auth rate-limit boost
    /// (5,000/hr instead of the unauthenticated 60/hr-per-IP limit) is an
    /// OAuth-App-only mechanism; GitHub Apps have no equivalent and GitHub
    /// rejects it with a flat 401 regardless of which repo is targeted. A
    /// GitHub App installation token would authenticate correctly, but only
    /// for repos the app is installed on — which defeats the point, since
    /// this check exists precisely to catch public repos the app is *not*
    /// installed on (ADR-0014, decision 2). So: no auth at all, same as any
    /// anonymous caller. `Ok(false)` on 404 — GitHub returns 404 (never 403)
    /// for both a private repo and one that no longer exists, and the caller
    /// only needs "is it public", not which.
    async fn is_repo_public(&self, repo_id: i64) -> Result<bool, GitHubClientError> {
        let resp = self
            .http
            .get(format!("{}/repositories/{}", self.api_base, repo_id))
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .map_err(GitHubClientError::Http)?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }
        if !resp.status().is_success() {
            return Err(GitHubClientError::UnexpectedStatus(resp.status()));
        }
        let body: RepoVisibilityResponse = resp.json().await.map_err(GitHubClientError::Http)?;
        Ok(body.visibility == "public")
    }

    async fn rest_get(
        &self,
        url: &str,
        access_token: &str,
    ) -> Result<reqwest::Response, GitHubClientError> {
        let resp = self
            .http
            .get(url)
            .bearer_auth(access_token)
            .header(ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .send()
            .await
            .map_err(GitHubClientError::Http)?;

        if !resp.status().is_success() {
            return Err(GitHubClientError::UnexpectedStatus(resp.status()));
        }
        Ok(resp)
    }

    /// Follow `Link: rel="next"` across every page of a REST list endpoint,
    /// decoding each page as `R` and flattening via `extract`. Returns the
    /// number of pages fetched alongside the flattened items, so callers can
    /// record pagination depth on their own span (see
    /// `auth.callback.repos`'s `pages` attribute).
    async fn fetch_all_pages<T, R, F>(
        &self,
        mut url: String,
        access_token: &str,
        extract: F,
    ) -> Result<(Vec<T>, u32), GitHubClientError>
    where
        R: serde::de::DeserializeOwned,
        F: Fn(R) -> Vec<T>,
    {
        let mut items = Vec::new();
        for page_num in 1..=MAX_PAGES {
            let resp = self.rest_get(&url, access_token).await?;
            let next = next_page_url(resp.headers());
            let page: R = resp.json().await.map_err(GitHubClientError::Http)?;
            items.extend(extract(page));
            match next {
                Some(next_url) => url = next_url,
                None => return Ok((items, page_num)),
            }
        }
        Err(GitHubClientError::TooManyPages)
    }
}

/// Parse the RFC 5988 `Link` response header for a `rel="next"` URL.
fn next_page_url(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let link = headers.get(LINK)?.to_str().ok()?;
    link.split(',').find_map(|entry| {
        let mut parts = entry.split(';').map(str::trim);
        let url_part = parts.next()?;
        let is_next = parts.any(|p| p == r#"rel="next""#);
        is_next.then(|| {
            url_part
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_page_url_finds_rel_next() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            LINK,
            r#"<https://api.github.com/user/installations?page=2>; rel="next", <https://api.github.com/user/installations?page=3>; rel="last""#
                .parse()
                .unwrap(),
        );
        assert_eq!(
            next_page_url(&headers),
            Some("https://api.github.com/user/installations?page=2".to_string())
        );
    }

    #[test]
    fn next_page_url_none_on_last_page() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            LINK,
            r#"<https://api.github.com/user/installations?page=1>; rel="first""#
                .parse()
                .unwrap(),
        );
        assert_eq!(next_page_url(&headers), None);
    }

    #[test]
    fn next_page_url_none_when_header_absent() {
        let headers = reqwest::header::HeaderMap::new();
        assert_eq!(next_page_url(&headers), None);
    }

    #[test]
    fn token_exchange_response_debug_redacts_access_token() {
        let resp = TokenExchangeResponse {
            access_token: Some("ghu_supersecretvalue".to_string()),
            error: None,
            error_description: None,
        };
        let debug = format!("{resp:?}");
        assert!(!debug.contains("ghu_supersecretvalue"));
        assert!(debug.contains("REDACTED"));
    }

    // `fetch_public_repo_ids`'s HTTP-level behavior (public vs. 404,
    // multi-repo batches, Basic auth) is covered by
    // `auth_tests::public_repos` against the full auth flow's existing
    // hand-rolled mock GitHub server — matching this crate's established
    // convention of exercising `GitHubClient`'s HTTP behavior only through
    // that mock, not a second one here (see `next_page_url` and
    // `token_exchange_response_debug_redacts_access_token` above, the only
    // other tests in this module, which test pure functions with no HTTP
    // involved). Only the empty-input short-circuit is worth a unit test:
    // it doesn't touch HTTP at all, so it doesn't need that mock.
    #[tokio::test]
    async fn fetch_public_repo_ids_empty_input_makes_no_calls() {
        let client = GitHubClient::with_base_urls(
            "client-id".to_string(),
            "client-secret".to_string(),
            "http://unused.invalid".to_string(),
            "http://unused.invalid".to_string(),
        );

        let public = client.fetch_public_repo_ids(&[]).await;

        assert!(public.is_empty());
    }
}
