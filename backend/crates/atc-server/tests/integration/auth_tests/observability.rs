//! `auth.callback`'s span tree and `atc_auth_logins_total` outcome counter
//! (#469). WS/state/me rejection-metric coverage and the session-sweep
//! metric live alongside the tests they extend (`ws_auth_filter_tests.rs`,
//! `state_auth_filter_tests.rs`, `auth_context.rs`, `atc-store-pg`'s
//! `session_store_tests.rs`) rather than duplicated here.

use opentelemetry_sdk::trace::SpanData;

use super::*;

fn span_named<'a>(spans: &'a [SpanData], name: &str) -> Option<&'a SpanData> {
    spans.iter().find(|s| s.name.as_ref() == name)
}

fn parent_of<'a>(spans: &'a [SpanData], child: &SpanData) -> Option<&'a SpanData> {
    if !child.parent_span_id.to_bytes().iter().any(|b| *b != 0) {
        return None;
    }
    spans
        .iter()
        .find(|s| s.span_context.span_id() == child.parent_span_id)
}

fn attribute_str(span: &SpanData, key: &str) -> Option<String> {
    span.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.to_string())
}

/// No attribute on any captured span may contain a token-shaped substring —
/// mirrors `login_callback.rs`'s `schema_has_no_token_columns`-style
/// assertion (AC5 on #455) at the span layer instead of the DB layer.
fn assert_no_token_material(spans: &[SpanData]) {
    for span in spans {
        for kv in &span.attributes {
            let value = kv.value.to_string();
            assert!(
                !value.to_lowercase().contains("token") && !value.contains("mock-access-token"),
                "span {:?} attribute {} looks token-shaped: {value}",
                span.name,
                kv.key.as_str(),
            );
        }
    }
}

#[tokio::test]
#[serial_test::serial]
async fn callback_success_emits_span_tree_and_login_metric() {
    common::ensure_recorder_installed();
    common::reset_spans();
    common::reset_metrics();

    let (_pool, _container, app) = setup_default().await;
    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        &format!("?code=good-code&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);

    let spans = common::read_finished_spans();
    let callback = span_named(&spans, "auth.callback").expect("auth.callback span must export");
    assert_eq!(
        attribute_str(callback, "outcome").as_deref(),
        Some("success")
    );
    assert_eq!(attribute_str(callback, "repo_count").as_deref(), Some("1"));

    let exchange = span_named(&spans, "auth.callback.exchange")
        .expect("auth.callback.exchange span must export");
    assert_eq!(
        parent_of(&spans, exchange).map(|p| p.name.as_ref()),
        Some("auth.callback"),
        "auth.callback.exchange must be a child of auth.callback"
    );

    let repos =
        span_named(&spans, "auth.callback.repos").expect("auth.callback.repos span must export");
    assert_eq!(
        parent_of(&spans, repos).map(|p| p.name.as_ref()),
        Some("auth.callback"),
        "auth.callback.repos must be a child of auth.callback"
    );
    assert!(
        repos.attributes.iter().any(|kv| kv.key.as_str() == "pages"),
        "auth.callback.repos must record a pages attribute"
    );

    assert_no_token_material(&spans);

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_logins_total",
            &[opentelemetry::KeyValue::new("outcome", "success")],
        ),
        1,
        "a successful login must increment atc_auth_logins_total{{outcome=success}}"
    );
    assert_eq!(
        common::histogram_count(
            &snapshot,
            "atc_auth_callback_duration_seconds",
            &[opentelemetry::KeyValue::new("phase", "exchange")],
        ),
        1,
        "the exchange phase must record one callback-duration observation"
    );
    assert_eq!(
        common::histogram_count(
            &snapshot,
            "atc_auth_callback_duration_seconds",
            &[opentelemetry::KeyValue::new("phase", "repos")],
        ),
        1,
        "the repos phase must record one callback-duration observation"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn callback_denied_records_denied_outcome() {
    common::ensure_recorder_installed();
    common::reset_spans();
    common::reset_metrics();

    let (_pool, _container, app) = setup_default().await;
    let (flow_cookie, state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        &format!("?error=access_denied&state={state}"),
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);

    let spans = common::read_finished_spans();
    let callback = span_named(&spans, "auth.callback").expect("auth.callback span must export");
    assert_eq!(
        attribute_str(callback, "outcome").as_deref(),
        Some("denied")
    );

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_logins_total",
            &[opentelemetry::KeyValue::new("outcome", "denied")],
        ),
        1,
        "a denied authorization must increment atc_auth_logins_total{{outcome=denied}}"
    );
}

#[tokio::test]
#[serial_test::serial]
async fn callback_state_mismatch_records_state_mismatch_outcome() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (_pool, _container, app) = setup_default().await;
    let (flow_cookie, _state) = start_real_flow(&app, "").await;
    let resp = do_callback(
        &app,
        "?code=good-code&state=wrong-state",
        Some(&flow_cookie),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_logins_total",
            &[opentelemetry::KeyValue::new("outcome", "state_mismatch")],
        ),
        1,
        "a state mismatch must increment atc_auth_logins_total{{outcome=state_mismatch}}"
    );
}

/// `SessionStore`'s sweep task (`atc-store-pg`) registers its instruments
/// against the same global meter this crate's test harness installs, so its
/// counter is observable from here without a separate metrics harness in
/// `atc-store-pg`'s own test suite.
#[tokio::test]
#[serial_test::serial]
async fn session_sweep_records_atc_auth_swept_total() {
    common::ensure_recorder_installed();
    common::reset_metrics();

    let (pool, _container, _db_url) = common::start_pg().await;
    let mock_base = spawn_mock_github(default_mock_config()).await;
    let (_app, app_state) = build_auth_test_app(pool, mock_base).await;
    let auth = app_state
        .auth
        .as_ref()
        .expect("fixture builds auth.mode = github");

    let now = SystemClock.now();
    auth.sessions
        .create_flow("state", "verifier", "/", false)
        .await
        .expect("create_flow should succeed");

    let later = now + chrono::Duration::minutes(11);
    let (flows_deleted, _sessions_deleted) = auth
        .sessions
        .sweep_expired(later)
        .await
        .expect("sweep_expired should succeed");
    assert_eq!(flows_deleted, 1, "the flow is past its 10-minute TTL");

    let snapshot = common::snapshot_metrics();
    assert_eq!(
        common::counter_value(
            &snapshot,
            "atc_auth_swept_total",
            &[opentelemetry::KeyValue::new("kind", "flow")],
        ),
        1,
        "sweeping one expired flow must increment atc_auth_swept_total{{kind=flow}}"
    );
}
