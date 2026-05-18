use opentelemetry::KeyValue;
use tokio::task::{AbortHandle, JoinHandle};
use tokio_util::sync::CancellationToken;

use atc_store_pg::metrics::METER_SCOPE;

/// Register the `atc_build_info` observable gauge with compile-time labels.
///
/// Built once at startup against the global meter installed by
/// `otel::init_otel`. The instrument is an `ObservableGauge` whose callback
/// re-reports `1.0` with the compile-time label set on every collection cycle
/// — the OTLP→Prometheus path needs the value to be emitted on every scrape
/// regardless of how long ago `register_build_info` ran. A sync `Gauge` would
/// only emit on `record()`, so under delta-temporality readers the value
/// would vanish from the second collection onward. When OTel is disabled the
/// global meter is a no-op meter and the observable callback is never
/// invoked.
pub fn register_build_info() {
    let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
    let attrs: [KeyValue; 6] = [
        // `version` mirrors `git_describe` (both `VERGEN_GIT_DESCRIBE`)
        // rather than `CARGO_PKG_VERSION` so the operator-facing identifier
        // tracks the git tag the image was built from, not whatever
        // `Cargo.toml` happened to say at that commit. Without this, an
        // ad-hoc rc tag placed on a commit where release-please has
        // already bumped `Cargo.toml` produces an image whose OCI label
        // (`org.opencontainers.image.version` = the tag) and Prometheus
        // `version=` label disagree — dashboards and image pulls then
        // identify the same artifact by two different strings.
        // `git_describe` is `git describe --tags`: for ATC's release pipeline
        // (which only fires on `v*` tags) this is always the exact tag (e.g.
        // `v1.0.0`). For local builds it's `<latest-tag>-<offset>-g<sha>` or
        // a vergen-gix fallback string if the build environment has no git
        // history available (e.g. a tarball-sourced build). The startup log
        // line in main.rs surfaces the same value for operator visibility.
        KeyValue::new("version", env!("VERGEN_GIT_DESCRIBE")),
        KeyValue::new("git_describe", env!("VERGEN_GIT_DESCRIBE")),
        KeyValue::new("git_sha", env!("VERGEN_GIT_SHA")),
        KeyValue::new("rustc_version", env!("VERGEN_RUSTC_SEMVER")),
        KeyValue::new("build_timestamp", env!("VERGEN_BUILD_TIMESTAMP")),
        KeyValue::new("target_triple", env!("VERGEN_CARGO_TARGET_TRIPLE")),
    ];
    let _gauge = meter
        .f64_observable_gauge("atc_build_info")
        .with_description("ATC build metadata (always 1; use labels for values)")
        .with_callback(move |observer| observer.observe(1.0, &attrs))
        .build();
}

/// Handle for the background process metrics observer spawned by
/// [`spawn_process_collector`].
///
/// The observer's spawned task is supervised by `tokio` and aborted at
/// shutdown via [`ProcessCollectorHandle::shutdown`]. The graceful-shutdown
/// orchestration in `shutdown.rs` awaits the join handle with a bounded
/// timeout after invoking shutdown, so a stuck observer cannot stall process
/// exit.
pub struct ProcessCollectorHandle {
    abort: AbortHandle,
    join: JoinHandle<()>,
}

impl ProcessCollectorHandle {
    /// Abort the underlying observer task and return the join handle so the
    /// shutdown orchestration can await it under a timeout.
    pub fn shutdown(self) -> JoinHandle<()> {
        self.abort.abort();
        self.join
    }

    /// Wrap an arbitrary spawned task in a `ProcessCollectorHandle`. Lets
    /// unit tests stub the process observer with a trivial future without
    /// paying the cost of spawning the real `sysinfo`-driven loop. The
    /// `cfg(test)` gate is sufficient because the only consumers are unit
    /// tests inside `shutdown::tests`; no integration test calls this.
    #[cfg(test)]
    pub fn from_join_handle(join: JoinHandle<()>) -> Self {
        let abort = join.abort_handle();
        Self { abort, join }
    }
}

/// Spawn the `opentelemetry-system-metrics` process observer and return a
/// handle that the shutdown orchestration can abort and join.
///
/// The observer ticks on a self-managed interval (driven by the standard
/// `OTEL_METRIC_EXPORT_INTERVAL` env var, default 30 seconds) and emits
/// `process.cpu.usage`, `process.cpu.utilization`, `process.memory.usage`,
/// `process.memory.virtual`, and `process.disk.io` against the supplied
/// meter. The emitter loops forever — there is no cooperative shutdown
/// surface on the upstream crate — so the wrapper uses `AbortHandle::abort()`
/// to terminate it. Aborting between ticks is the common case; aborting
/// mid-tick is safe because `init_process_observer` does no DB / network
/// work that would surface a panic on cancellation.
pub fn spawn_process_collector(_cancel: CancellationToken) -> ProcessCollectorHandle {
    let meter = opentelemetry::global::meter_provider().meter(METER_SCOPE);
    let join = tokio::spawn(async move {
        if let Err(err) = opentelemetry_system_metrics::init_process_observer(meter).await {
            tracing::warn!(%err, "opentelemetry-system-metrics observer exited with error");
        }
    });
    let abort = join.abort_handle();
    ProcessCollectorHandle { abort, join }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;
    use tokio_util::sync::CancellationToken;

    use super::spawn_process_collector;

    /// The handle's `shutdown()` aborts the spawned observer and returns a
    /// join handle that resolves promptly.
    #[tokio::test]
    async fn process_collector_aborts_on_shutdown() {
        let handle = spawn_process_collector(CancellationToken::new());
        let join = handle.shutdown();
        let result = timeout(Duration::from_millis(200), join).await;
        assert!(
            result.is_ok(),
            "process collector task did not exit within 200 ms after abort",
        );
    }
}
