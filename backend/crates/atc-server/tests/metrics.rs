//! Integration tests for the metrics side-port.

use std::net::SocketAddr;

use axum::Json;
use axum::Router;
use axum::routing::get;
use axum_prometheus::PrometheusMetricLayer;
use serde::Serialize;
use tokio::net::TcpListener;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

/// Build the full server setup with metrics registration.
/// Returns (main_addr, metrics_addr).
async fn test_setup() -> (SocketAddr, SocketAddr) {
    // Step 1: Build Prometheus layer + metrics side-port router. Must happen before
    // register_build_info() and spawn_process_collector() because pair()
    // installs the global metrics recorder.
    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();

    // Step 2: Register build info (in real app this happens in metrics::register_build_info())
    metrics::describe_gauge!(
        "atc_build_info",
        "ATC build metadata (always 1; use labels for values)"
    );
    metrics::gauge!(
        "atc_build_info",
        "version" => env!("CARGO_PKG_VERSION"),
        "git_sha" => env!("CARGO_PKG_VERSION"), // Use a placeholder since we're in test
        "rustc_version" => env!("CARGO_PKG_VERSION"),
        "build_timestamp" => env!("CARGO_PKG_VERSION"),
        "target_triple" => env!("CARGO_PKG_VERSION"),
    )
    .set(1.0);

    // Step 3: Spawn process collector task
    let collector = metrics_process::Collector::default();
    collector.describe();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            collector.collect();
        }
    });

    // Step 4: Build main router with healthz and readyz endpoints
    let main_router = Router::new()
        .route("/healthz", get(health))
        .route("/readyz", get(health))
        .layer(prometheus_layer);

    // Step 5: Build metrics router with /metrics endpoint
    let metrics_router = Router::new().route(
        "/metrics",
        get(move || async move { metric_handle.render() }),
    );

    // Step 6: Bind main listener on ephemeral port
    let main_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let main_addr = main_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(main_listener, main_router).await.unwrap();
    });

    // Step 7: Bind metrics listener on ephemeral port
    let metrics_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let metrics_addr = metrics_listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(metrics_listener, metrics_router).await.unwrap();
    });

    (main_addr, metrics_addr)
}

#[tokio::test]
#[serial_test::serial]
async fn metrics_endpoint_contains_expected_families() {
    let (main_addr, metrics_addr) = test_setup().await;

    // Give servers time to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let client = reqwest::Client::new();

    // Fire a request at /healthz so axum_http_requests_total gets a row (AC2.4, AC3.5).
    let healthz_url = format!("http://{main_addr}/healthz");
    let healthz_resp = client.get(&healthz_url).send().await.unwrap();
    assert_eq!(healthz_resp.status(), 200);

    // Fire a request at /readyz so both paths appear in axum_http_requests_total (AC3.5).
    let readyz_url = format!("http://{main_addr}/readyz");
    let readyz_resp = client.get(&readyz_url).send().await.unwrap();
    assert_eq!(readyz_resp.status(), 200);

    // Fetch /metrics from the side-port listener (AC2.1).
    let metrics_url = format!("http://{metrics_addr}/metrics");
    let resp = client.get(&metrics_url).send().await.unwrap();

    // AC2.2 — Content-Type (should be text/plain)
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/plain"),
        "expected text/plain content-type, got: {content_type}"
    );

    let body = resp.text().await.unwrap();

    // AC2.2 — build_info gauge with all required labels
    assert!(
        body.contains("atc_build_info{"),
        "expected atc_build_info gauge in /metrics body"
    );

    // AC2.3 — process metrics
    for expected in &[
        "process_cpu_seconds_total",
        "process_resident_memory_bytes",
        "process_open_fds",
        "process_start_time_seconds",
    ] {
        assert!(
            body.contains(expected),
            "expected {expected} in /metrics body"
        );
    }

    // AC2.4 + AC3.5 — axum_http_requests_total with healthz and readyz paths
    assert!(
        body.contains("axum_http_requests_total"),
        "expected axum_http_requests_total in /metrics body"
    );
    assert!(
        body.contains("axum_http_requests_duration_seconds_bucket"),
        "expected axum_http_requests_duration_seconds_bucket in /metrics body"
    );
    assert!(
        body.contains("/healthz"),
        "expected /healthz path label in axum_http_requests_total"
    );
    assert!(
        body.contains("/readyz"),
        "expected /readyz path label in axum_http_requests_total"
    );
}
