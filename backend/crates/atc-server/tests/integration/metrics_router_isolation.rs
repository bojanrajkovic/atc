//! Test that metrics router does NOT serve health check endpoints.
//! Uses `common::build_metrics_router` to share the per-binary recorder
//! install — calling `atc_server::metrics::build()` would attempt a second
//! `set_global_recorder` and panic.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn metrics_router_does_not_serve_healthz_readyz() {
    // Metrics router (side-port) must NOT have /healthz or /readyz routes.
    // This isolates health checking to the main port.
    let metrics_router = common::build_metrics_router();

    // Try GET /healthz on the metrics router (should return 404)
    let healthz_request = Request::builder()
        .method("GET")
        .uri("/healthz")
        .body(Body::empty())
        .unwrap();

    let healthz_response = metrics_router
        .clone()
        .oneshot(healthz_request)
        .await
        .unwrap();
    assert_eq!(
        healthz_response.status(),
        StatusCode::NOT_FOUND,
        "metrics router should NOT serve /healthz"
    );

    // Try GET /readyz on the metrics router (should return 404)
    let readyz_request = Request::builder()
        .method("GET")
        .uri("/readyz")
        .body(Body::empty())
        .unwrap();

    let readyz_response = metrics_router.oneshot(readyz_request).await.unwrap();
    assert_eq!(
        readyz_response.status(),
        StatusCode::NOT_FOUND,
        "metrics router should NOT serve /readyz"
    );
}
