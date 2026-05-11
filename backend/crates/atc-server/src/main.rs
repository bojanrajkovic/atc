#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use std::future::IntoFuture;
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::Notify;

use atc_core::SystemClock;
use atc_server::config;
use atc_server::db;
use atc_server::listener;
use atc_server::metrics;
use atc_server::otel::{self, OtelHandles};
use atc_server::persist::eviction::spawn_eviction_task;
use atc_server::persist::{InMemoryStore, PgStore};
use atc_server::routes;
use atc_server::shutdown::run_shutdown_orchestration;
use atc_server::state::{AppState, SeqEvent};
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
/// `sqlx::Error::Configuration` deep inside `PgPool::connect` or
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

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
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

    let otel_handles: Option<OtelHandles> = otel::init_otel(&cfg);

    init_tracing_subscriber(&cfg, otel_handles.as_ref());

    if let Some(ref db_url) = cfg.database_url {
        ensure_pg_scheme("ATC_DATABASE_URL", db_url);
    }
    if let Some(ref listener_url) = cfg.database_listener_url {
        ensure_pg_scheme("ATC_DATABASE_LISTENER_URL", listener_url);
    }

    // Create the broadcast channel for pushing domain events to WebSocket clients.
    // Capacity of 256 events — if a client falls behind, it receives RecvError::Lagged
    // and should re-fetch via GET /v1/state.
    let (webhook_tx, _rx) = tokio::sync::broadcast::channel::<SeqEvent>(256);

    // Single shared cancellation token observed by all supervised surfaces.
    let shutdown = CancellationToken::new();

    // Gap-healing backstop for the outbox drain. i64::MAX at boot (no in-flight handlers).
    let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));
    // Wake-coalesce instrumentation flag shared by listener and drain.
    let drain_in_flight = Arc::new(AtomicBool::new(false));

    // Storage mode dispatch.
    let pg_pool: Option<sqlx::PgPool> = if let Some(ref db_url) = cfg.database_url {
        let pool = db::init_pool(db_url).await.unwrap_or_else(|e| {
            if matches!(e, sqlx::Error::Migrate(_)) {
                tracing::error!(error = %e, "failed to run database migrations");
            } else {
                tracing::error!(error = %e, "failed to connect to PostgreSQL");
            }
            process::exit(1);
        });
        tracing::info!("database connected and migrations applied");
        Some(pool)
    } else {
        tracing::info!("no ATC_DATABASE_URL configured; running in in-memory mode");
        None
    };

    let (persist, eviction_handle, listener_handle, drain_handle) = if let Some(pool) = pg_pool {
        let listener_url = cfg
            .database_listener_url
            .clone()
            .or_else(|| cfg.database_url.clone())
            .expect("pg_pool is Some only when database_url is set");

        let pg_listener = listener::connect_listener(&listener_url)
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to connect PG listener");
                process::exit(1);
            });

        // Capture startup_at BEFORE the COALESCE round-trip so the drain
        // startup histogram includes the cold-pool query cost.
        let startup_at = Instant::now();

        let initial_watermark: i64 =
            sqlx::query_scalar!("SELECT COALESCE(MAX(seq), 0) AS \"max!: i64\" FROM outbox")
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "failed to query outbox watermark");
                    process::exit(1);
                });

        let broadcast_watermark = Arc::new(AtomicI64::new(initial_watermark));
        let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()));

        // Seed the gauge so /metrics reflects the initial watermark immediately.
        #[allow(clippy::cast_precision_loss)]
        ::metrics::gauge!("atc_pg_broadcast_watermark").set(initial_watermark as f64);

        let pg_store = Arc::new(PgStore::new(
            pool.clone(),
            Arc::clone(&broadcast_watermark),
            Arc::clone(&last_drain_pass_at),
        ));

        let drain_notify = Arc::new(Notify::new());
        let lh = listener::spawn_listener_task(
            pg_listener,
            drain_notify.clone(),
            min_pending_seq.clone(),
            drain_in_flight.clone(),
            shutdown.clone(),
            None,
        );
        let dh = listener::spawn_drain_task(
            pool,
            initial_watermark,
            startup_at,
            drain_notify,
            min_pending_seq,
            last_drain_pass_at,
            broadcast_watermark,
            drain_in_flight,
            webhook_tx.clone(),
            shutdown.clone(),
            None,
            None,
            None, // drain_delay: None in production
        );

        let persist: Arc<dyn atc_server::persist::PersistentStore> = pg_store;
        (persist, None, Some(lh), Some(dh))
    } else {
        // In-memory mode: InMemoryStore owns all state.
        let in_memory = Arc::new(InMemoryStore::new(
            Arc::new(SystemClock),
            Duration::from_secs(3600),
            webhook_tx.clone(),
        ));
        // Spawn eviction only in in-memory mode (PG mode has no in-memory state to evict).
        let ev_handle = spawn_eviction_task(
            Arc::clone(&in_memory),
            Duration::from_secs(60),
            shutdown.clone(),
        );
        let persist: Arc<dyn atc_server::persist::PersistentStore> = in_memory;
        (persist, Some(ev_handle), None, None)
    };

    let ws_tracker = TaskTracker::new();

    // Build AppState with the five fields that survive the refactor.
    let app_state = Arc::new(AppState {
        persist,
        webhook_tx: webhook_tx.clone(),
        webhook_secret: cfg.github.webhook_secret.clone(),
        shutdown: shutdown.clone(),
        ws_tracker: ws_tracker.clone(),
    });

    // Register metric descriptions and spawn the process-metrics collector.
    // `register_*` runs unconditionally so descriptions land before the first
    // emission either way (axum-otel-metrics channels through the no-op recorder
    // when OTel is disabled, so emits are cheap no-ops when unconnected).
    metrics::register_build_info();
    metrics::register_pg_write_counters();
    metrics::register_listener_metrics();
    let metrics_handle = metrics::spawn_process_collector(shutdown.clone());

    // Clone the Arc into the router so `app_state` itself stays in this scope
    // for the lifetime of `main`. With `app_state` still held here, AppState's
    // embedded `webhook_tx` clone keeps the broadcast channel open through
    // shutdown orchestration — WS handlers see `shutdown.cancelled()` and send
    // Close(1001) rather than racing against a `RecvError::Closed` from a
    // prematurely-dropped AppState.
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
        drain_handle,
        listener_handle,
        eviction_handle,
        metrics_handle,
        otel_handles,
    )
    .await;

    if serve_failure {
        process::exit(1);
    }
}
