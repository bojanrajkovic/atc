use std::net::SocketAddr;

use figment::{
    Figment,
    providers::{Env, Serialized},
};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Pretty,
    Json,
}

impl Default for LogFormat {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            LogFormat::Pretty
        } else {
            LogFormat::Json
        }
    }
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct GitHubConfig {
    #[serde(default)]
    pub webhook_secret: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub database_url: Option<String>,
    pub database_listener_url: Option<String>,
    pub log_filter: String,
    pub log_format: LogFormat,
    pub github: GitHubConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_addr: "0.0.0.0:8080".parse().unwrap(),
            database_url: None,
            database_listener_url: None,
            log_filter: "info".to_string(),
            log_format: LogFormat::default(),
            github: GitHubConfig::default(),
        }
    }
}

impl Config {
    /// Loads configuration from defaults merged with environment variables.
    ///
    /// The configuration is built from two sources, in order:
    /// 1. Struct field defaults (hardcoded in the `Default` impl)
    /// 2. Environment variables prefixed with `ATC_` (e.g., `ATC_HTTP_ADDR`, `ATC_LOG_FORMAT`)
    ///
    /// The env var prefix uses `__` (double underscore) as a hierarchy separator for future
    /// nested configuration (e.g., `ATC_GITHUB__WEBHOOK_SECRET` → `config.github.webhook_secret`).
    ///
    /// # Errors
    ///
    /// Returns `Err(Box<figment::Error>)` if:
    /// - An environment variable fails to deserialize to its target type (e.g., `ATC_HTTP_ADDR=invalid-addr`)
    /// - A required field is missing (unlikely given the `Default` impl)
    ///
    /// The error is boxed (`Box<figment::Error>`) because `figment::Error` is large (clippy::result_large_err).
    /// The error message from figment includes the failing field path for easy debugging.
    pub fn load() -> Result<Self, Box<figment::Error>> {
        Figment::from(Serialized::defaults(Config::default()))
            .merge(Env::prefixed("ATC_").split("__"))
            .extract()
            .map_err(Box::new)
    }
}
