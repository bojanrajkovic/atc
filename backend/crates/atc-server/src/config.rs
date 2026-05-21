use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use atc_core::{LabelSet, RunnerPoolCapacity};
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
/// rendered `ConfigMap` when `runnerPools` is non-empty. Missing-file is benign
/// at startup (figment treats `Yaml::file` as auto-optional); the hot-reload
/// path treats it as an error so an operator who deletes the file mid-deploy
/// does not silently clear all pool capacities.
const DEFAULT_CONFIG_FILE: &str = "/etc/atc/config.yaml";

/// Resolves the YAML configuration file path the same way `Config::load` does:
/// `$ATC_CONFIG_FILE` if set, otherwise the chart-default `/etc/atc/config.yaml`.
///
/// The hot-reload watcher in `config_watcher.rs` uses this to arm its file
/// watcher on the same path the startup loader read from.
pub fn config_path() -> PathBuf {
    std::env::var(CONFIG_FILE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG_FILE))
}

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
    #[serde(default)]
    pub runner_pools: Vec<RunnerPoolCapacity>,
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
    /// After extraction, `runner_pools` is validated: `LabelSet` canonicalizes
    /// labels (sort + dedup) during deserialization, so the post-extract step
    /// is a pure scan via `validate_capacities`. Rejected: empty label sets,
    /// `capacity: 0` (`capacity: null` declares an unbounded pool), missing
    /// `capacity` key (rejected at parse time by `RunnerPoolCapacity`'s custom
    /// `Deserialize` impl), and duplicate canonicalized label sets.
    ///
    /// # Errors
    ///
    /// Returns `Err(Box<figment::Error>)` if:
    /// - An environment variable fails to deserialize to its target type (e.g.,
    ///   `ATC_HTTP_ADDR=invalid-addr`).
    /// - The YAML file is syntactically invalid or has type-mismatched fields.
    /// - A `runner_pools` entry omits the `capacity` key (must be present —
    ///   `capacity: null` is the canonical way to declare an unbounded pool).
    /// - A `runner_pools` entry fails validation (empty labels post-dedup,
    ///   `capacity: 0`, or a duplicate canonicalized label set).
    ///
    /// The error is boxed (`Box<figment::Error>`) because `figment::Error` is large
    /// (clippy::result_large_err).
    pub fn load() -> Result<Self, Box<figment::Error>> {
        let path = config_path();

        let config: Config = Figment::from(Serialized::defaults(Config::default()))
            .merge(Yaml::file(&path))
            .merge(Env::prefixed("ATC_").split("__"))
            .extract()
            .map_err(Box::new)?;

        validate_capacities(&config.runner_pools)
            .map_err(|msg| Box::new(figment::Error::from(msg)))?;

        Ok(config)
    }
}

/// Validates operator-declared runner pool capacities.
///
/// Shared by `Config::load` (startup) and `reload_runner_pools` (hot reload)
/// so both paths see identical validation. `LabelSet` already canonicalizes
/// (sort + dedup) during deserialization, so this function is a pure scan —
/// no mutation. Fatal — callers map the returned message into their own error
/// type.
fn validate_capacities(caps: &[RunnerPoolCapacity]) -> Result<(), String> {
    let mut seen: HashSet<&LabelSet> = HashSet::new();

    for (idx, cap) in caps.iter().enumerate() {
        if matches!(cap.capacity, Some(0)) {
            return Err(format!(
                "runner_pools[{idx}]: capacity must be >= 1 (use null for unbounded pools)"
            ));
        }

        if cap.labels.is_empty() {
            return Err(format!(
                "runner_pools[{idx}]: labels must contain at least one entry"
            ));
        }

        if !seen.insert(&cap.labels) {
            let label_list: Vec<_> = cap.labels.iter().collect();
            return Err(format!(
                "runner_pools[{idx}]: duplicate label set (labels={label_list:?} canonicalizes to a pool already declared earlier)"
            ));
        }
    }

    Ok(())
}

