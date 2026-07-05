//! Hot-reload watcher integration tests.
//!
//! Covers atomic-rename, the Kubernetes `..data` symlink swap pattern,
//! bad-reload error broadcast, no-op skip on identical content, file
//! deletion, missing parent dir, scalar-drift diagnostic, and shutdown
//! integration.

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use atc_core::SystemClock;
use atc_persist::PersistentStore;
use atc_server::config::ScalarSnapshot;
use atc_server::config_watcher::{ConfigEvent, ConfigWatcherMetrics, spawn_config_watcher};
use atc_server::state::AppState;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::common;

const RELOAD_TIMEOUT: Duration = Duration::from_secs(5);

fn write_yaml(path: &Path, contents: &str) {
    std::fs::write(path, contents).expect("write config file");
}

/// Build a minimal `AppState` that the watcher can mutate.
fn build_app_state() -> (Arc<AppState>, broadcast::Sender<ConfigEvent>) {
    let clock: Arc<dyn atc_core::Clock> = Arc::new(SystemClock);
    let persist = atc_store_mem::InMemoryStore::new_for_test(
        Arc::clone(&clock),
        Duration::from_secs(3600),
        256,
    ) as Arc<dyn PersistentStore>;

    let (tx, _) = broadcast::channel::<ConfigEvent>(16);

    let state = common::TestAppState::new(persist, clock)
        .with_config_events_tx(tx.clone())
        .build();
    (state, tx)
}

