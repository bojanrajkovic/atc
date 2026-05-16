//! Hot-reload watcher for `runner_pools` config.
//!
//! Watches the parent directory of `$ATC_CONFIG_FILE` for changes (debounced)
//! and re-parses the file through the same validation path as startup. Pushes
//! results via a process-internal broadcast channel; the WebSocket handler in
//! `ws.rs` wraps these into the wire `WireFrame` shape and forwards to clients.
//!
//! See `docs/architecture/deployment.md` § "Hot-reload" for the operator-facing
//! behavior (kubelet propagation timing, missing-file semantics, scalar-drift
//! warn-log) and `docs/architecture/backend-server.md` for the in-process
//! integration (second broadcast channel on `AppState`, `RwLock`-wrapped
//! capacities, shutdown integration).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Weak;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use atc_core::RunnerPoolCapacity;
use atc_store_pg::metrics::METER_SCOPE;
use figment::{
    Figment,
    providers::{Format, Serialized, Yaml},
};
use notify_debouncer_full::notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, new_debouncer};
use opentelemetry::KeyValue;
use opentelemetry::metrics::Counter;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::{Config, ScalarSnapshot, reload_runner_pools};
use crate::state::AppState;

/// Process-internal event broadcast from the watcher to the WS handler.
///
/// Two layers exist by design: `ConfigEvent` is a Rust-internal channel
/// (broadcast on `AppState::config_events_tx`), and `ws::WireFrame` is the
/// wire shape that wraps each variant for the WS client. Keeping them separate
/// avoids leaking serde/ts-rs annotations across the broadcast surface and
/// confines the wire concept to the WS handler.
#[derive(Debug, Clone)]
pub enum ConfigEvent {
    /// A reload succeeded with content that differs from current AppState.
    /// Carries the full new capacity list (NOT a delta).
    Update(Vec<RunnerPoolCapacity>),
    /// A reload attempt failed. Old AppState capacities are kept; this event
    /// is informational. The wrapped string is operator-readable detail.
    ReloadError { reason: String },
}

/// Debounce window before notify events fire a reload. Kept at 500 ms — long
/// enough to coalesce a rapid burst of writes from the kubelet ConfigMap swap
/// (`..data` symlink rename plus several stat/follow events), short enough
/// that an operator edit propagates within a second.
const DEBOUNCE: Duration = Duration::from_millis(500);

/// OTel metrics emitted by the config watcher. Owned by `spawn_config_watcher`
/// and updated on every reload attempt.
///
/// `atc_config_reload_total{result,reason}` is a cached `Counter<u64>` with
/// pre-built attribute slices for each (result, reason) combination —
/// matching the cached-instrument convention in `docs/architecture/metrics.md`
/// § "Metric and span authoring contract". Allocating attribute slices on
/// the emit path would defeat the convention.
///
/// `atc_config_runner_pools` is registered as an `ObservableGauge<f64>` whose
/// callback closes over a `Weak<AtomicI64>` so a dropped store/watcher
/// short-circuits the callback to a no-op (matches the PgMetrics pattern).
/// The watcher updates `pool_count` synchronously on each applied reload;
/// the OTel collection cycle reads it on every scrape.
pub struct ConfigWatcherMetrics {
    reload_counter: Counter<u64>,
    pool_count: Arc<AtomicI64>,
    attrs_applied: [KeyValue; 2],
    attrs_noop: [KeyValue; 2],
    attrs_read: [KeyValue; 2],
    attrs_parse: [KeyValue; 2],
    attrs_validate: [KeyValue; 2],
}

impl ConfigWatcherMetrics {
    /// Register the watcher's OTel instruments against the global meter.
    ///
    /// Must be called after `otel::init_otel` — the global meter provider
    /// is the precondition. Under a no-op meter (OTel disabled) every
    /// instrument is a no-op; calling this is still cheap.
    ///
    /// `initial_pool_count` is the number of pools loaded at startup;
    /// surfaces immediately on the gauge so dashboards reflect the
    /// startup-loaded state without waiting for the first reload.
    pub fn register(initial_pool_count: usize) -> Arc<Self> {
        let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);

