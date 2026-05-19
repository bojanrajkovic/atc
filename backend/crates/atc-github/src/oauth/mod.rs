//! GitHub OAuth client for user-to-server tokens (PKCE flow).
//!
//! Provides a thin wrapper over GitHub's OAuth code-exchange and
//! refresh-token endpoints plus the user-token-scoped API endpoints
//! `GET /user`, `GET /user/installations`, and
//! `GET /user/installations/{id}/repositories`.
//!
//! The GitHub App backing this code must have **expiring user tokens
//! enabled**: access tokens last ~8h and refresh tokens last ~6 months of
//! disuse, and the refresh endpoint rotates both. Without that
//! registration setting, this module's refresh path is dead code.
//!
//! This module owns no global HTTP client. Callers construct an
//! [`OAuthClient`] with a shared [`reqwest::Client`] so connection pooling
//! is reused across the application.
//!
//! ## Token-endpoint quirk
//!
//! GitHub's OAuth token endpoint returns OAuth-level errors as HTTP 200
//! responses whose JSON body carries an `error` field
//! (e.g., `{"error":"bad_verification_code"}`). The success and error
//! shapes are therefore mutually exclusive on the same status code, and
//! the exchange / refresh paths inspect the body before treating it as a
//! success.

mod errors;
mod installations;
mod user;

pub use errors::OAuthError;
pub use installations::{Installation, Repository};
pub use user::UserInfo;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// `User-Agent` value sent with every request. GitHub's API rejects
/// requests without a UA.
const USER_AGENT: &str = concat!("atc-github/", env!("CARGO_PKG_VERSION"));

/// Default OAuth web-flow base URL.
const DEFAULT_OAUTH_BASE: &str = "https://github.com";

/// Default GitHub REST API base URL.
const DEFAULT_API_BASE: &str = "https://api.github.com";

/// A pair of access + refresh tokens issued (or rotated) by GitHub's
/// token endpoint.
#[derive(Debug, Clone)]
pub struct TokenSet {
    /// User-to-server access token. Expires in `access_token_expires_in`
    /// seconds (typically 28800 = 8h).
    pub access_token: String,
    /// Refresh token. Valid for 6 months of disuse; refreshing rotates
    /// this value, so callers must persist the new value on every
    /// refresh.
    pub refresh_token: String,
    /// Lifetime of `access_token` in seconds, as returned by GitHub.
    pub access_token_expires_in: u64,
    /// Lifetime of `refresh_token` in seconds, as returned by GitHub.
    pub refresh_token_expires_in: u64,
}

/// A PKCE challenge pair.
///
/// The `verifier` must be retained by the caller (typically in an
/// `HttpOnly` state cookie) and replayed during the code-exchange request.
/// The `challenge` is sent to GitHub at the start of the authorize flow
/// alongside `code_challenge_method=S256`.
#[derive(Debug, Clone)]
pub struct PkcePair {
    /// The PKCE code verifier. 64 bytes of randomness, base64url-encoded
    /// (no padding) — 86 characters, well within RFC 7636's 43–128 range.
    pub verifier: String,
    /// The PKCE code challenge: `BASE64URL(SHA256(verifier))`, no padding.
    pub challenge: String,
}

/// Generate a PKCE `(verifier, challenge)` pair with method `S256`.
///
/// The verifier is 64 random bytes encoded as base64url-without-padding
/// (the encoding produces ~86 characters, well within RFC 7636's
/// 43–128-character range). The challenge is
/// `base64url(sha256(verifier))`.
#[must_use]
pub fn generate_pkce_pair() -> PkcePair {
    let mut raw = [0u8; 64];
    rand::fill(&mut raw);
    let verifier = URL_SAFE_NO_PAD.encode(raw);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    PkcePair {
        verifier,
        challenge,
    }
}

/// OAuth client bound to a base URL pair and a shared HTTP client.
///
/// Production callers construct one with [`OAuthClient::new`], passing a
/// [`reqwest::Client`] they already own so connection pools are shared.
/// Tests use [`OAuthClient::with_bases`] to point the client at a
/// mockito-served origin.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    http: reqwest::Client,
    oauth_base: String,
    api_base: String,
    client_id: String,
    client_secret: String,
}

impl OAuthClient {
    /// Construct a client targeting the real GitHub endpoints.
    #[must_use]
    pub fn new(http: reqwest::Client, client_id: String, client_secret: String) -> Self {
        Self::with_bases(
            http,
            client_id,
            client_secret,
            DEFAULT_OAUTH_BASE.to_string(),
            DEFAULT_API_BASE.to_string(),
        )
    }

