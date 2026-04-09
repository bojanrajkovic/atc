//! Test that metrics router does NOT serve health check endpoints (AC3.4).
//! This test must run in its own binary because metrics::build() installs a global recorder.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

#[tokio::test]
async fn metrics_router_does_not_serve_healthz_readyz() {
    // AC3.4 — metrics router (side-port) must NOT have /healthz or /readyz routes.
    // This isolates health checking to the main port.
    let (_prometheus_layer, metrics_router) = atc_server::metrics::build();

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