        let reload_counter = meter
            .u64_counter("atc_config_reload_total")
            .with_description(
                "Config-watcher reload attempts, labeled by outcome. \
                 result=success,reason=applied → reload changed AppState; \
                 result=success,reason=noop → reload matched current state (no broadcast); \
                 result=failure,reason=<read|parse|validate> → reload kept old state.",
            )
            .build();

        let pool_count = Arc::new(AtomicI64::new(initial_pool_count_as_i64(
            initial_pool_count,
        )));

        // Register the observable gauge with a Weak reference so callbacks
        // belonging to dropped watchers become no-ops once `pool_count` is
        // released. Following the PgMetrics pattern in
        // `atc-store-pg::metrics::PgMetrics::register_with_meter`.
        let pool_count_weak: Weak<AtomicI64> = Arc::downgrade(&pool_count);
        let _gauge = meter
            .f64_observable_gauge("atc_config_runner_pools")
            .with_description(
                "Current number of operator-declared runner pools loaded by the \
                 config watcher. Reflects startup-loaded state until the first \
                 reload, then tracks the latest applied reload.",
            )
            .with_callback(move |observer| {
                if let Some(atomic) = pool_count_weak.upgrade() {
                    #[allow(clippy::cast_precision_loss)]
                    let value = atomic.load(Ordering::Acquire) as f64;
                    observer.observe(value, &[]);
                }
            })
            .build();

        Arc::new(Self {
            reload_counter,
            pool_count,
            attrs_applied: [
                KeyValue::new("result", "success"),
                KeyValue::new("reason", "applied"),
            ],
            attrs_noop: [
                KeyValue::new("result", "success"),
                KeyValue::new("reason", "noop"),
            ],
            attrs_read: [
                KeyValue::new("result", "failure"),
                KeyValue::new("reason", "read"),
            ],
            attrs_parse: [
                KeyValue::new("result", "failure"),
                KeyValue::new("reason", "parse"),
            ],
            attrs_validate: [
                KeyValue::new("result", "failure"),
                KeyValue::new("reason", "validate"),
            ],
        })
    }

    fn record_applied(&self, new_pool_count: usize) {
        #[allow(clippy::cast_possible_wrap)]
        let v = new_pool_count.min(i64::MAX as usize) as i64;
        self.pool_count.store(v, Ordering::Release);
        self.reload_counter.add(1, &self.attrs_applied);
    }

    fn record_noop(&self) {
        self.reload_counter.add(1, &self.attrs_noop);
    }

    fn record_failure(&self, category: &str) {
        let attrs = match category {
            "read" => &self.attrs_read,
            "parse" => &self.attrs_parse,
            "validate" => &self.attrs_validate,
            // Unknown category — log and skip rather than allocating a fresh
            // attribute slice in the hot path. The reload_runner_pools
            // helper currently emits one of the three known categories.
            other => {
                tracing::warn!(
                    category = other,
                    "config_watcher: unknown failure category for metric"
                );
                return;
            }
        };
        self.reload_counter.add(1, attrs);
    }
}

#[allow(clippy::cast_possible_wrap)]
fn initial_pool_count_as_i64(n: usize) -> i64 {
    n.min(i64::MAX as usize) as i64
}

