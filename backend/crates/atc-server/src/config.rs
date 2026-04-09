use std::net::SocketAddr;

use figment::{
    Figment,
    providers::{Env, Serialized},
};

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub metrics_addr: SocketAddr,
    pub database_url: Option<String>,
    pub log_filter: String,
    pub log_format: LogFormat,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            http_addr: "0.0.0.0:8080".parse().unwrap(),
            metrics_addr: "0.0.0.0:9090".parse().unwrap(),
            database_url: None,
            log_filter: "info".to_string(),
            log_format: LogFormat::default(),
        }
    }
}

impl Config {
    #[allow(dead_code)]
    pub fn load() -> Result<Self, Box<figment::Error>> {
        Figment::from(Serialized::defaults(Config::default()))
            .merge(Env::prefixed("ATC_").split("__"))
            .extract()
            .map_err(Box::new)
    }
}
