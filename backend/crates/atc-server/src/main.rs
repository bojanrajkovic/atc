#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use std::future::IntoFuture;
use std::process;
use std::sync::Arc;
use std::time::Duration;

use atc_core::{Clock, SystemClock};
use atc_persist::PersistentStore;
use atc_server::config;
use atc_server::config::ScalarSnapshot;
use atc_server::config_watcher;
use atc_server::config_watcher::ConfigEvent;
use atc_server::config_watcher::ConfigWatcherMetrics;
use atc_server::metrics;
use atc_server::otel::{self, OtelHandles};
use atc_server::routes;
use atc_server::shutdown::run_shutdown_orchestration;
use atc_server::state::AppState;
use atc_store_mem::InMemoryStore;
use atc_store_pg::{DbInitError, PgStore, db, listener};
use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Validates that a database URL uses a scheme ATC supports (postgres:// or
/// postgresql://) and exits the process with an actionable log message if not.
///
/// Called eagerly at startup for both `ATC_DATABASE_URL` and
/// `ATC_DATABASE_LISTENER_URL` (when set) so that misconfigurations fail fast
/// with a remediation-naming message instead of bottoming out as
/// a misclassified `Connect` error deep inside `PgPool::connect` or
/// `connect_listener`. Mirrors the chart-time guard in
/// `deploy/helm/atc/templates/deployment.yaml`, which catches the same
/// misconfiguration at `helm template/install` time on the inline
/// `config.databaseUrl` path; this binary check covers the `existingSecret`
/// path (whose contents are opaque to the chart) and any out-of-cluster
/// invocations.
fn ensure_pg_scheme(label: &str, url: &str) {
    if url.starts_with("postgres://") || url.starts_with("postgresql://") {
        return;
    }
    let scheme = url.split("://").next().unwrap_or("");
    tracing::error!(
        url_scheme = scheme,
        "{label} must be a postgres:// or postgresql:// URL; got scheme {scheme:?}. ATC only supports external PostgreSQL.",
    );
    process::exit(1);
}

fn init_tracing_subscriber(cfg: &config::Config, otel_handles: Option<&OtelHandles>) {
    let filter = EnvFilter::try_new(&cfg.log_filter).unwrap_or_else(|_| EnvFilter::new("info"));

    let otel_layer = otel_handles.map(|handles| {
        tracing_opentelemetry::layer()
            .with_tracer(handles.tracer.clone())
            .boxed()
    });

    if matches!(cfg.log_format, config::LogFormat::Json) {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_span_list(true)
            .boxed();
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer().pretty().boxed();
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_layer)
            .init();
    }
}

