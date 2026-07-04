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

use reqwest::header::{ACCEPT, LINK};

const API_VERSION: &str = "2022-11-28";

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
            http: reqwest::Client::new(),
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
    pub async fn get_authorized_repo_ids(
        &self,
        access_token: &str,
    ) -> Result<Vec<i64>, GitHubClientError> {
        let installations: Vec<InstallationItem> = self
            .fetch_all_pages(
                format!("{}/user/installations?per_page=100", self.api_base),
                access_token,
                |page: InstallationsResponse| page.installations,
            )
            .await?;

        let mut repo_ids = Vec::new();
        for installation in installations {
            let repos: Vec<RepositoryItem> = self
                .fetch_all_pages(
                    format!(
                        "{}/user/installations/{}/repositories?per_page=100",
                        self.api_base, installation.id
                    ),
                    access_token,
                    |page: RepositoriesResponse| page.repositories,
                )
                .await?;
            repo_ids.extend(repos.into_iter().map(|r| r.id));
        }

        Ok(repo_ids)
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
    /// decoding each page as `R` and flattening via `extract`.
    async fn fetch_all_pages<T, R, F>(
        &self,
        mut url: String,
        access_token: &str,
        extract: F,
    ) -> Result<Vec<T>, GitHubClientError>
    where
        R: serde::de::DeserializeOwned,
        F: Fn(R) -> Vec<T>,
    {
        let mut items = Vec::new();
        for _ in 0..MAX_PAGES {
            let resp = self.rest_get(&url, access_token).await?;
            let next = next_page_url(resp.headers());
            let page: R = resp.json().await.map_err(GitHubClientError::Http)?;
            items.extend(extract(page));
            match next {
                Some(next_url) => url = next_url,
                None => return Ok(items),
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
}
