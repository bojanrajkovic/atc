use atc_server::config::{Config, LogFormat};
use std::net::SocketAddr;

/// Test 1: Config defaults when no env vars are set (AC1.1).
#[test]
#[serial_test::serial]
fn config_load_defaults() {
    // Clean up any existing ATC_* env vars
    unsafe {
        std::env::remove_var("ATC_HTTP_ADDR");
        std::env::remove_var("ATC_METRICS_ADDR");
        std::env::remove_var("ATC_DATABASE_URL");
        std::env::remove_var("ATC_LOG_FILTER");
        std::env::remove_var("ATC_LOG_FORMAT");
    }

    let config = Config::load().expect("Config::load() should succeed with defaults");

    // Verify defaults
    assert_eq!(
        config.http_addr,
        "0.0.0.0:8080".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        config.metrics_addr,
        "0.0.0.0:9090".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.database_url, None);
    assert_eq!(config.log_filter, "info");

    // Verify log_format matches compile-time default
    if cfg!(debug_assertions) {
        assert_eq!(config.log_format, LogFormat::Pretty);
    } else {
        assert_eq!(config.log_format, LogFormat::Json);
    }
}

/// Test 2: Config environment variable overrides (AC1.2).
#[test]
#[serial_test::serial]
fn config_load_env_overrides() {
    // Set environment variables for override test
    unsafe {
        std::env::set_var("ATC_HTTP_ADDR", "127.0.0.1:9999");
        std::env::set_var("ATC_METRICS_ADDR", "127.0.0.1:7777");
        std::env::set_var("ATC_DATABASE_URL", "sqlite::memory:");
    }

    let config = Config::load().expect("Config::load() should succeed with overrides");

    // Verify overrides
    assert_eq!(
        config.http_addr,
        "127.0.0.1:9999".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(
        config.metrics_addr,
        "127.0.0.1:7777".parse::<SocketAddr>().unwrap()
    );
    assert_eq!(config.database_url, Some("sqlite::memory:".to_string()));

    // Clean up
    unsafe {
        std::env::remove_var("ATC_HTTP_ADDR");
        std::env::remove_var("ATC_METRICS_ADDR");
        std::env::remove_var("ATC_DATABASE_URL");
    }
}

/// Test 3: Malformed address causes error (AC1.3).
#[test]
#[serial_test::serial]
fn config_load_malformed_address() {
    // Set malformed HTTP address
    unsafe {
        std::env::set_var("ATC_HTTP_ADDR", "not-a-socket-addr");
    }

    let result = Config::load();

    // Verify it returns an error
    assert!(
        result.is_err(),
        "Config::load() should return Err for malformed address"
    );

    let error = result.unwrap_err();
    let error_msg = error.to_string();

    // Verify the error message identifies the failing field
    assert!(
        error_msg.contains("http_addr")
            || error_msg.contains("HTTP_ADDR")
            || error_msg.contains("not-a-socket-addr"),
        "Error message should identify the failing field or value: {}",
        error_msg
    );

    // Clean up
    unsafe {
        std::env::remove_var("ATC_HTTP_ADDR");
    }
}

/// Test 4: Invalid log_format value causes error (I2).
#[test]
#[serial_test::serial]
fn config_load_invalid_log_format() {
    // Set invalid log format
    unsafe {
        std::env::set_var("ATC_LOG_FORMAT", "verbose");
    }

    let result = Config::load();

    // Verify it returns an error
    assert!(
        result.is_err(),
        "Config::load() should return Err for invalid log_format"
    );

    let error = result.unwrap_err();
    let error_msg = error.to_string();

    // Verify the error message mentions log_format or the invalid value
    assert!(
        error_msg.contains("log_format")
            || error_msg.contains("LOG_FORMAT")
            || error_msg.contains("verbose"),
        "Error message should identify the failing field or value: {}",
        error_msg
    );

    // Clean up
    unsafe {
        std::env::remove_var("ATC_LOG_FORMAT");
    }
}