    /// Construct a client with explicit OAuth and API base URLs. Used by
    /// tests and any deployment fronting a GitHub Enterprise Server
    /// instance.
    #[must_use]
    pub fn with_bases(
        http: reqwest::Client,
        client_id: String,
        client_secret: String,
        oauth_base: String,
        api_base: String,
    ) -> Self {
        Self {
            http,
            oauth_base,
            api_base,
            client_id,
            client_secret,
        }
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub(crate) fn api_base(&self) -> &str {
        &self.api_base
    }

    /// Exchange an authorization code for an access + refresh token pair.
    ///
    /// # Errors
    ///
    /// - [`OAuthError::InvalidGrant`] if GitHub returns an OAuth error
    ///   body (HTTP 200 with `{"error":"..."}`).
    /// - [`OAuthError::Other`] for transport, deserialization, or
    ///   unexpected response shape.
    pub async fn exchange_code(
        &self,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, OAuthError> {
        let form = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("code_verifier", code_verifier),
            ("redirect_uri", redirect_uri),
            ("grant_type", "authorization_code"),
        ];
        self.post_token_endpoint(&form, /* refresh */ false).await
    }

    /// Refresh an expired access token using its paired refresh token.
    /// The response rotates both tokens; the caller must persist the new
    /// pair.
    ///
    /// # Errors
    ///
    /// - [`OAuthError::RefreshExpired`] if GitHub returns an OAuth error
    ///   body — the refresh token is no longer valid and the session
    ///   should be deleted.
    /// - [`OAuthError::Other`] for transport, deserialization, or
    ///   unexpected response shape.
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenSet, OAuthError> {
        let form = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];
        self.post_token_endpoint(&form, /* refresh */ true).await
    }

