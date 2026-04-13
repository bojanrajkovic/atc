use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use atc_core::{SystemClock, StateStore};
use atc_server::state::AppState;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;
use std::time::Duration;
use tower::ServiceExt;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Compute HMAC-SHA256 signature in the format GitHub expects: sha256=<hex>
fn compute_signature(secret: &[u8], body: &[u8]) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).unwrap();
    mac.update(body);
    let digest = mac.finalize();
    format!("sha256={}", const_hex::encode(digest.into_bytes()))
}

/// Build app with a specific webhook secret
fn build_app_with_secret(secret: &str) -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: Some(secret.to_string()),
        seq: AtomicU64::new(0),
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

/// Build app with no webhook secret (verification bypassed)
fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
    let layer = PROMETHEUS_INIT.get_or_init(|| PrometheusMetricLayer::pair().0);
    let store = Arc::new(StateStore::new(
        Arc::new(SystemClock),
        Duration::from_secs(3600),
    ));
    let (webhook_tx, _) = tokio::sync::broadcast::channel(256);
    let app_state = Arc::new(AppState {
        store,
        webhook_tx,
        webhook_secret: None,
        seq: AtomicU64::new(0),
    });
    let app = atc_server::routes::api_routes(layer.clone())
        .with_state(app_state.clone())
        .fallback(atc_server::assets::fallback_handler());
    (app, app_state)
}

// Fixture: workflow_run_requested.json
fn fixture_workflow_run_requested() -> Vec<u8> {
    include_bytes!("../../atc-github/tests/fixtures/workflow_run_requested.json").to_vec()
}

/// AC1.1: Valid signature with matching secret returns 200
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_valid_signature_returns_200() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();
    let signature = compute_signature(secret.as_bytes(), &body);

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header("x-hub-signature-256", signature)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "processed");
}

/// AC1.2: No secret configured + no signature header returns 200 (verification skipped)
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_no_secret_no_signature_returns_200() {
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_no_secret();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["status"], "processed");
}

/// AC1.3: Invalid signature returns 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_invalid_signature_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header("x-hub-signature-256", "sha256=0000000000000000000000000000000000000000000000000000000000000000")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["error"], "invalid signature");
}

/// AC1.4: Missing signature header when secret configured returns 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_missing_signature_header_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("failed to read response body");
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("response is valid JSON");
    assert_eq!(json["error"], "missing X-Hub-Signature-256 header");
}

/// AC1.5: SHA-1 signature rejected with 401
#[tokio::test]
#[serial_test::serial]
async fn webhook_hmac_sha1_signature_rejected_returns_401() {
    let secret = "test-secret";
    let body = fixture_workflow_run_requested();

    let (app, _) = build_app_with_secret(secret);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/webhooks/github")
                .header("x-github-event", "workflow_run")
                .header("x-hub-signature-256", "sha1=0000000000000000000000000000000000000000")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
