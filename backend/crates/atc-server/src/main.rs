#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use atc_server::config;
use atc_server::metrics;
use atc_server::routes;

async fn shutdown_signal() {
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
}

#[tokio::main]
async fn main() {
    let cfg = config::Config::load().unwrap_or_else(|e| {
        eprintln!("configuration error: {e}");
        std::process::exit(1);
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

    // Build Prometheus layer + metrics side-port router. Must happen before
    // register_build_info() and spawn_process_collector() because pair()
    // installs the global metrics recorder.
    let (prometheus_layer, metrics_router) = metrics::build();
    metrics::register_build_info();
    metrics::spawn_process_collector();

    let app = routes::api_routes(prometheus_layer).fallback(assets::fallback_handler());

    // Bind metrics listener first so a port-conflict failure is detected before
    // the main listener opens (AC2.5: bind failure exits non-zero cleanly).
    let metrics_listener = tokio::net::TcpListener::bind(cfg.metrics_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!(
                "failed to bind metrics listener to {}: {e}",
                cfg.metrics_addr
            );
            std::process::exit(1);
        });
    tracing::info!("metrics listening on http://{}", cfg.metrics_addr);

    let main_listener = tokio::net::TcpListener::bind(cfg.http_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to bind to {}: {e}", cfg.http_addr);
            std::process::exit(1);
        });
    tracing::info!("listening on http://{}", cfg.http_addr);

    let main_serve = axum::serve(main_listener, app).with_graceful_shutdown(shutdown_signal());
    let metrics_serve =
        axum::serve(metrics_listener, metrics_router).with_graceful_shutdown(shutdown_signal());

    tokio::select! {
        res = main_serve => res.expect("main server error"),
        res = metrics_serve => res.expect("metrics server error"),
    }
}