/// Spawns the hot-reload watcher task.
///
/// Watches `config_path.parent()` (non-recursive) and triggers a reload on
/// any debounced filesystem event that touches the configured file or a
/// `..data` symlink (the Kubernetes ConfigMap atomic-swap pattern). Each
/// reload attempt:
///
/// 1. Reads and validates the file via [`reload_runner_pools`].
/// 2. Diffs the file's scalar fields against `startup_scalars` and emits a
///    `tracing::warn!` per changed scalar (diagnostic only — hot-reload is
///    restricted to `runner_pools` by design).
/// 3. On success with capacities that differ from current AppState, takes
///    `AppState.runner_pool_capacities.write().await`, replaces, and
///    broadcasts [`ConfigEvent::Update`]. Identical content is a no-op.
/// 4. On failure, keeps AppState unchanged and broadcasts
///    [`ConfigEvent::ReloadError`].
///
/// Returns `None` (with a `warn!`) if `config_path.parent()` does not exist —
/// bare-metal dev boxes without `/etc/atc/` still boot cleanly, they just
/// don't get hot-reload.
///
/// The returned `JoinHandle` MUST be joined by `run_shutdown_orchestration`
/// before `otel::shutdown` (the watcher emits spans and metrics; per the
/// "no live emitter when shutdown fires" invariant it joins before the OTel
/// providers shut down).
pub fn spawn_config_watcher(
    config_path: PathBuf,
    app_state: Arc<AppState>,
    startup_scalars: ScalarSnapshot,
    metrics: Arc<ConfigWatcherMetrics>,
    shutdown: CancellationToken,
) -> Option<JoinHandle<()>> {
    let parent = match config_path.parent() {
        Some(p) if p.exists() => p.to_path_buf(),
        _ => {
            tracing::warn!(
                config_path = %config_path.display(),
                "config_watcher: parent directory does not exist; hot-reload disabled",
            );
            return None;
        }
    };

    let file_name = config_path.file_name().map(OsStr::to_os_string);

    // mpsc capacity 1: collapse all debounced events into a single
    // "something happened" tick. We re-read the whole file on every tick, so
    // there's nothing to gain from queuing multiple notifications — the
    // pending tick will trigger a fresh read once the watcher task wakes.
    let (tx, rx) = mpsc::channel::<()>(1);

    let tx_clone = tx.clone();
    let parent_for_filter = parent.clone();
    let file_name_for_filter = file_name.clone();
    let event_handler = move |result: DebounceEventResult| {
        let interesting = match &result {
            Ok(events) => events.iter().any(|de| {
                de.event.paths.iter().any(|p| {
                    path_is_interesting(p, &parent_for_filter, file_name_for_filter.as_deref())
                })
            }),
            // notify errors (e.g., backend-specific overflow) — always
            // trigger a reload so a temporary glitch doesn't pin stale
            // capacities. The equality check on the reload path keeps a
            // spurious wake from causing a redundant broadcast.
            Err(_) => true,
        };
        if interesting {
            let _ = tx_clone.try_send(());
        }
    };

    let mut debouncer = match new_debouncer(DEBOUNCE, None, event_handler) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(
                error = %e,
                "config_watcher: failed to construct debouncer; hot-reload disabled",
            );
            return None;
        }
    };
    if let Err(e) = debouncer.watch(&parent, RecursiveMode::NonRecursive) {
        tracing::error!(
            error = %e,
            parent = %parent.display(),
            "config_watcher: failed to watch parent directory; hot-reload disabled",
        );
        return None;
    }

    tracing::info!(
        config_path = %config_path.display(),
        parent = %parent.display(),
        "config_watcher: hot-reload watcher armed",
    );

    Some(tokio::spawn(async move {
        // Move the debouncer into the task so it lives for the watcher's
        // lifetime; dropping it stops the underlying notify watcher cleanly.
        let _debouncer = debouncer;
        run_watcher(
            config_path,
            app_state,
            startup_scalars,
            metrics,
            rx,
            shutdown,
        )
        .await;
    }))
}

