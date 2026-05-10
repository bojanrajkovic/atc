use atc_server::config::Config;
use atc_server::otel::init_otel;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial_test::serial]
async fn init_otel_returns_some_with_endpoint() {
    unsafe {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://127.0.0.1:1");
    }
    let cfg = Config::load().expect("Config::load() should succeed with defaults");

    let handles = init_otel(&cfg);

    let was_some = handles.is_some();
    if let Some(h) = handles {
        let _ = h.tracer_provider.shutdown();
        let _ = h.meter_provider.shutdown();
    }

    unsafe {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
    }

    assert!(
        was_some,
        "init_otel must return Some when OTEL_EXPORTER_OTLP_ENDPOINT is set"
    );
}
