use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use atc_core::LabelSet;
use figment::{
    Figment,
    providers::{Env, Format, Serialized, Yaml},
};

/// Default outbox retention: 7 days. See ADR 0007 § Default for the
/// calendar-time rationale.
fn default_outbox_retention() -> Duration {
    Duration::from_secs(7 * 24 * 60 * 60)
}

/// Environment variable that overrides the path of the YAML configuration file.
const CONFIG_FILE_ENV: &str = "ATC_CONFIG_FILE";

/// Default path of the YAML configuration file. Mounted by the Helm chart from a
/// rendered `ConfigMap` when `runnerPools` is non-empty. Missing-file is benign.
const DEFAULT_CONFIG_FILE: &str = "/etc/atc/config.yaml";

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

/// Operator-declared capacity for a runner pool, keyed by label set.
///
/// Loaded from `runner_pools:` entries in the YAML config file. Labels are
/// canonicalized (sorted + deduplicated) during validation so that downstream
/// `LabelSet` comparisons match regardless of the source ordering.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RunnerPoolConfig {
    pub labels: Vec<String>,
    pub capacity: u32,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Config {
    pub http_addr: SocketAddr,
    pub database_url: Option<String>,
    pub database_listener_url: Option<String>,
    pub log_filter: String,
    pub log_format: LogFormat,
    pub github: GitHubConfig,
    #[serde(default)]
    pub runner_pools: Vec<RunnerPoolConfig>,
    /// Outbox retention age. Operator-tunable via `ATC_OUTBOX_RETENTION`
    /// (humantime-parseable: `7d`, `24h`, etc.). Default 7 days. Must be at
    /// least 1 hour — `PgStore::start_inner` rejects shorter values because
    /// `inserted_at` is transaction-start time and a long-held writer
    /// transaction could commit a row past the retention cutoff before any
    /// replica has drained it. See ADR 0007 for the floor rationale.
    #[serde(default = "default_outbox_retention", with = "humantime_serde")]
    pub outbox_retention: Duration,
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
            runner_pools: Vec::new(),
            outbox_retention: default_outbox_retention(),
        }
    }
}