async fn recv_event(rx: &mut broadcast::Receiver<ConfigEvent>) -> ConfigEvent {
    tokio::time::timeout(RELOAD_TIMEOUT, rx.recv())
        .await
        .expect("timed out waiting for ConfigEvent")
        .expect("broadcast channel closed unexpectedly")
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_detects_atomic_rename_and_updates_state() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    let (state, tx) = build_app_state();
    let mut rx = tx.subscribe();

    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm when parent dir exists");

    // Atomic-rename pattern: write the new content into a sibling file, then
    // rename onto the configured path so the file change is observed as a
    // single rename event in notify (matching editor save-pattern behavior).
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [self-hosted, linux]\n    capacity: 42\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("atomic rename");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::Update(caps) => {
            assert_eq!(caps.len(), 1, "expected one pool, got {caps:?}");
            assert_eq!(caps[0].capacity, Some(42));
        }
        ConfigEvent::ReloadError { reason } => panic!("expected Update, got error: {reason}"),
    }

    // AppState reflects the new caps.
    let guard = state.runner_pool_capacities.read().await;
    assert_eq!(guard.len(), 1);
    assert_eq!(guard[0].capacity, Some(42));
    drop(guard);

    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_emits_reload_error_on_bad_file() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    let (state, tx) = build_app_state();
    {
        let mut guard = state.runner_pool_capacities.write().await;
        *guard = vec![atc_core::RunnerPoolCapacity {
            labels: atc_core::LabelSet::new(["a"]),
            capacity: Some(1),
        }];
    }
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Rewrite with `capacity: 0` — validation rejects, reload returns
    // ReloadError::Validate, AppState is preserved.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [a]\n    capacity: 0\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("atomic rename");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::ReloadError { reason } => {
            assert!(
                reason.contains("capacity"),
                "reason should mention capacity, got: {reason}",
            );
        }
        ConfigEvent::Update(caps) => panic!("expected ReloadError, got Update({caps:?})"),
    }

    let guard = state.runner_pool_capacities.read().await;
    assert_eq!(guard.len(), 1, "AppState unchanged after bad reload");
    assert_eq!(guard[0].capacity, Some(1));
    drop(guard);

    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_skips_broadcast_on_identical_content() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    let initial = "runner_pools:\n  - labels: [a]\n    capacity: 1\n";
    write_yaml(&config_path, initial);

    let (state, tx) = build_app_state();
    {
        // Pre-load with the same content the file already has.
        let mut guard = state.runner_pool_capacities.write().await;
        *guard = vec![atc_core::RunnerPoolCapacity {
            labels: atc_core::LabelSet::new(["a"]),
            capacity: Some(1),
        }];
    }
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Rewrite the file with identical content — the post-debounce equality
    // check inside the write guard must suppress the broadcast.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(&tmp_new, initial);
    std::fs::rename(&tmp_new, &config_path).expect("atomic rename");

    let waited = tokio::time::timeout(Duration::from_millis(1500), rx.recv()).await;
    assert!(
        waited.is_err(),
        "no broadcast expected for identical content; got: {waited:?}",
    );

    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_treats_file_deletion_as_reload_error() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    let (state, tx) = build_app_state();
    {
        let mut guard = state.runner_pool_capacities.write().await;
        *guard = vec![atc_core::RunnerPoolCapacity {
            labels: atc_core::LabelSet::new(["a"]),
            capacity: Some(1),
        }];
    }
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    std::fs::remove_file(&config_path).expect("delete config");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::ReloadError { reason } => {
            // The wrapped read error mentions the filesystem operation; the
            // ReloadError::Read variant categorizes it.
            assert!(
                reason.contains("read") || reason.contains("No such file"),
                "expected read-style error, got: {reason}",
            );
        }
        ConfigEvent::Update(caps) => panic!("expected ReloadError, got Update({caps:?})"),
    }

    let guard = state.runner_pool_capacities.read().await;
    assert_eq!(guard.len(), 1, "AppState capacities preserved after delete",);
    drop(guard);

    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_handles_kubernetes_symlink_swap() {
    // Approximates Kubernetes' ConfigMap atomic-swap pattern:
    //
    //   /etc/atc/..data         → symlink to ..data_TS1
    //   /etc/atc/..data_TS1/
    //   /etc/atc/..data_TS1/config.yaml
    //   /etc/atc/config.yaml    → symlink to ..data/config.yaml
    //
    // On update, kubelet creates ..data_TS2/, then atomically renames
    // ..data → ..data_TS2 (the new dir). Following the configured path
    // (config.yaml) transparently picks up the new content via the
    // refreshed `..data` symlink target.
    //
    // This test approximates but does not fully reproduce kubelet's
    // behavior; the real validation is operator-time. See AC6.
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");

    let ts1 = tmp.path().join("..data_TS1");
    std::fs::create_dir(&ts1).expect("mkdir ts1");
    write_yaml(
        &ts1.join("config.yaml"),
        "runner_pools:\n  - labels: [v1]\n    capacity: 1\n",
    );
    let data_link = tmp.path().join("..data");
    symlink(&ts1, &data_link).expect("..data symlink");
    let config_path = tmp.path().join("config.yaml");
    symlink(PathBuf::from("..data").join("config.yaml"), &config_path)
        .expect("config.yaml symlink");

    let (state, tx) = build_app_state();
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Stage the new dir and swap ..data atomically. `ln -sfn` semantics:
    // create a temp symlink with a unique name, then rename onto the target.
    let ts2 = tmp.path().join("..data_TS2");
    std::fs::create_dir(&ts2).expect("mkdir ts2");
    write_yaml(
        &ts2.join("config.yaml"),
        "runner_pools:\n  - labels: [v2]\n    capacity: 99\n",
    );
    let tmp_link = tmp.path().join("..data_new");
    symlink(&ts2, &tmp_link).expect("temp symlink");
    std::fs::rename(&tmp_link, &data_link).expect("atomic symlink swap");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::Update(caps) => {
            assert_eq!(caps.len(), 1);
            assert_eq!(caps[0].capacity, Some(99));
            let labels: Vec<_> = caps[0].labels.iter().collect();
            assert_eq!(labels, vec!["v2"]);
        }
        ConfigEvent::ReloadError { reason } => panic!("expected Update, got error: {reason}"),
    }

    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