/// Best-effort filter for notify event paths. Returns `true` if `path` looks
/// like the configured file or a Kubernetes `..data` symlink that wraps it.
///
/// False positives are tolerated: the reload path performs an equality check
/// before broadcasting, so a spurious tick produces at most one no-op
/// `reload_runner_pools` call and no `ConfigEvent`.
fn path_is_interesting(path: &Path, parent: &Path, file_name: Option<&OsStr>) -> bool {
    if let Some(fname) = file_name
        && path.file_name() == Some(fname)
    {
        return true;
    }
    // Kubernetes ConfigMap atomic-swap pattern: the projected directory
    // exposes `..data` as a symlink to the current `..data_TIMESTAMP/` dir.
    // kubelet swaps `..data` atomically via rename — the file under the
    // configured name (`config.yaml`) is a symlink through `..data`.
    if path.file_name().and_then(OsStr::to_str) == Some("..data") {
        return true;
    }
    // Fallback: any path under the parent directory we're watching.
    path.starts_with(parent)
}

async fn run_watcher(
    config_path: PathBuf,
    app_state: Arc<AppState>,
    startup_scalars: ScalarSnapshot,
    metrics: Arc<ConfigWatcherMetrics>,
    mut rx: mpsc::Receiver<()>,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                tracing::debug!("config_watcher: shutdown observed; exiting");
                return;
            }
            tick = rx.recv() => {
                if tick.is_none() {
                    // mpsc sender dropped — only happens if the debouncer
                    // dropped, which would have been driven by our own task
                    // exit. Defensive return.
                    return;
                }
                process_reload(&config_path, &app_state, &startup_scalars, &metrics).await;
            }
        }
    }
}

async fn process_reload(
    config_path: &Path,
    app_state: &AppState,
    startup_scalars: &ScalarSnapshot,
    metrics: &ConfigWatcherMetrics,
) {
    diagnose_scalar_drift(config_path, startup_scalars);

    match reload_runner_pools(config_path) {
        Ok(new_caps) => {
            // Compare and replace under the same write guard — Decision 7 in
            // the design plan (TOCTOU-safe; the equality check is inside the
            // guard so a hypothetical concurrent writer can't slip in).
            let mut guard = app_state.runner_pool_capacities.write().await;
            if *guard == new_caps {
                tracing::debug!(
                    "config_watcher: reload produced identical capacities; no broadcast",
                );
                metrics.record_noop();
                return;
            }
            *guard = new_caps.clone();
            drop(guard);
            let new_count = new_caps.len();
            metrics.record_applied(new_count);
            tracing::info!(
                pools = new_count,
                "config_watcher: applied new runner_pools capacities",
            );
            // `send` returns Err only when no subscriber exists; that's a
            // normal state (no WS clients connected yet) and not a fault.
            let _ = app_state
                .config_events_tx
                .send(ConfigEvent::Update(new_caps));
        }
        Err(err) => {
            metrics.record_failure(err.category());
            tracing::error!(
                error = %err,
                category = err.category(),
                "config_watcher: reload failed; keeping previous capacities",
            );
            let reason = err.to_string();
            let _ = app_state
                .config_events_tx
                .send(ConfigEvent::ReloadError { reason });
        }
    }
}

/// Diagnostic-only: parse the full Config from the live file and compare its
/// scalar fields against the startup snapshot. Emits a `tracing::warn!` per
/// changed scalar field. Hot-reload is restricted to `runner_pools` (Decision
/// 9), so a changed scalar is a no-op the operator should know about —
/// either revert the YAML edit or roll the pod to apply.
///
/// Errors from the full-Config parse (malformed YAML, etc.) are suppressed
/// here — the narrow-schema reload path reports those.
fn diagnose_scalar_drift(config_path: &Path, startup_scalars: &ScalarSnapshot) {
    let Ok(contents) = std::fs::read_to_string(config_path) else {
        return;
    };
    let Ok(cfg): Result<Config, _> = Figment::from(Serialized::defaults(Config::default()))
        .merge(Yaml::string(&contents))
        .extract()
    else {
        return;
    };
    let now = ScalarSnapshot::from_config(&cfg);
    for field in startup_scalars.diff(&now) {
        tracing::warn!(
            field,
            "config_watcher: scalar config field changed but hot-reload is restricted to runner_pools; roll the pod to apply",
        );
    }
}