/// Narrow schema used by `reload_runner_pools`.
///
/// Deliberately captures ONLY `runner_pools` — other config fields require
/// per-field reload safety analysis and remain restart-only. Operators editing
/// `http_addr` / `database_url` / etc. in the live file get a `tracing::warn!`
/// from the watcher (via [`ScalarSnapshot::diff`]) noting the changed field;
/// the actual scalar value is never observed here.
#[derive(Debug, Default, serde::Deserialize)]
struct ReloadPayload {
    #[serde(default)]
    runner_pools: Vec<RunnerPoolCapacity>,
}

/// Categorized failure mode from `reload_runner_pools`.
///
/// `category()` produces the stable label set for the
/// `atc_config_reload_total{reason}` counter; the wrapped message is the
/// human-readable detail surfaced in logs and the `ConfigReloadError` WS
/// frame.
#[derive(Debug)]
pub enum ReloadError {
    /// File I/O failure — missing file, permissions, etc.
    Read(String),
    /// YAML parse / deserialization failure.
    Parse(String),
    /// Validation failure — zero capacity, empty labels, duplicate pool.
    Validate(String),
}

impl ReloadError {
    /// Stable lowercase category label for the
    /// `atc_config_reload_total{reason}` counter.
    pub fn category(&self) -> &'static str {
        match self {
            ReloadError::Read(_) => "read",
            ReloadError::Parse(_) => "parse",
            ReloadError::Validate(_) => "validate",
        }
    }
}

impl std::fmt::Display for ReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReloadError::Read(m) | ReloadError::Parse(m) | ReloadError::Validate(m) => {
                f.write_str(m)
            }
        }
    }
}

impl std::error::Error for ReloadError {}

/// Re-reads the YAML configuration file and extracts a fresh
/// `Vec<RunnerPoolCapacity>`.
///
/// Used by `config_watcher` on every debounced filesystem event. Validation
/// is identical to startup (`validate_capacities`). The path is read
/// directly so the read/parse error split is precise: figment's `Yaml::file`
/// is auto-optional and would mask a deleted-file edit as an empty document.
///
/// # Errors
///
/// - [`ReloadError::Read`] — file does not exist, permission denied, etc.
///   This is a deliberate divergence from startup behavior: `Config::load`
///   tolerates a missing file (figment's auto-optional `Yaml::file`), but on
///   reload, missing-file almost certainly indicates an operator mistake
///   that should be surfaced rather than silently zeroing all capacities.
/// - [`ReloadError::Parse`] — YAML deserialization failed.
/// - [`ReloadError::Validate`] — zero capacity, empty labels, or duplicate
///   canonicalized label set.
#[tracing::instrument(
    name = "config.reload",
    skip(path),
    fields(
        config.path = %path.display(),
        config.pools = tracing::field::Empty,
        config.outcome = tracing::field::Empty,
    ),
)]
pub fn reload_runner_pools(path: &Path) -> Result<Vec<RunnerPoolCapacity>, ReloadError> {
    let span = tracing::Span::current();
    let contents = std::fs::read_to_string(path).map_err(|e| {
        span.record("config.outcome", "read_error");
        ReloadError::Read(format!("failed to read {}: {e}", path.display()))
    })?;

    let payload: ReloadPayload = Figment::from(Yaml::string(&contents))
        .extract()
        .map_err(|e| {
            span.record("config.outcome", "parse_error");
            ReloadError::Parse(format!("failed to parse {}: {e}", path.display()))
        })?;

    validate_capacities(&payload.runner_pools).map_err(|e| {
        span.record("config.outcome", "validate_error");
        ReloadError::Validate(e)
    })?;

    span.record("config.pools", payload.runner_pools.len());
    span.record("config.outcome", "ok");
    Ok(payload.runner_pools)
}

/// Snapshot of scalar `Config` fields captured at process startup.
///
/// The hot-reload watcher restricts itself to `runner_pools` (Decision 9 in
/// `docs/design-plans/2026-05-15-issue-172-hot-reload-runner-pools.md`).
/// Scalar fields require per-field reload safety analysis and remain
/// restart-only. The watcher diffs each reload's file against this snapshot
/// and emits a `tracing::warn!` per changed scalar — diagnostic only, never
/// applied — so operators editing a scalar know the change won't take effect
/// until the next pod roll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScalarSnapshot {
    pub http_addr: SocketAddr,
    pub database_url: Option<String>,
    pub database_listener_url: Option<String>,
    pub log_filter: String,
    pub log_format: LogFormat,
    pub outbox_retention: Duration,
}