impl Config {
    /// Loads configuration from defaults, a YAML file, and environment variables.
    ///
    /// Layering, lowest precedence to highest:
    /// 1. Struct field defaults (hardcoded in the `Default` impl).
    /// 2. YAML file at `$ATC_CONFIG_FILE` (default `/etc/atc/config.yaml`).
    ///    Missing file is **not** an error — figment's `Yaml::file` returns an
    ///    empty map when the path is absent.
    /// 3. Environment variables prefixed with `ATC_` (e.g., `ATC_HTTP_ADDR`,
    ///    `ATC_LOG_FORMAT`). Env carries scalar overrides only — `runner_pools`
    ///    is file-only by design.
    ///
    /// The env var prefix uses `__` (double underscore) as a hierarchy separator
    /// for nested configuration (e.g., `ATC_GITHUB__WEBHOOK_SECRET` →
    /// `config.github.webhook_secret`).
    ///
    /// After extraction, `runner_pools` is validated and canonicalized: each
    /// pool's labels are sorted + deduplicated in place, empty labels arrays are
    /// rejected, `capacity == 0` is rejected, and duplicate canonicalized label
    /// sets across the array are rejected.
    ///
    /// # Errors
    ///
    /// Returns `Err(Box<figment::Error>)` if:
    /// - An environment variable fails to deserialize to its target type (e.g.,
    ///   `ATC_HTTP_ADDR=invalid-addr`).
    /// - The YAML file is syntactically invalid or has type-mismatched fields.
    /// - A `runner_pools` entry fails validation (empty labels post-dedup,
    ///   `capacity == 0`, or a duplicate canonicalized label set).
    ///
    /// The error is boxed (`Box<figment::Error>`) because `figment::Error` is large
    /// (clippy::result_large_err).
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let path: PathBuf = std::env::var(CONFIG_FILE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_FILE));

        let mut config: Config = Figment::from(Serialized::defaults(Config::default()))
            .merge(Yaml::file(&path))
            .merge(Env::prefixed("ATC_").split("__"))
            .extract()
            .map_err(Box::new)?;

        config
            .validate_runner_pools()
            .map_err(|msg| Box::new(figment::Error::from(msg)))?;

        Ok(config)
    }

    /// Canonicalizes each pool's labels (sort + dedup) and rejects invalid entries.
    ///
    /// Fatal at startup so operators see misconfiguration immediately rather than
    /// silently picking a last-one-wins resolution at runtime.
    fn validate_runner_pools(&mut self) -> Result<(), String> {
        let mut seen: HashSet<LabelSet> = HashSet::new();

        for (idx, pool) in self.runner_pools.iter_mut().enumerate() {
            if pool.capacity == 0 {
                return Err(format!(
                    "runner_pools[{idx}]: capacity must be >= 1 (got 0)"
                ));
            }

            // Canonicalize: dedup via BTreeSet, then reflect sorted order back
            // into the Vec so the on-wire payload is already canonical.
            let canonical = LabelSet::new(pool.labels.iter().cloned());
            if canonical.is_empty() {
                return Err(format!(
                    "runner_pools[{idx}]: labels must contain at least one entry"
                ));
            }
            pool.labels = canonical.iter().map(str::to_owned).collect();

            if !seen.insert(canonical) {
                return Err(format!(
                    "runner_pools[{idx}]: duplicate label set (labels={:?} canonicalizes to a pool already declared earlier)",
                    pool.labels
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::io::Write;

    /// RAII guard that snapshots and restores a set of env vars across a test.
    ///
    /// Tests in this module mutate `ATC_*` env state; `serial_test::serial`
    /// orders them but does not isolate state. The guard captures each var's
    /// value (or absence) at construction and restores it on drop, so a failed
    /// test cannot leak `ATC_CONFIG_FILE` (or any `ATC_*`) into a subsequent
    /// test that asserts the file-absent path.
    struct EnvGuard {
        snapshots: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn capture(keys: &[&'static str]) -> Self {
            let snapshots = keys.iter().map(|k| (*k, std::env::var(*k).ok())).collect();
            for k in keys {
                // SAFETY: tests are serialized via `#[serial]` so no other thread
                // is reading/writing ATC_* env state concurrently.
                unsafe { std::env::remove_var(k) };
            }
            Self { snapshots }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.snapshots {
                // SAFETY: see capture().
                match v {
                    Some(val) => unsafe { std::env::set_var(k, val) },
                    None => unsafe { std::env::remove_var(k) },
                }
            }
        }
    }

    fn write_yaml(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new()
            .prefix("atc-config-")
            .suffix(".yaml")
            .tempfile()
            .expect("create temp config file");
        file.write_all(contents.as_bytes())
            .expect("write temp config");
        file.flush().expect("flush temp config");
        file
    }

    #[test]
    #[serial]
    fn load_with_no_file_succeeds_with_empty_runner_pools() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        // Point at a path guaranteed not to exist so the default
        // `/etc/atc/config.yaml` (which may exist on a developer machine) does
        // not bleed into this test.
        unsafe {
            std::env::set_var(
                CONFIG_FILE_ENV,
                "/tmp/atc-test-definitely-does-not-exist.yaml",
            )
        };

        let config = Config::load().expect("load with no file should succeed");
        assert!(
            config.runner_pools.is_empty(),
            "runner_pools should default to empty when no file is present, got {:?}",
            config.runner_pools
        );
    }

    #[test]
    #[serial]
    fn load_from_yaml_file_parses_runner_pools() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [ubuntu-latest]
    capacity: 20
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let config = Config::load().expect("load with valid file should succeed");
        assert_eq!(config.runner_pools.len(), 2);
        assert_eq!(
            config.runner_pools[0].labels,
            vec!["linux", "self-hosted", "x64"],
            "labels should be canonicalized to sorted order"
        );
        assert_eq!(config.runner_pools[0].capacity, 10);
        assert_eq!(config.runner_pools[1].labels, vec!["ubuntu-latest"]);
        assert_eq!(config.runner_pools[1].capacity, 20);
    }

    #[test]
    #[serial]
    fn label_canonicalization_sorts_and_dedups_within_a_pool() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [x64, self-hosted, linux, self-hosted]
    capacity: 4
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let config = Config::load().expect("dedup should not be fatal");
        assert_eq!(
            config.runner_pools[0].labels,
            vec!["linux", "self-hosted", "x64"]
        );
    }

    #[test]
    #[serial]
    fn missing_capacity_is_a_deserialization_error() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [a]
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("missing capacity should fail");
        let msg = format!("{err}");
        assert!(
            msg.to_lowercase().contains("capacity"),
            "error message should mention capacity, got: {msg}"
        );
    }

    #[test]
    #[serial]
    fn zero_capacity_is_a_validation_error() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [a]
    capacity: 0
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("capacity: 0 should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("capacity") && msg.contains(">= 1"),
            "error should explain capacity bound, got: {msg}"
        );
    }

    #[test]
    #[serial]
    fn empty_labels_is_a_validation_error() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: []
    capacity: 1
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("empty labels should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("labels"),
            "error should mention labels, got: {msg}"
        );
    }

    #[test]
    #[serial]
    fn duplicate_canonicalized_pools_fail_startup() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [x64, linux, self-hosted]
    capacity: 5
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("duplicate canonicalized pools should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("duplicate label set"),
            "error should mention duplicate, got: {msg}"
        );
    }

    #[test]
    #[serial]
    fn unknown_field_in_pool_is_rejected() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        // The `elastic` field is explicitly out of scope for v1; deny_unknown_fields
        // ensures operators get a clear error instead of silent acceptance.
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [a]
    capacity: 1
    elastic: true
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("unknown field should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("elastic"),
            "error should mention the unknown field, got: {msg}"
        );
    }
}