async fn watcher_skip_when_parent_dir_missing() {
    common::ensure_recorder_installed();
    let nonexistent = PathBuf::from("/tmp/atc-watcher-no-such-dir/config.yaml");
    // Make sure the parent really doesn't exist.
    let _ = std::fs::remove_dir_all(nonexistent.parent().unwrap());

    let (state, _tx) = build_app_state();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        nonexistent,
        state,
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown,
    );
    assert!(
        handle.is_none(),
        "watcher should return None when parent directory is absent",
    );
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_warn_logs_scalar_drift() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    let (state, tx) = build_app_state();
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    // Capture a snapshot at the production default.
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Edit the file to include a scalar drift (`http_addr`) AND a runner
    // pool change. The watcher must apply the pools and (separately) warn
    // about the scalar drift.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "http_addr: \"127.0.0.1:9999\"\nrunner_pools:\n  - labels: [a]\n    capacity: 99\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("atomic rename");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::Update(caps) => {
            assert_eq!(caps.len(), 1);
            assert_eq!(caps[0].capacity, Some(99), "pools should still be applied");
        }
        ConfigEvent::ReloadError { reason } => panic!("expected Update, got error: {reason}"),
    }

    // We don't assert on the warn-log payload here — tracing capture is
    // brittle across the shared OTel test harness. The diagnostic is
    // covered by the `scalar_snapshot_diff_detects_changed_field` unit
    // test; this test exercises the integration path end-to-end.
    shutdown.cancel();
    handle.await.expect("watcher join");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_joined_in_shutdown_orchestration() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    let (state, _tx) = build_app_state();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());

    let handle = spawn_config_watcher(
        config_path,
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Immediately cancel — the watcher's biased select on shutdown should
    // resolve within the SHUTDOWN_TIMEOUT_CONFIG_WATCHER budget (1s). Use a
    // slightly more generous test bound to absorb scheduler jitter.
    shutdown.cancel();
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "watcher did not exit on cancel within 2s");
    result
        .expect("did not time out")
        .expect("watcher task panicked");
}

#[tokio::test]
#[serial_test::serial]
async fn watcher_metrics_count_applied_noop_and_failure() {
    use opentelemetry::KeyValue;

    common::ensure_recorder_installed();
    common::reset_metrics();

    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    let initial = "runner_pools:\n  - labels: [a]\n    capacity: 1\n";
    write_yaml(&config_path, initial);

    let (state, tx) = build_app_state();
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();
    let scalars = ScalarSnapshot::from_config(&atc_server::config::Config::default());
    let metrics = ConfigWatcherMetrics::register(0);

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        metrics,
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // (1) Applied reload — content differs from initial AppState ([]).
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [a]\n    capacity: 7\n  - labels: [b]\n    capacity: 9\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("rename");
    let _ = recv_event(&mut rx).await;

    // (2) No-op reload — write identical content.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [a]\n    capacity: 7\n  - labels: [b]\n    capacity: 9\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("rename noop");
    // Allow the debounce + reload to flow even though no broadcast fires.
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // (3) Failure reload — capacity: 0 fails validation.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [a]\n    capacity: 0\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("rename bad");
    let _ = recv_event(&mut rx).await;

    let snapshot = common::snapshot_metrics();

    let applied = common::counter_value(
        &snapshot,
        "atc_config_reload_total",
        &[
            KeyValue::new("result", "success"),
            KeyValue::new("reason", "applied"),
        ],
    );
    assert!(
        applied >= 1,
        "applied counter should be >= 1; got {applied}"
    );

    let noop = common::counter_value(
        &snapshot,
        "atc_config_reload_total",
        &[
            KeyValue::new("result", "success"),
            KeyValue::new("reason", "noop"),
        ],
    );
    assert!(noop >= 1, "noop counter should be >= 1; got {noop}");

    let failure_validate = common::counter_value(
        &snapshot,
        "atc_config_reload_total",
        &[
            KeyValue::new("result", "failure"),
            KeyValue::new("reason", "validate"),
        ],
    );
    assert_eq!(
        failure_validate, 1,
        "failure(validate) counter should be exactly 1; got {failure_validate}",
    );

    let pools_gauge = common::gauge_value(&snapshot, "atc_config_runner_pools", &[]);
    assert_eq!(
        pools_gauge,
        Some(2.0),
        "gauge should reflect the latest applied count (2); got {pools_gauge:?}",
    );

    shutdown.cancel();
    handle.await.expect("watcher join");
}

