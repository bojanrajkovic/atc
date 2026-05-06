//! Integration tests for the metrics side-port.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::routes;
use atc_server::state::{AppState, SeqEvent};

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
use tokio::net::TcpListener;

/// Build the full server setup with metrics registration.
/// Returns (main_addr, metrics_addr).
async fn test_setup() -> (SocketAddr, SocketAddr) {
    // Step 1: Build Prometheus layer + metrics side-port router. Must happen before
    // register_build_info() and spawn_process_collector() because pair()
    // installs the global metrics recorder.
    let (prometheus_layer, metrics_router) = atc_server::metrics::build();

    // Step 2: Register build info with real VERGEN_* labels from build.rs
    atc_server::metrics::register_build_info();

    // Step 3: Spawn process collector task
    atc_server::metrics::spawn_process_collector();

    // Step 4: Create app state
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel::<SeqEvent>(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: tokio::sync::Mutex::new(0),
        pg_pool: None,
        min_pending_seq: Arc::new(AtomicI64::new(i64::MAX)),
        last_drain_pass_at: Arc::new(AtomicI64::new(now_millis_for_test())),
    });

    // Step 5: Build main router using the production api_routes function
    let main_router = routes::api_routes(prometheus_layer).with_state(app_state);

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

    // AC2.2 — Content-Type must match the Prometheus text exposition format
    // spec exactly: `text/plain; version=0.0.4; charset=utf-8`. axum-prometheus
    // emits only `text/plain; charset=utf-8` by default, so `metrics::build()`
    // wraps the render in an explicit header tuple — if this assertion ever
    // regresses, the wrapper has been undone.
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.starts_with("text/plain"),
        "expected text/plain content-type, got: {content_type}"
    );
    assert!(
        content_type.contains("version=0.0.4"),
        "expected Prometheus exposition format version=0.0.4 in content-type, got: {content_type}"
    );
    assert!(
        content_type.contains("charset=utf-8"),
        "expected charset=utf-8 in content-type, got: {content_type}"
    );

    let body = resp.text().await.unwrap();

    // AC2.2 — build_info gauge with all required labels (real VERGEN_* labels from build.rs)
    assert!(
        body.contains("atc_build_info{"),
        "expected atc_build_info gauge in /metrics body"
    );
    // Verify real vergen labels appear in the body (not placeholders)
    assert!(
        body.contains("version="),
        "expected version label in atc_build_info"
    );
    assert!(
        body.contains("git_sha="),
        "expected git_sha label in atc_build_info"
    );
    assert!(
        body.contains("rustc_version="),
        "expected rustc_version label in atc_build_info"
    );
    assert!(
        body.contains("build_timestamp="),
        "expected build_timestamp label in atc_build_info"
    );
    assert!(
        body.contains("target_triple="),
        "expected target_triple label in atc_build_info"
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
