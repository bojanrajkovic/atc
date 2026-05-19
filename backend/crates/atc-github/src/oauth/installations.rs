//! User-token-scoped installation and repository listings.
//!
//! GitHub paginates both endpoints. The fetch helpers follow `Link:
//! <...>; rel="next"` until the header is absent, aggregating every page
//! into a single `Vec`.

use super::OAuthClient;
use super::errors::OAuthError;
use serde::Deserialize;

/// One row of `GET /user/installations`.
///
/// The endpoint returns the installations wrapped under
/// `{"total_count": N, "installations": [...]}`; the wrapper is consumed
/// internally and only this inner shape is exposed.
#[derive(Debug, Clone, Deserialize)]
pub struct Installation {
    /// The numeric installation id assigned by GitHub.
    pub id: u64,
    /// The login of the user or organization that owns the installation
    /// (when available — installations on user accounts may omit this).
    #[serde(default)]
    pub account_login: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationsPage {
    installations: Vec<RawInstallation>,
}

#[derive(Debug, Deserialize)]
struct RawInstallation {
    id: u64,
    #[serde(default)]
    account: Option<Account>,
}

#[derive(Debug, Deserialize)]
struct Account {
    #[serde(default)]
    login: Option<String>,
}

/// One row of `GET /user/installations/{id}/repositories`.
///
/// Additional fields returned by GitHub are silently ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct Repository {
    /// The numeric repository id assigned by GitHub.
    pub id: u64,
    /// The fully-qualified name (`owner/repo`).
    pub full_name: String,
}

#[derive(Debug, Deserialize)]
struct RepositoriesPage {
    repositories: Vec<Repository>,
}

impl OAuthClient {
    /// List every installation visible to the authenticated user, walking
    /// `Link: <...>; rel="next"` pagination until exhausted.
    ///
    /// # Errors
    ///
    /// - [`OAuthError::Unauthenticated`] for HTTP 401.
    /// - [`OAuthError::RateLimited`] for HTTP 403/429.
    /// - [`OAuthError::Other`] for transport or deserialization failures.
    pub async fn list_user_installations(
        &self,
        access_token: &str,
    ) -> Result<Vec<Installation>, OAuthError> {
        let initial = format!("{}/user/installations?per_page=100", self.api_base());
        let pages: Vec<InstallationsPage> = self.fetch_all_pages(access_token, &initial).await?;
        let installations = pages
            .into_iter()
            .flat_map(|page| page.installations)
            .map(|raw| Installation {
                id: raw.id,
                account_login: raw.account.and_then(|a| a.login),
            })
            .collect();
        Ok(installations)
    }

    /// List every repository selected for the given installation under
    /// the authenticated user's token, walking pagination.
    ///
    /// # Errors
    ///
    /// See [`list_user_installations`](OAuthClient::list_user_installations).
    pub async fn list_installation_repositories(
        &self,
        access_token: &str,
        installation_id: u64,
    ) -> Result<Vec<Repository>, OAuthError> {
        let initial = format!(
            "{}/user/installations/{installation_id}/repositories?per_page=100",
            self.api_base()
        );
        let pages: Vec<RepositoriesPage> = self.fetch_all_pages(access_token, &initial).await?;
        let repositories = pages
            .into_iter()
            .flat_map(|page| page.repositories)
            .collect();
        Ok(repositories)
    }

    /// Generic Link-header-driven pagination walker. Fetches `initial_url`,
    /// then follows `rel="next"` until absent. Each page body is JSON-parsed
    /// as `T`.
    async fn fetch_all_pages<T: serde::de::DeserializeOwned>(
        &self,
        access_token: &str,
        initial_url: &str,
    ) -> Result<Vec<T>, OAuthError> {
        let mut pages = Vec::new();
        let mut next_url: Option<String> = Some(initial_url.to_string());

        while let Some(url) = next_url.take() {
            let resp = self
                .http()
                .get(&url)
                .bearer_auth(access_token)
                .header(reqwest::header::ACCEPT, "application/vnd.github+json")
                .header(reqwest::header::USER_AGENT, super::USER_AGENT)
                .send()
                .await?;

            super::check_api_status(&resp)?;

            next_url = parse_next_link(resp.headers());
            let body = resp.text().await?;
            let page: T = serde_json::from_str(&body)?;
            pages.push(page);
        }

        Ok(pages)
    }
}

/// Parse the `Link` response header and return the URL bearing `rel="next"`,
/// if any.
///
/// GitHub's Link headers look like:
/// `<https://api.github.com/...?page=2>; rel="next", <https://api.github.com/...?page=5>; rel="last"`
///
/// The values are commas-separated, but commas inside angle-bracketed URLs
/// must not split entries. This parser scans angle-bracket-balanced segments
/// to be safe.
fn parse_next_link(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let raw = headers.get(reqwest::header::LINK)?.to_str().ok()?;

    for entry in split_link_entries(raw) {
        let entry = entry.trim();
        if !entry.starts_with('<') {
            continue;
        }
        let url_end = entry.find('>')?;
        let url = &entry[1..url_end];
        let rest = &entry[url_end + 1..];
        // rest looks like: `; rel="next"; foo=bar`
        let mut rel: Option<&str> = None;
        for param in rest.split(';') {
            let param = param.trim();
            if let Some(value) = param.strip_prefix("rel=") {
                rel = Some(value.trim_matches('"'));
            }
        }
        if rel == Some("next") {
            return Some(url.to_string());
        }
    }
    None
}

