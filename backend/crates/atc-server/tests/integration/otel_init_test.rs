//! Tests for the OTel SDK gating contract.
//!
//! `init_otel` installs PROCESS-GLOBAL providers, recorder, and propagator —
//! it cannot safely run inside this integration binary because the providers
//! it sets up persist across tests and a subsequent `shutdown()` would render
//! the globals unusable for every later test (including the OTel test harness
//! installed by `common::ensure_recorder_installed`). The "Some when endpoint
//! set" verification is therefore done against the side-effect-free
//! `endpoint_configured()` gate; the "None when endpoint unset" path is safe
//! to exercise because it short-circuits before any global mutation. The
//! production code path is covered end-to-end by deployment + the homelab
//! smoke test.

use atc_server::config::Config;
use atc_server::otel::{endpoint_configured, init_otel};

#[test]
#[serial_test::serial]
fn metrics_macro_is_noop_with_no_endpoint() {
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }
    metrics::counter!("atc_otel_init_test_counter").increment(1);
    metrics::gauge!("atc_otel_init_test_gauge").set(1.0);
    metrics::histogram!("atc_otel_init_test_histogram").record(0.5);
}

#[test]
#[serial_test::serial]
fn init_otel_returns_none_with_no_endpoint() {
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }
    let cfg = Config::load().expect("Config::load() should succeed with defaults");

    assert!(
        init_otel(&cfg).is_none(),
        "init_otel must return None when OTEL_EXPORTER_OTLP_ENDPOINT is unset"
    );
}

#[test]
#[serial_test::serial]
fn endpoint_configured_reports_endpoint_unset() {
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }
    assert!(
        !endpoint_configured(),
        "endpoint_configured() must be false when OTEL_EXPORTER_OTLP_ENDPOINT is unset"
    );
}

#[test]
#[serial_test::serial]
fn endpoint_configured_reports_endpoint_set() {
    unsafe {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_ENDPOINT",
            "https://collector.example:4318",
        );
    }
    let result = endpoint_configured();
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }
    assert!(
        result,
        "endpoint_configured() must be true when OTEL_EXPORTER_OTLP_ENDPOINT is set"
    );
}

#[test]
#[serial_test::serial]
fn endpoint_configured_treats_empty_string_as_unset() {
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "");
    }
    let result = endpoint_configured();
    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }
    assert!(
        !result,
        "endpoint_configured() must treat OTEL_EXPORTER_OTLP_ENDPOINT=\"\" as unset \
         (matches init_otel's gating contract — empty endpoint silently disables OTel)"
    );
}
