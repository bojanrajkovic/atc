#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use atc_core::{StateStore, SystemClock};
use atc_server::state::AppState;
use axum_prometheus::PrometheusMetricLayer;
use std::sync::OnceLock;

// Guard: PrometheusMetricLayer::pair() is called only once per test binary.
// Tests that use this must be marked with #[serial_test::serial] to avoid concurrent execution.
pub static PROMETHEUS_INIT: OnceLock<PrometheusMetricLayer<'static>> = OnceLock::new();

/// Compute HMAC-SHA256 signature in the format GitHub expects: sha256=<hex>
pub fn compute_signature(secret: &[u8], body: &[u8]) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).unwrap();
    mac.update(body);
    let digest = mac.finalize();
    format!("sha256={}", const_hex::encode(digest.into_bytes()))
}

/// Build app with a specific webhook secret
pub fn build_app_with_secret(secret: &str) -> (axum::Router, Arc<AppState>) {
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
pub fn build_app_no_secret() -> (axum::Router, Arc<AppState>) {
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
pub fn fixture_workflow_run_requested() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_requested.json").to_vec()
}

// Fixture: workflow_job_queued.json
pub fn fixture_workflow_job_queued() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_job_queued.json").to_vec()
}

// Fixture: workflow_run_completed.json
pub fn fixture_workflow_run_completed() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_completed.json").to_vec()
}

// Fixture: workflow_run_in_progress.json
pub fn fixture_workflow_run_in_progress() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_run_in_progress.json").to_vec()
}

// Fixture: workflow_job_in_progress.json
pub fn fixture_workflow_job_in_progress() -> Vec<u8> {
    include_bytes!("../../../atc-github/tests/fixtures/workflow_job_in_progress.json").to_vec()
}
