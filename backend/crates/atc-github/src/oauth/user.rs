//! `GET /user` — fetch the GitHub user identified by a bearer token.

use super::OAuthClient;
use super::errors::OAuthError;
use serde::Deserialize;

/// Subset of GitHub's user object that ATC consumes during the OAuth flow.
///
/// Additional fields from GitHub's response are silently ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct UserInfo {
    /// GitHub's stable numeric user id.
    pub id: u64,
    /// The user's login handle (e.g., `"octocat"`).
    pub login: String,
    /// Display name as configured on the GitHub profile. May be null on
    /// GitHub, in which case this is `None`.
    #[serde(default)]
    pub name: Option<String>,
    /// URL to the user's avatar image.
    pub avatar_url: String,
}

impl OAuthClient {
    /// Fetch the authenticated user via `GET /user`.
    ///
    /// # Errors
    ///
    /// - [`OAuthError::Unauthenticated`] if GitHub returns HTTP 401.
    /// - [`OAuthError::RateLimited`] for HTTP 403/429.
    /// - [`OAuthError::Other`] for transport or deserialization failures
    ///   or unexpected HTTP statuses.
    pub async fn get_user(&self, access_token: &str) -> Result<UserInfo, OAuthError> {
        let url = format!("{}/user", self.api_base());
        let resp = self
            .http()
            .get(&url)
            .bearer_auth(access_token)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header(reqwest::header::USER_AGENT, super::USER_AGENT)
            .send()
            .await?;

        super::check_api_status(&resp)?;
        let body = resp.text().await?;
        let user: UserInfo = serde_json::from_str(&body)?;
        Ok(user)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::testing::test_client;

    #[tokio::test]
    async fn get_user_happy_path_returns_user_info() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/user")
            .match_header("authorization", "Bearer access-abc")
            .match_header("accept", "application/vnd.github+json")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"id": 42, "login": "octocat", "name": "Octo Cat", "avatar_url": "https://example.invalid/a.png"}"#,
            )
            .create_async()
            .await;

        let client = test_client(&server);
        let user = client.get_user("access-abc").await.expect("get_user ok");

        assert_eq!(user.id, 42);
        assert_eq!(user.login, "octocat");
        assert_eq!(user.name.as_deref(), Some("Octo Cat"));
        assert_eq!(user.avatar_url, "https://example.invalid/a.png");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn get_user_401_returns_unauthenticated() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/user")
            .with_status(401)
            .with_body(r#"{"message":"Bad credentials"}"#)
            .create_async()
            .await;

        let client = test_client(&server);
        let err = client
            .get_user("stale-token")
            .await
            .expect_err("expected error");

        assert!(matches!(err, OAuthError::Unauthenticated));
    }

    #[tokio::test]
    async fn get_user_handles_null_name() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/user")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"id": 7, "login": "nameless", "name": null, "avatar_url": "https://example.invalid/n.png"}"#,
            )
            .create_async()
            .await;

        let client = test_client(&server);
        let user = client.get_user("t").await.expect("get_user ok");
        assert_eq!(user.name, None);
    }
}