async fn shutdown_signal(shutdown: CancellationToken) {
    use tokio::signal::unix::{SignalKind, signal};

    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    let sigterm = async {
        signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    tokio::select! {
        () = ctrl_c => {},
        () = sigterm => {},
    }

    shutdown.cancel();
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() {
    let cfg = config::Config::load().unwrap_or_else(|e| {
        eprintln!("configuration error: {e}");
        process::exit(1);
    });

    // Capture scalar fields BEFORE moving cfg into AppState construction.
    // Hot-reload is restricted to `runner_pools`; the watcher diffs each
    // reload against this snapshot and warn-logs any scalar drift so
    // operators editing scalar fields know to roll the pod.
    let startup_scalars = ScalarSnapshot::from_config(&cfg);
    let watcher_config_path = config::config_path();

    let otel_handles: Option<OtelHandles> = otel::init_otel(&cfg);

    init_tracing_subscriber(&cfg, otel_handles.as_ref());

    // Build-info startup line. Mirrors the `atc_build_info` gauge labels
    // (registered immediately below) so operators reading container logs
    // see the same vergen-embedded metadata that's exposed via OTel metrics
    // — useful when the metrics endpoint isn't accessible (early startup
    // crashes, no OTel pipeline, etc.).
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        git_describe = env!("VERGEN_GIT_DESCRIBE"),
        git_sha = env!("VERGEN_GIT_SHA"),
        rustc_version = env!("VERGEN_RUSTC_SEMVER"),
        build_timestamp = env!("VERGEN_BUILD_TIMESTAMP"),
        target_triple = env!("VERGEN_CARGO_TARGET_TRIPLE"),
        "atc-server starting",
    );

    // `register_build_info` is the only eager metric registration `main.rs`
    // performs. The `PgMetrics` instruments are constructed inside
    // `PgStore::start` so PG-mode metrics only register when a `PgStore` is
    // built. Under a no-op meter (OTel disabled) the gauge build + record
    // pair is cheap.
    metrics::register_build_info();

    if let Some(ref db_url) = cfg.database_url {
        ensure_pg_scheme("ATC_DATABASE_URL", db_url);
    }
    if let Some(ref listener_url) = cfg.database_listener_url {
        ensure_pg_scheme("ATC_DATABASE_LISTENER_URL", listener_url);
    }

    // Single shared cancellation token observed by all supervised surfaces.
    let shutdown = CancellationToken::new();

    // Single Clock for the process. Routes every wall-clock read in
    // production through one source so tests (and any future fault-injection
    // shim) can swap it deterministically.
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    // Storage mode dispatch. Each store owns its own broadcast sender and
    // background tasks; main.rs only holds the resulting `Arc<dyn
    // PersistentStore>`.
    let persist: Arc<dyn PersistentStore> = if let Some(ref db_url) = cfg.database_url {
        let pool = db::init_pool(db_url).await.unwrap_or_else(|e| {
            if matches!(e, DbInitError::Migrate(_)) {
                tracing::error!(error = %e, "failed to run database migrations");
            } else {
                tracing::error!(error = %e, "failed to connect to PostgreSQL");
            }
            process::exit(1);
        });
        tracing::info!("database connected and migrations applied");

        let listener_url = cfg
            .database_listener_url
            .clone()
            .unwrap_or_else(|| db_url.clone());
        let pg_listener = listener::connect_listener(&listener_url)
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to connect PG listener");
                process::exit(1);
            });

        PgStore::start(
            Arc::clone(&clock),
            pool,
            pg_listener,
            shutdown.clone(),
            cfg.outbox_retention,
        )
        .await
        .unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to start PgStore");
            process::exit(1);
        })
    } else {
        tracing::info!("no ATC_DATABASE_URL configured; running in in-memory mode");
        InMemoryStore::start(
            Arc::clone(&clock),
            Duration::from_hours(1),
            Duration::from_mins(1),
            shutdown.clone(),
        )
    };

    let ws_tracker = TaskTracker::new();

    // Build AppState. `Config::runner_pools` is already `Vec<RunnerPoolCapacity>`
    // (figment deserializes directly into the domain type), so no mapping is
    // needed here.
    let initial_capacities: Vec<atc_core::RunnerPoolCapacity> = cfg.runner_pools;

    // Bounded channel for config-change events. Capacity 256 matches the
    // CommittedEvent channel — both surfaces close the WS connection on
    // Lagged, and a debugging operator who edits the config file repeatedly
    // shouldn't be able to backpressure other consumers.
    let (config_events_tx, _) = broadcast::channel::<ConfigEvent>(256);

    let app_state = Arc::new(AppState {
        persist: Arc::clone(&persist),
        webhook_secret: cfg.github.webhook_secret.clone(),
        runner_pool_capacities: RwLock::new(initial_capacities),
        config_events_tx,
        shutdown: shutdown.clone(),
        ws_tracker: ws_tracker.clone(),
    });

    // Spawn the process-metrics observer (wraps `opentelemetry-system-metrics`).
    // Returns a `ProcessCollectorHandle` whose `shutdown()` aborts the loop and
    // surfaces a `JoinHandle` for the orchestration to await under a timeout.
    let metrics_handle = metrics::spawn_process_collector(shutdown.clone());

    // Register watcher-owned OTel instruments — gauge surfaces the
    // startup-loaded pool count immediately; counter increments on each
    // reload outcome.
    let config_watcher_metrics =
        ConfigWatcherMetrics::register(app_state.runner_pool_capacities.read().await.len());

    // Spawn the hot-reload watcher. Skipped (returns None) when the config
    // file's parent directory does not exist — bare-metal dev boxes without
    // `/etc/atc/` still boot cleanly, just without hot-reload.
    let config_watcher_handle = config_watcher::spawn_config_watcher(
        watcher_config_path,
        Arc::clone(&app_state),
        startup_scalars,
        config_watcher_metrics,
        shutdown.clone(),
    );

    // Clone the Arc into the router so `app_state` itself stays in this scope
    // for the lifetime of `main`. With `app_state` still held here, the
    // embedded `Arc<dyn PersistentStore>` keeps the store's broadcast sender
    // open through shutdown orchestration — WS handlers see
    // `shutdown.cancelled()` and send Close(1001) rather than racing against
    // a `RecvError::Closed` from a prematurely-dropped store.
    let app = routes::api_routes()
        .with_state(app_state.clone())
        .fallback(assets::fallback_handler());

    let main_listener = tokio::net::TcpListener::bind(cfg.http_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to bind to {}: {e}", cfg.http_addr);
            process::exit(1);
        });
    tracing::info!("listening on http://{}", cfg.http_addr);

    // Spawn the signal handler task that will cancel the shutdown token.
    tokio::spawn(shutdown_signal(shutdown.clone()));

    // axum::serve(...).with_graceful_shutdown(...) is IntoFuture (not Future),
    // so spawn via .into_future() so the runtime drives it independently of
    // main's shutdown choreography.
    let main_serve =
        axum::serve(main_listener, app).with_graceful_shutdown(shutdown.clone().cancelled_owned());

    let main_serve_task = tokio::spawn(main_serve.into_future());

    // Cooperative shutdown orchestration. Awaits the trigger (signal handler
    // or unexpected serve-task exit), waits for tracked WS handlers to flush
    // Close(1001) frames, then joins serve tasks and remaining background
    // handles within bounded timeouts. Returns `true` if the trigger was an
    // early serve-task failure — propagate to a non-zero process exit so K8s
    // / systemd restart the pod and alert.
    let serve_failure = run_shutdown_orchestration(
        shutdown,
        ws_tracker,
        main_serve_task,
        persist,
        metrics_handle,
        config_watcher_handle,
        otel_handles,
    )
    .await;

    if serve_failure {
        process::exit(1);
    }
}
