use std::sync::Arc;
use std::sync::atomic::AtomicI64;

use atc_core::{StateStore, SystemClock};
use atc_server::state::{AppState, SeqEvent};

fn now_millis_for_test() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode, header};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use std::time::Duration;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Helper to build and test the full app with API routes and asset fallback.
/// Must be used in tests marked with #[serial_test::serial] since pair() installs a global recorder.
fn build_full_app() -> axum::Router {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
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
        broadcast_watermark: Arc::new(AtomicI64::new(0)),
    });
    atc_server::routes::api_routes(layer.clone())
        .with_state(app_state)
        .fallback(atc_server::assets::fallback_handler())
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

#[tokio::test]
#[serial_test::serial]
async fn health_returns_404() {
    // Test that the full app (with fallback) returns 404 for /health, not SPA index.html.
    // This verifies AC3.3: unknown API paths return 404 at the app level.
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
