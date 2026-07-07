use crate::common;
use crate::common::{attribute_str, parent_of, span_named};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use tower::ServiceExt;

/// Helper to build and test the full app with API routes and asset fallback.
/// Must be used in tests marked with #[serial_test::serial] since the global
/// recorder install can only happen once per binary.
fn build_full_app() -> axum::Router {
    let (app, _state) = common::build_app_no_secret();
    app
}

#[tokio::test]
#[serial_test::serial]
async fn healthz_returns_ok() {
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify response body is valid JSON
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "ok");

    // Verify content-type header
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

#[tokio::test]
#[serial_test::serial]
async fn readyz_returns_ok() {
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Verify response body is valid JSON
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "ok");

    // Verify content-type header
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/json")
    );
}

/// `healthz` has no hand-rolled span of its own — before the blanket
/// `tower_http::TraceLayer` (`routes::with_request_tracing`), it had zero
/// trace visibility. Asserts the layer covers it anyway.
#[tokio::test]
#[serial_test::serial]
async fn healthz_emits_blanket_http_request_span() {
    common::ensure_recorder_installed();
    common::reset_spans();

    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // `TraceLayer`'s span stays open until the response body is fully
    // consumed (it wraps the body to time the full transfer, not just
    // headers) — production traffic always drains the body via the HTTP
    // layer, but `oneshot()` in a test does not, so the span never closes
    // (and never exports) unless the body is drained here too.
    let _ = to_bytes(response.into_body(), usize::MAX).await;

    let spans = common::read_finished_spans();
    let root = span_named(&spans, "http.request").expect("http.request span must be exported");
    assert_eq!(
        attribute_str(root, "http.route").as_deref(),
        Some("/healthz")
    );
    assert_eq!(
        attribute_str(root, "http.response.status_code").as_deref(),
        Some("200")
    );
}

/// `state.snapshot` is a hand-rolled span (`state_handler`) — verifies it
/// nests under the blanket `http.request` span rather than the two competing
/// for root status.
#[tokio::test]
#[serial_test::serial]
async fn state_snapshot_span_nests_under_blanket_http_request_span() {
    common::ensure_recorder_installed();
    common::reset_spans();

    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    // Drain the body — see `healthz_emits_blanket_http_request_span` for why
    // `TraceLayer`'s span won't export otherwise.
    let _ = to_bytes(response.into_body(), usize::MAX).await;

    let spans = common::read_finished_spans();
    let snapshot =
        span_named(&spans, "state.snapshot").expect("state.snapshot span must be exported");
    assert_eq!(
        parent_of(&spans, snapshot).map(|p| p.name.as_ref()),
        Some("http.request"),
        "state.snapshot must be a child of the blanket http.request span"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn health_returns_404() {
    // Test that the full app (with fallback) returns 404 for /health, not SPA index.html.
    // Verifies unknown API paths return 404 at the app level.
    let app = build_full_app();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
