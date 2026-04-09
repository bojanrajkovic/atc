#![deny(clippy::all)]
#![warn(clippy::pedantic)]

mod assets;

use atc_server::config;
use atc_server::routes;

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
            .flatten_event(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .pretty()
            .init();
    }

    let app = routes::api_routes().fallback(assets::fallback_handler());

    let listener = tokio::net::TcpListener::bind(cfg.http_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to bind to {}: {e}", cfg.http_addr);
            std::process::exit(1);
        });

    tracing::info!("listening on http://{}", cfg.http_addr);
    axum::serve(listener, app).await.expect("server error");
}
