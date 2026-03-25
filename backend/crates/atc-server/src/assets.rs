use axum::{
    extract::Request,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use std::sync::LazyLock;

/// Returns the appropriate fallback handler for the current build mode.
///
/// - Release mode: serves embedded frontend assets with SPA fallback.
/// - Dev mode: proxies to Vite dev server at localhost:5173.
pub fn fallback_handler() -> axum::routing::MethodRouter {
    if cfg!(debug_assertions) {
        axum::routing::any(dev_proxy)
    } else {
        axum::routing::any(serve_embedded)
    }
}

// ── Release mode: embedded assets ──────────────────────────────────────────

#[derive(rust_embed::RustEmbed)]
#[folder = "../../../frontend/dist"]
struct FrontendAssets;

fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

async fn serve_embedded(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');

    // Try to serve the exact file first.
    if let Some(file) = FrontendAssets::get(path) {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime_for_path(path))],
            file.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: non-file paths get index.html.
    match FrontendAssets::get("index.html") {
        Some(index) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.data.to_vec(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "frontend not embedded").into_response(),
    }
}

// ── Dev mode: proxy to Vite ────────────────────────────────────────────────

static HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

async fn dev_proxy(req: Request) -> Response {
    let uri = req.uri();
    let vite_url = format!(
        "http://localhost:5173{}",
        uri.path_and_query()
            .map_or("/", axum::http::uri::PathAndQuery::as_str)
    );

    match HTTP_CLIENT.get(&vite_url).send().await {
        Ok(upstream) => {
            let status =
                StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let content_type = upstream
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_string();
            match upstream.bytes().await {
                Ok(body) => (
                    status,
                    [(header::CONTENT_TYPE, content_type)],
                    body.to_vec(),
                )
                    .into_response(),
                Err(_) => (StatusCode::BAD_GATEWAY, "proxy read error").into_response(),
            }
        }
        Err(_) => (
            StatusCode::BAD_GATEWAY,
            "Vite dev server not running at localhost:5173",
        )
            .into_response(),
    }
}
