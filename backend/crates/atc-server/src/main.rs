#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::{Mutex, Notify};

use atc_core::{RunStateMachine, SystemClock};
use atc_server::config;
use atc_server::db;
use atc_server::listener;
use atc_server::metrics;
use atc_server::routes;
use atc_server::state::{AppState, SeqEvent};
use tokio_util::sync::CancellationToken;

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

    if let Some(ref db_url) = cfg.database_url {
        ensure_pg_scheme("ATC_DATABASE_URL", db_url);
    }
    if let Some(ref listener_url) = cfg.database_listener_url {
        ensure_pg_scheme("ATC_DATABASE_LISTENER_URL", listener_url);
    }

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

    // Clone the pool before it moves into AppState so the listener can use it.
    // PgPool is internally reference-counted; this clone is cheap.
    let pg_pool_for_listener = pg_pool.clone();

    // Create the shared state machine with system clock and 1-hour TTL for completed entries.
    let state_machine = Arc::new(RunStateMachine::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));

    // Start the background eviction task. Runs every 60 seconds.
    let eviction_handle = state_machine.start_eviction_task(Duration::from_secs(60));

    // Create the broadcast channel for pushing domain events to WebSocket clients.
    // Capacity of 256 events — if a client falls behind, it receives RecvError::Lagged
    // and should re-fetch via GET /v1/state.
    let (webhook_tx, _rx) = tokio::sync::broadcast::channel::<SeqEvent>(256);

    // Gap-healing backstop and drain heartbeat. min_pending_seq is
    // i64::MAX at boot (no in-flight handlers); last_drain_pass_at is now()
    // so /readyz cannot 503 between bind and the first drain pass.
    // broadcast_watermark seeds from the same MAX(seq) value used to seed the
    // drain's local watermark — see below; we plumb that value in once the
    // PG-pool branch computes it.
    let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));
    let last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()));
    let broadcast_watermark = Arc::new(AtomicI64::new(0));
    // Wake-coalesce instrumentation: the listener observes this flag in its
    // NOTIFY recv loop to count arrivals that overlapped an in-flight drain
    // pass. The drain task brackets each `drain_pass(...)` call with
    // `store(true)` ... `store(false)`.
    let drain_in_flight = Arc::new(AtomicBool::new(false));

    let seq = Arc::new(Mutex::new(0u64));
    let persist: Arc<dyn atc_server::persist::PersistentStore> = match pg_pool.clone() {
        Some(pool) => Arc::new(atc_server::persist::PgStore::new(pool)),
        None => Arc::new(atc_server::persist::InMemoryStore::new(
            state_machine.clone(),
            seq.clone(),
            webhook_tx.clone(),
        )),
    };
    let app_state = Arc::new(AppState {
        state_machine,
        webhook_tx: webhook_tx.clone(),
        webhook_secret: cfg.github.webhook_secret.clone(),
        seq,
        pg_pool,
        min_pending_seq: min_pending_seq.clone(),
        last_drain_pass_at: last_drain_pass_at.clone(),
        broadcast_watermark: broadcast_watermark.clone(),
        persist,
    });

    // Build Prometheus layer + metrics side-port router. Must happen before
    // register_build_info() and spawn_process_collector() because pair()
    // installs the global metrics recorder. Also must happen before the
    // listener/drain tasks spawn so background tasks never increment counters
    // before the global recorder is installed.
    let (prometheus_layer, metrics_router) = metrics::build();
    metrics::register_build_info();
    metrics::register_pg_write_counters();
    metrics::register_listener_metrics();
    metrics::spawn_process_collector();

    // Create a shared cancellation token for both servers and background tasks.
    // Must be created before the listener init so we can pass shutdown.clone() to the tasks.
    let shutdown = CancellationToken::new();

    // If a PG pool is configured, initialize the listener and drain background tasks.
    let (listener_handle, drain_handle) = if let Some(pool) = pg_pool_for_listener {
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
        // startup histogram includes the cold-pool query cost. Observed once
        // by the drain task after its first pass returns.
        let startup_at = Instant::now();

        let initial_watermark: i64 =
            sqlx::query_scalar!("SELECT COALESCE(MAX(seq), 0) AS \"max!: i64\" FROM outbox")
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| {
                    tracing::error!(error = %e, "failed to query outbox watermark");
                    process::exit(1);
                });

        // Seed broadcast_watermark from the same MAX(seq) so /v1/state returns
        // a sensible lastSeq before the first post-startup drain pass.
        broadcast_watermark.store(initial_watermark, std::sync::atomic::Ordering::Release);
        #[allow(clippy::cast_precision_loss)]
        ::metrics::gauge!("atc_pg_broadcast_watermark").set(initial_watermark as f64);

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
            webhook_tx,
            shutdown.clone(),
            None,
            None,
            None, // drain_delay: None in production
        );
        (Some(lh), Some(dh))
    } else {
        (None, None)
    };

    let app = routes::api_routes(prometheus_layer)
        .with_state(app_state)
        .fallback(assets::fallback_handler());

    // Bind metrics listener first so a port-conflict failure is detected before
    // the main listener opens.
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

    // Clean up: abort the eviction and listener/drain background tasks, then
    // await each within a short budget to allow clean task teardown.
    eviction_handle.abort();
    if let Some(h) = listener_handle {
        h.abort();
        let _ = tokio::time::timeout(Duration::from_millis(500), h).await;
    }
    if let Some(h) = drain_handle {
        h.abort();
        let _ = tokio::time::timeout(Duration::from_millis(500), h).await;
    }
}