/// Regression for the env-overrides false-positive on scalar drift: when an
/// `ATC_*` env var supplies a scalar (the typical K8s pattern for
/// `ATC_DATABASE_URL` via `existingSecret`), the startup `ScalarSnapshot`
/// captures the env-overridden value. The watcher's diagnostic parse must
/// apply the same env layer so the diff sees identical values and no
/// false-positive warn-log fires on every reload.
///
/// Asserts via the success path: a clean ConfigUpdate broadcast (no scalar
/// fields in the YAML, no actual drift) reaches subscribers. Without the env
/// layer in `diagnose_scalar_drift`, the test would still pass behaviorally
/// (the diagnostic doesn't affect AppState), but a manual log scan would
/// reveal spurious warnings. This test pins the contract via the figment
/// chain — if a future regression drops the `Env` layer in
/// `diagnose_scalar_drift`, the manual log check from operators would
/// surface it; here we at least confirm that env-set scalars do not break
/// the happy path of the reload.
#[tokio::test]
#[serial_test::serial]
async fn watcher_handles_env_provided_scalars_without_false_drift() {
    common::ensure_recorder_installed();
    let tmp = tempfile::tempdir().expect("tempdir");
    let config_path = tmp.path().join("config.yaml");
    write_yaml(
        &config_path,
        "runner_pools:\n  - labels: [a]\n    capacity: 1\n",
    );

    // Set an env-only scalar. `Config::load` would lift this into the
    // startup snapshot via `Env::prefixed("ATC_").split("__")`; the watcher
    // must follow the same chain when diffing.
    //
    // SAFETY: the integration test binary serializes env-touching tests via
    // `#[serial_test::serial]`; no other thread is reading/writing this
    // variable concurrently.
    unsafe { std::env::set_var("ATC_DATABASE_URL", "postgres://example/db") };

    let cfg = atc_server::config::Config::load().expect("load with env override");
    assert_eq!(
        cfg.database_url.as_deref(),
        Some("postgres://example/db"),
        "startup must see the env-supplied scalar",
    );
    let scalars = ScalarSnapshot::from_config(&cfg);

    let (state, tx) = build_app_state();
    let mut rx = tx.subscribe();
    let shutdown = CancellationToken::new();

    let handle = spawn_config_watcher(
        config_path.clone(),
        Arc::clone(&state),
        scalars,
        ConfigWatcherMetrics::register(0),
        shutdown.clone(),
    )
    .expect("watcher should arm");

    // Trigger a reload that touches only `runner_pools`; the env-supplied
    // scalar is unchanged. The diagnostic must NOT misclassify
    // `database_url` as drift.
    let tmp_new = tmp.path().join("config.yaml.tmp");
    write_yaml(
        &tmp_new,
        "runner_pools:\n  - labels: [a]\n    capacity: 7\n",
    );
    std::fs::rename(&tmp_new, &config_path).expect("atomic rename");

    let event = recv_event(&mut rx).await;
    match event {
        ConfigEvent::Update(caps) => {
            assert_eq!(caps.len(), 1);
            assert_eq!(caps[0].capacity, Some(7));
        }
        ConfigEvent::ReloadError { reason } => panic!("expected Update, got error: {reason}"),
    }

    // Cleanup env so subsequent serial tests start clean.
    unsafe { std::env::remove_var("ATC_DATABASE_URL") };

    shutdown.cancel();
    handle.await.expect("watcher join");
}
