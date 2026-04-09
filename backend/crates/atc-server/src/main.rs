#![deny(clippy::all)]
#![warn(clippy::pedantic)]

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

mod assets;
use atc_server::routes;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let app = routes::api_routes().fallback(assets::fallback_handler());

    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind to :8080");

    tracing::info!("listening on http://0.0.0.0:8080");
    axum::serve(listener, app).await.expect("server error");
}