impl ScalarSnapshot {
    /// Capture the scalar surface of a fully-loaded `Config`.
    pub fn from_config(cfg: &Config) -> Self {
        Self {
            http_addr: cfg.http_addr,
            database_url: cfg.database_url.clone(),
            database_listener_url: cfg.database_listener_url.clone(),
            log_filter: cfg.log_filter.clone(),
            log_format: cfg.log_format.clone(),
            outbox_retention: cfg.outbox_retention,
        }
    }

    /// Names of scalar fields that differ between `self` and `other`. Empty
    /// slice means no drift.
    pub fn diff(&self, other: &Self) -> Vec<&'static str> {
        let mut changed = Vec::new();
        if self.http_addr != other.http_addr {
            changed.push("http_addr");
        }
        if self.database_url != other.database_url {
            changed.push("database_url");
        }
        if self.database_listener_url != other.database_listener_url {
            changed.push("database_listener_url");
        }
        if self.log_filter != other.log_filter {
            changed.push("log_filter");
        }
        if self.log_format != other.log_format {
            changed.push("log_format");
        }
        if self.outbox_retention != other.outbox_retention {
            changed.push("outbox_retention");
        }
        changed
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
        let first_labels: Vec<_> = config.runner_pools[0].labels.iter().collect();
        assert_eq!(
            first_labels,
            vec!["linux", "self-hosted", "x64"],
            "labels should be canonicalized to sorted order"
        );
        assert_eq!(config.runner_pools[0].capacity, Some(10));
        let second_labels: Vec<_> = config.runner_pools[1].labels.iter().collect();
        assert_eq!(second_labels, vec!["ubuntu-latest"]);
        assert_eq!(config.runner_pools[1].capacity, Some(20));
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
        let labels: Vec<_> = config.runner_pools[0].labels.iter().collect();
        assert_eq!(labels, vec!["linux", "self-hosted", "x64"]);
    }

    #[test]
    #[serial]
    fn unbounded_capacity_via_null_is_accepted() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [ubuntu-latest]
    capacity: null
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let config = Config::load().expect("capacity: null should succeed");
        assert_eq!(config.runner_pools.len(), 1);
        assert_eq!(
            config.runner_pools[0].capacity, None,
            "explicit null → None"
        );
        let labels: Vec<_> = config.runner_pools[0].labels.iter().collect();
        assert_eq!(labels, vec!["ubuntu-latest"]);
    }

    #[test]
    #[serial]
    fn unbounded_capacity_via_key_omission_is_rejected() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [a]
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let err = Config::load().expect_err("missing capacity key should fail");
        let msg = format!("{err}");
        assert!(
            msg.contains("capacity is required"),
            "error should explain the key requirement, got: {msg}"
        );
        assert!(
            msg.contains("capacity: null"),
            "error should point operator at `capacity: null`, got: {msg}"
        );
    }

    #[test]
    #[serial]
    fn mixed_pool_list_validates() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [ubuntu-latest]
    capacity: null