    async fn post_token_endpoint(
        &self,
        form: &[(&str, &str)],
        refresh: bool,
    ) -> Result<TokenSet, OAuthError> {
        let url = format!("{}/login/oauth/access_token", self.oauth_base);
        let resp = self
            .http
            .post(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .form(form)
            .send()
            .await?;

        // GitHub returns OAuth errors as HTTP 200 with an "error" body. Only
        // promote non-200 statuses to transport errors after we've decided
        // the body shape doesn't look like a token response.
        let status = resp.status();
        let body = resp.text().await?;

        let value: serde_json::Value = serde_json::from_str(&body)?;

        if let Some(err_code) = value.get("error").and_then(|v| v.as_str()) {
            let description = value
                .get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("(no description)")
                .to_string();
            let code = err_code.to_string();
            return Err(if refresh {
                OAuthError::RefreshExpired { code, description }
            } else {
                OAuthError::InvalidGrant { code, description }
            });
        }

        if !status.is_success() {
            return Err(OAuthError::Other(format!(
                "github oauth token endpoint returned {status}",
            )));
        }

        let token: TokenResponse = serde_json::from_value(value)?;
        Ok(TokenSet {
            access_token: token.access_token,
            refresh_token: token.refresh_token,
            access_token_expires_in: token.expires_in,
            refresh_token_expires_in: token.refresh_token_expires_in,
        })
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    refresh_token_expires_in: u64,
}

/// Translate an HTTP response status from a REST-style endpoint into the
/// appropriate `OAuthError`. Returns `Ok(())` on 2xx.
fn check_api_status(resp: &reqwest::Response) -> Result<(), OAuthError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(());
    }
    match status.as_u16() {
        401 => Err(OAuthError::Unauthenticated),
        403 | 429 => {
            let reset = resp
                .headers()
                .get("x-ratelimit-reset")
                .and_then(|v| v.to_str().ok())
                .map_or_else(
                    || {
                        status
                            .canonical_reason()
                            .unwrap_or("rate-limited")
                            .to_string()
                    },
                    str::to_string,
                );
            Err(OAuthError::RateLimited(reset))
        }
        _ => Err(OAuthError::Other(format!("github api returned {status}"))),
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::OAuthClient;

    /// Build an `OAuthClient` pointed at the mockito server's URL for
    /// both OAuth and API operations. The single base mirrors mockito's
    /// single-origin server.
    pub(crate) fn test_client(server: &mockito::ServerGuard) -> OAuthClient {
        let base = server.url();
        OAuthClient::with_bases(
            reqwest::Client::new(),
            "test-client-id".to_string(),
            "test-client-secret".to_string(),
            base.clone(),
            base,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn pkce_pair_challenge_matches_sha256_of_verifier_base64url() {
        let pair = generate_pkce_pair();
        let mut hasher = Sha256::new();
        hasher.update(pair.verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(pair.challenge, expected);
    }

    #[test]
    fn pkce_verifier_is_base64url_no_pad_of_64_random_bytes() {
        let pair = generate_pkce_pair();
        // base64url-no-pad of 64 bytes is ceil(64 * 4 / 3) = 86 chars.
        assert_eq!(pair.verifier.len(), 86);
        // Decoding round-trips; verifies it is well-formed base64url.
        let raw = URL_SAFE_NO_PAD
            .decode(pair.verifier.as_bytes())
            .expect("verifier decodes");
        assert_eq!(raw.len(), 64);
    }

    #[test]
    fn pkce_pairs_are_distinct() {
        // Probability of collision on 64 random bytes is ~0; this guards
        // against accidentally seeding from a constant.
        let a = generate_pkce_pair();
        let b = generate_pkce_pair();
        assert_ne!(a.verifier, b.verifier);
        assert_ne!(a.challenge, b.challenge);
    }

    #[tokio::test]
    async fn exchange_code_happy_path_returns_token_pair() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/login/oauth/access_token")
            .match_header("accept", "application/json")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("client_id".into(), "test-client-id".into()),
                mockito::Matcher::UrlEncoded(
                    "client_secret".into(),
                    "test-client-secret".into(),
                ),
                mockito::Matcher::UrlEncoded("code".into(), "the-code".into()),
                mockito::Matcher::UrlEncoded("code_verifier".into(), "the-verifier".into()),
                mockito::Matcher::UrlEncoded(
                    "redirect_uri".into(),
                    "https://atc.example/cb".into(),
                ),
                mockito::Matcher::UrlEncoded(
                    "grant_type".into(),
                    "authorization_code".into(),
                ),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"a-tok","refresh_token":"r-tok","expires_in":28800,"refresh_token_expires_in":15897600,"token_type":"bearer","scope":""}"#,
            )
            .create_async()
            .await;

        let client = testing::test_client(&server);
        let tokens = client
            .exchange_code("the-code", "the-verifier", "https://atc.example/cb")
            .await
            .expect("exchange ok");

        assert_eq!(tokens.access_token, "a-tok");
        assert_eq!(tokens.refresh_token, "r-tok");
        assert_eq!(tokens.access_token_expires_in, 28800);
        assert_eq!(tokens.refresh_token_expires_in, 15_897_600);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn exchange_code_invalid_grant_returns_invalid_grant_error() {
        // GitHub returns HTTP 200 with `error` field for invalid grants.
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/login/oauth/access_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired.","error_uri":"https://docs.github.com"}"#,
            )
            .create_async()
            .await;

        let client = testing::test_client(&server);
        let err = client
            .exchange_code("bad-code", "v", "https://atc.example/cb")
            .await
            .expect_err("should reject");

        match err {
            OAuthError::InvalidGrant { code, description } => {
                assert_eq!(code, "bad_verification_code");
                assert!(description.contains("incorrect or expired"));
            }
            other => panic!("expected InvalidGrant, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn refresh_token_happy_path_rotates_both_tokens() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/login/oauth/access_token")
            .match_body(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
                mockito::Matcher::UrlEncoded("refresh_token".into(), "old-refresh".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"new-access","refresh_token":"new-refresh","expires_in":28800,"refresh_token_expires_in":15897600,"token_type":"bearer","scope":""}"#,
            )
            .create_async()
            .await;

        let client = testing::test_client(&server);
        let tokens = client
            .refresh_token("old-refresh")
            .await
            .expect("refresh ok");

        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "new-refresh");
        assert_ne!(tokens.refresh_token, "old-refresh", "refresh must rotate");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn refresh_token_invalid_grant_returns_refresh_expired() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("POST", "/login/oauth/access_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"error":"bad_refresh_token","error_description":"The refresh token passed is incorrect or expired."}"#,
            )
            .create_async()
            .await;

        let client = testing::test_client(&server);
        let err = client
            .refresh_token("expired-refresh")
            .await
            .expect_err("should reject");

        match err {
            OAuthError::RefreshExpired { code, description } => {
                assert_eq!(code, "bad_refresh_token");
                assert!(description.contains("incorrect or expired"));
            }
            other => panic!("expected RefreshExpired, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn exchange_code_sends_application_json_accept_and_form_body() {
        // Tightening header / body invariant: GitHub returns
        // x-www-form-urlencoded by default; we must opt into JSON.
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/login/oauth/access_token")
            .match_header("accept", "application/json")
            .match_header(
                "content-type",
                mockito::Matcher::Regex("application/x-www-form-urlencoded".into()),
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"a","refresh_token":"r","expires_in":1,"refresh_token_expires_in":1,"token_type":"bearer","scope":""}"#,
            )
            .create_async()
            .await;

        let client = testing::test_client(&server);
        client
            .exchange_code("c", "v", "https://atc.example/cb")
            .await
            .expect("ok");
        mock.assert_async().await;
    }
}
