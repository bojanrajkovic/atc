//! OAuth client error type.

/// Errors surfaced by the [`OAuthClient`](super::OAuthClient) and its
/// associated helpers.
///
/// GitHub's OAuth token endpoint returns OAuth-level errors as HTTP 200
/// responses with an `error` field in the JSON body (e.g.,
/// `{"error":"bad_verification_code"}`), so this enum distinguishes those
/// protocol errors from transport/parse failures and HTTP-status-coded
/// failures from the regular REST API.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    /// The authorization-code grant was rejected by GitHub. Returned when
    /// the code-exchange endpoint replies with an `error` body whose value
    /// is `bad_verification_code` (or otherwise indicates the code was
    /// invalid, expired, mismatched, or already redeemed).
    #[error("invalid grant: {description}")]
    InvalidGrant {
        /// The `error` field from GitHub's response (e.g.,
        /// `"bad_verification_code"`).
        code: String,
        /// The `error_description` field from GitHub's response, or a
        /// generic message if absent.
        description: String,
    },

    /// The refresh-token grant was rejected. Returned when the refresh
    /// endpoint replies with `error: bad_refresh_token` (or, more
    /// generally, any `error` body — refresh tokens that expire after 6
    /// months of disuse manifest this way). Sessions should be deleted on
    /// this error per the auth design.
    #[error("refresh token expired or rejected: {description}")]
    RefreshExpired {
        /// The `error` field from GitHub's response.
        code: String,
        /// The `error_description` field from GitHub's response, or a
        /// generic message if absent.
        description: String,
    },

    /// A bearer-token request returned 401. The token is no longer valid;
    /// callers should attempt a refresh and retry, or surface logout.
    #[error("unauthenticated: 401 from GitHub")]
    Unauthenticated,

    /// GitHub returned a rate-limit response (HTTP 403 or 429 with
    /// rate-limit headers). The wrapped string carries the value of the
    /// `X-RateLimit-Reset` header when present, otherwise the response
    /// status text.
    #[error("rate limited by GitHub: {0}")]
    RateLimited(String),

    /// Any other failure: transport errors, JSON deserialization,
    /// unexpected response shape, or non-401/403/429 HTTP statuses with no
    /// recognized `error` body.
    #[error("oauth client error: {0}")]
    Other(String),
}

impl From<reqwest::Error> for OAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::Other(err.to_string())
    }
}

impl From<serde_json::Error> for OAuthError {
    fn from(err: serde_json::Error) -> Self {
        Self::Other(format!("json parse: {err}"))
    }
}