"#,
        );
        unsafe { std::env::set_var(CONFIG_FILE_ENV, file.path()) };

        let config = Config::load().expect("mixed bounded + unbounded should succeed");
        assert_eq!(config.runner_pools.len(), 2);
        assert_eq!(config.runner_pools[0].capacity, Some(10));
        let first_labels: Vec<_> = config.runner_pools[0].labels.iter().collect();
        assert_eq!(first_labels, vec!["linux", "self-hosted", "x64"]);
        assert_eq!(config.runner_pools[1].capacity, None);
        let second_labels: Vec<_> = config.runner_pools[1].labels.iter().collect();
        assert_eq!(second_labels, vec!["ubuntu-latest"]);
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
            msg.contains("capacity is required"),
            "error should explain the key requirement via the custom Deserialize impl, got: {msg}"
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
            msg.contains("capacity must be >= 1"),
            "error should explain capacity bound, got: {msg}"
        );
        assert!(
            msg.contains("null"),
            "error should point operator at `capacity: null` for unbounded pools, got: {msg}"
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
    fn reload_runner_pools_returns_caps_from_yaml() {
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [ubuntu-latest]
    capacity: 20
"#,
        );

        let caps = reload_runner_pools(file.path()).expect("happy path should succeed");
        assert_eq!(caps.len(), 2);
        let first_labels: Vec<_> = caps[0].labels.iter().collect();
        assert_eq!(first_labels, vec!["linux", "self-hosted", "x64"]);
        assert_eq!(caps[0].capacity, Some(10));
        let second_labels: Vec<_> = caps[1].labels.iter().collect();
        assert_eq!(second_labels, vec!["ubuntu-latest"]);
        assert_eq!(caps[1].capacity, Some(20));
    }

    #[test]
    #[serial]
    fn reload_runner_pools_rejects_zero_capacity() {
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [a]
    capacity: 0
"#,
        );

        let err = reload_runner_pools(file.path()).expect_err("capacity 0 should fail");
        assert!(matches!(err, ReloadError::Validate(_)), "got {err:?}");
        assert_eq!(err.category(), "validate");
    }

    #[test]
    #[serial]
    fn reload_runner_pools_rejects_duplicate_pool() {
        let file = write_yaml(
            r#"
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [x64, linux, self-hosted]
    capacity: 5
"#,
        );

        let err = reload_runner_pools(file.path()).expect_err("duplicate pool should fail");
        assert!(matches!(err, ReloadError::Validate(_)), "got {err:?}");
    }

    #[test]
    #[serial]
    fn reload_runner_pools_rejects_malformed_yaml() {
        let file = write_yaml("runner_pools: not-a-list\n");

        let err = reload_runner_pools(file.path()).expect_err("malformed YAML should fail");
        assert!(matches!(err, ReloadError::Parse(_)), "got {err:?}");
        assert_eq!(err.category(), "parse");
    }

    #[test]
    #[serial]
    fn reload_runner_pools_treats_missing_file_as_read_error() {
        // Divergence from startup: `Config::load` tolerates a missing file
        // (figment's `Yaml::file` is auto-optional and silently produces an
        // empty document). On reload, missing-file means an operator likely
        // deleted the live config; surface as an error so the old capacities
        // stay in place and the operator sees it in logs / metrics / WS.
        let path = std::path::Path::new("/tmp/atc-config-does-not-exist-172.yaml");
        let _ = std::fs::remove_file(path);
        let err = reload_runner_pools(path).expect_err("missing file should fail on reload");
        assert!(matches!(err, ReloadError::Read(_)), "got {err:?}");
        assert_eq!(err.category(), "read");
    }

    #[test]
    #[serial]
    fn reload_runner_pools_uses_narrow_schema_no_scalar_leakage() {
        // A YAML file containing both `runner_pools` and a scalar field
        // (`http_addr`) must reload the pools without complaint — the narrow
        // schema deliberately ignores scalar fields so editing one is a no-op
        // on the reload path (Decision 9). The watcher emits a separate
        // diagnostic warn-log via `ScalarSnapshot::diff`; that path is tested
        // in `config_watcher_tests.rs`.
        let file = write_yaml(
            r#"
http_addr: "0.0.0.0:9999"
database_url: "postgres://example/x"
runner_pools:
  - labels: [a]
    capacity: 1
"#,
        );

        let caps =
            reload_runner_pools(file.path()).expect("narrow schema should ignore scalar fields");
        assert_eq!(caps.len(), 1);
        assert_eq!(caps[0].capacity, Some(1));
    }

    #[test]
    fn scalar_snapshot_diff_detects_changed_field() {
        let cfg = Config::default();
        let base = ScalarSnapshot::from_config(&cfg);

        let same = ScalarSnapshot::from_config(&cfg);
        assert!(base.diff(&same).is_empty(), "identical snapshots match");

        let mut other = base.clone();
        other.http_addr = "127.0.0.1:1".parse().unwrap();
        other.log_filter = "debug".to_string();
        let changed = base.diff(&other);
        assert_eq!(changed, vec!["http_addr", "log_filter"]);
    }

    #[test]
    #[serial]
    fn unknown_field_in_pool_is_rejected() {
        let _guard = EnvGuard::capture(&[CONFIG_FILE_ENV]);
        // `capacity: null` is the canonical way to declare a pool unbounded;
        // an `elastic: true` shape remains an unknown key and must surface a
        // clear operator error rather than silent acceptance.
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