/// Split a Link header value on top-level commas only (commas inside `<...>`
/// URL brackets do not separate entries).
fn split_link_entries(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0;
    let mut in_brackets = false;
    for (idx, ch) in value.char_indices() {
        match ch {
            '<' => in_brackets = true,
            '>' => in_brackets = false,
            ',' if !in_brackets => {
                entries.push(&value[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    entries.push(&value[start..]);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::testing::test_client;

    fn page_of_installations(start_id: u64, count: u64) -> String {
        let items: Vec<String> = (0..count)
            .map(|i| {
                let id = start_id + i;
                format!(r#"{{"id": {id}, "account": {{"login": "user-{id}"}}}}"#)
            })
            .collect();
        format!(
            r#"{{"total_count": {count}, "installations": [{}]}}"#,
            items.join(",")
        )
    }

    fn page_of_repos(start_id: u64, count: u64) -> String {
        let items: Vec<String> = (0..count)
            .map(|i| {
                let id = start_id + i;
                format!(r#"{{"id": {id}, "full_name": "owner/repo-{id}"}}"#)
            })
            .collect();
        format!(
            r#"{{"total_count": {count}, "repositories": [{}]}}"#,
            items.join(",")
        )
    }

    #[tokio::test]
    async fn list_user_installations_single_page_no_link_header() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/user/installations?per_page=100")
            .match_header("authorization", "Bearer t")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page_of_installations(1, 3))
            .create_async()
            .await;

        let client = test_client(&server);
        let installations = client.list_user_installations("t").await.unwrap();

        assert_eq!(installations.len(), 3);
        assert_eq!(installations[0].id, 1);
        assert_eq!(installations[0].account_login.as_deref(), Some("user-1"));
        assert_eq!(installations[2].id, 3);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn list_user_installations_follows_link_next_across_three_pages() {
        let mut server = mockito::Server::new_async().await;
        let host = server.url();

        // Page 1 -> Link next page=2
        let link1 = format!(
            "<{host}/user/installations?per_page=100&page=2>; rel=\"next\", <{host}/user/installations?per_page=100&page=3>; rel=\"last\""
        );
        let m1 = server
            .mock("GET", "/user/installations?per_page=100")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", &link1)
            .with_body(page_of_installations(1, 100))
            .create_async()
            .await;

        // Page 2 -> Link next page=3
        let link2 = format!(
            "<{host}/user/installations?per_page=100&page=1>; rel=\"prev\", <{host}/user/installations?per_page=100&page=3>; rel=\"next\""
        );
        let m2 = server
            .mock("GET", "/user/installations?per_page=100&page=2")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", &link2)
            .with_body(page_of_installations(101, 100))
            .create_async()
            .await;

        // Page 3 -> no rel="next" (last)
        let link3 = format!(
            "<{host}/user/installations?per_page=100&page=2>; rel=\"prev\", <{host}/user/installations?per_page=100&page=1>; rel=\"first\""
        );
        let m3 = server
            .mock("GET", "/user/installations?per_page=100&page=3")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", &link3)
            .with_body(page_of_installations(201, 100))
            .create_async()
            .await;

        let client = test_client(&server);
        let installations = client.list_user_installations("t").await.unwrap();

        assert_eq!(installations.len(), 300);
        assert_eq!(installations[0].id, 1);
        assert_eq!(installations[99].id, 100);
        assert_eq!(installations[100].id, 101);
        assert_eq!(installations[299].id, 300);

        m1.assert_async().await;
        m2.assert_async().await;
        m3.assert_async().await;
    }

    #[tokio::test]
    async fn list_installation_repositories_single_page() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/user/installations/42/repositories?per_page=100")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page_of_repos(1, 5))
            .create_async()
            .await;

        let client = test_client(&server);
        let repos = client
            .list_installation_repositories("t", 42)
            .await
            .unwrap();

        assert_eq!(repos.len(), 5);
        assert_eq!(repos[0].full_name, "owner/repo-1");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn list_installation_repositories_follows_link_next() {
        let mut server = mockito::Server::new_async().await;
        let host = server.url();

        let link1 = format!(
            "<{host}/user/installations/42/repositories?per_page=100&page=2>; rel=\"next\""
        );
        let m1 = server
            .mock("GET", "/user/installations/42/repositories?per_page=100")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_header("link", &link1)
            .with_body(page_of_repos(1, 100))
            .create_async()
            .await;

        let m2 = server
            .mock(
                "GET",
                "/user/installations/42/repositories?per_page=100&page=2",
            )
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(page_of_repos(101, 50))
            .create_async()
            .await;

        let client = test_client(&server);
        let repos = client
            .list_installation_repositories("t", 42)
            .await
            .unwrap();
        assert_eq!(repos.len(), 150);
        m1.assert_async().await;
        m2.assert_async().await;
    }

    #[tokio::test]
    async fn list_user_installations_401_returns_unauthenticated() {
        let mut server = mockito::Server::new_async().await;
        let _mock = server
            .mock("GET", "/user/installations?per_page=100")
            .with_status(401)
            .with_body("{}")
            .create_async()
            .await;

        let client = test_client(&server);
        let err = client.list_user_installations("t").await.unwrap_err();
        assert!(matches!(err, OAuthError::Unauthenticated));
    }

    #[test]
    fn parse_next_link_handles_commas_in_url_segments() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LINK,
            "<https://api.github.example/x?a=b,c&page=2>; rel=\"next\", <https://api.github.example/x?page=9>; rel=\"last\""
                .parse()
                .unwrap(),
        );
        let next = parse_next_link(&headers).unwrap();
        assert_eq!(next, "https://api.github.example/x?a=b,c&page=2");
    }

    #[test]
    fn parse_next_link_none_when_only_last() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::LINK,
            "<https://api.github.example/x?page=9>; rel=\"last\""
                .parse()
                .unwrap(),
        );
        assert!(parse_next_link(&headers).is_none());
    }
}
