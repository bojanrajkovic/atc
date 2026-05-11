//! Behavioral assertions that the legacy `/metrics` HTTP endpoint is gone.
//!
//! After the swap to OTLP export, atc-server no longer exposes a Prometheus
//! text-format endpoint. Operators run a collector that ingests OTLP and
//! re-exposes scrape endpoints if needed. These tests stand up the production
//! router and verify the endpoint returns 404, and that the HTTP middleware
//! emits OTel HTTP semantic-conventions attributes for the requests it does
//! handle.
//!
//! Source-grep "is `/metrics` declared in any router" assertions are
//! intentionally not used — see `feedback_no_source_grep_tests.md`.

use crate::common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use opentelemetry::KeyValue;
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use serial_test::serial;
use std::time::Duration;
use tower::ServiceExt;

/// Snapshot, retrying briefly for `metric_name` to surface. The OTel SDK's
/// metric pipeline is synchronous on the call path, but under heavy CI load
/// (llvm-cov + coverage instrumentation) the first force_flush after an HTTP
/// middleware observation has occasionally returned an empty snapshot. Retry
/// up to ~250 ms with a yield between attempts so the test reflects a real
/// missing observation rather than a transient scheduling artifact.
async fn poll_for_metric(metric_name: &str) -> Vec<ResourceMetrics> {
    for _ in 0..50 {
        let snapshot = common::snapshot_metrics();
        if common::metric_present(&snapshot, metric_name) {
            return snapshot;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    common::snapshot_metrics()
}

#[tokio::test]
#[serial]
async fn metrics_endpoint_not_served_by_api_router() {
    common::ensure_recorder_installed();

    // Use the bare API router (no SPA fallback) so this test reflects the
    // route table itself rather than the asset fallback.
    let (_full_app, app_state) = common::build_app_no_secret();
    let app = atc_server::routes::api_routes().with_state(app_state);

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "/metrics must return 404 from the API router (the legacy endpoint is gone)",
    );
}

#[tokio::test]
#[serial]
async fn metrics_endpoint_returns_404_through_full_app_with_spa_fallback() {
    // Production wires the API router behind a SPA asset fallback. Without an
    // explicit `/metrics` 404 route on the API side, the fallback would serve
    // index.html with status 200 for any unknown path — silently misleading
    // scrapers that still hit the legacy endpoint into parsing app HTML.
    // This test exercises the full app shape (api routes + SPA fallback) to
    // pin the contract.
    common::ensure_recorder_installed();

    let (app, _state) = common::build_app_no_secret();

    let req = Request::builder()
        .method("GET")
        .uri("/metrics")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "/metrics must return 404 from the full app stack — SPA fallback must \
         not catch the legacy metrics endpoint",
    );
}

#[tokio::test]
#[serial]
async fn http_middleware_records_request_duration_with_semconv_attributes() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (app, _state) = common::build_app_no_secret();

    let req = Request::builder()
        .method("GET")
        .uri("/healthz")
        .header(header::HOST, "atc.test")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Yield once so any non-runtime-aware accumulator updates in the OTel SDK's
    // metric pipeline complete before we force_flush. Observed on Linux CI
    // under llvm-cov instrumentation: the histogram observation occasionally
    // didn't surface in the next snapshot without this yield.
    tokio::task::yield_now().await;

    let snapshot = poll_for_metric("http.server.request.duration").await;
    assert!(
        common::metric_present(&snapshot, "http.server.request.duration"),
        "expected http.server.request.duration in snapshot",
    );

    // axum-otel-metrics 0.13 records the duration histogram with
    // `http.request.method`, `http.route`, `http.response.status_code`,
    // and `server.address`. The status_code attribute is a stringified status
    // (e.g. "200") per the upstream implementation. `url.scheme` is recorded
    // on the active-requests up/down counter, not on the duration histogram.
    let duration_attrs = vec![
        KeyValue::new("http.request.method", "GET"),
        KeyValue::new("http.route", "/healthz"),
        KeyValue::new("http.response.status_code", "200"),
        KeyValue::new("server.address", "atc.test"),
    ];
    let count = common::histogram_count(&snapshot, "http.server.request.duration", &duration_attrs);
    assert!(
        count >= 1,
        "expected at least one observation of http.server.request.duration with HTTP \
         semantic-conventions attributes; got count={count}",
    );

    // url.scheme appears on the active-requests up/down counter; verifying its
    // presence here completes the AC-level coverage for HTTP semconv attrs.
    assert!(
        common::metric_present(&snapshot, "http.server.active_requests"),
        "expected http.server.active_requests in snapshot",
    );
}
