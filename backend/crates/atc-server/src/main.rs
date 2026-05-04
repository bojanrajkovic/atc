#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use std::process;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use atc_core::{StateStore, SystemClock};
use atc_server::config;
use atc_server::metrics;
use atc_server::routes;
use atc_server::state::{AppState, SeqEvent};
use tokio_util::sync::CancellationToken;

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
async fn main() {
    let cfg = config::Config::load().unwrap_or_else(|e| {
        eprintln!("configuration error: {e}");
        process::exit(1);
    });

    let filter = tracing_subscriber::EnvFilter::try_new(&cfg.log_filter)
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    if matches!(cfg.log_format, config::LogFormat::Json) {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .with_span_list(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .pretty()
            .init();
    }

    let pg_pool = if let Some(ref db_url) = cfg.database_url {
        let pool = sqlx::PgPool::connect(db_url).await.unwrap_or_else(|e| {
            tracing::error!(error = %e, "failed to connect to PostgreSQL");
            process::exit(1);
        });
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to run database migrations");
                process::exit(1);
            });
        tracing::info!("database connected and migrations applied");
        Some(pool)
    } else {
        tracing::info!("no ATC_DATABASE_URL configured; running in in-memory mode");
        None
    };

    // Create the shared state store with system clock and 1-hour TTL for completed entries.
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));

    // Start the background eviction task. Runs every 60 seconds.
    let eviction_handle = store.start_eviction_task(Duration::from_secs(60));

    // Create the broadcast channel for pushing domain events to WebSocket clients.
    // Capacity of 256 events — if a client falls behind, it receives RecvError::Lagged
    // and should re-fetch via GET /v1/state.
    let (webhook_tx, _rx) = tokio::sync::broadcast::channel::<SeqEvent>(256);

    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: cfg.github.webhook_secret.clone(),
        seq: Mutex::new(0),
        pg_pool,
    });

    // Build Prometheus layer + metrics side-port router. Must happen before
    // register_build_info() and spawn_process_collector() because pair()
    // installs the global metrics recorder.
    let (prometheus_layer, metrics_router) = metrics::build();
    metrics::register_build_info();
    metrics::spawn_process_collector();

    let app = routes::api_routes(prometheus_layer)
        .with_state(app_state)
        .fallback(assets::fallback_handler());

    // Bind metrics listener first so a port-conflict failure is detected before
    // the main listener opens (AC2.5: bind failure exits non-zero cleanly).
    let metrics_listener = tokio::net::TcpListener::bind(cfg.metrics_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(
                "failed to bind metrics listener to {}: {e}",
                cfg.metrics_addr
            );
            process::exit(1);
        });
    tracing::info!("metrics listening on http://{}", cfg.metrics_addr);

    let main_listener = tokio::net::TcpListener::bind(cfg.http_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to bind to {}: {e}", cfg.http_addr);
            process::exit(1);
        });
    tracing::info!("listening on http://{}", cfg.http_addr);

    // Create a shared cancellation token for both servers
    let shutdown = CancellationToken::new();

    // Spawn the signal handler task that will cancel the token
    let shutdown_clone = shutdown.clone();
    tokio::spawn(shutdown_signal(shutdown_clone));

    // Both servers observe the same cancellation token
    let shutdown_main = shutdown.clone();
    let shutdown_metrics = shutdown.clone();
    let main_serve =
        axum::serve(main_listener, app).with_graceful_shutdown(shutdown_main.cancelled_owned());
    let metrics_serve = axum::serve(metrics_listener, metrics_router)
        .with_graceful_shutdown(shutdown_metrics.cancelled_owned());

    tokio::select! {
        res = main_serve => {
            if let Err(e) = res {
                tracing::error!("main server error: {e}");
                shutdown.cancel();
                process::exit(1);
            }
        }
        res = metrics_serve => {
            if let Err(e) = res {
                tracing::error!("metrics server error: {e}");
                shutdown.cancel();
                process::exit(1);
            }
        }
    }

    // Clean up: abort the eviction background task
    eviction_handle.abort();
}
