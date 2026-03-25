use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// API routes. Mount these before the asset fallback.
pub fn api_routes() -> Router {
    Router::new().route("/health", get(health))
}
