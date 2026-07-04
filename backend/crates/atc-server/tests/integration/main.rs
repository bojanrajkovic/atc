//! Consolidated integration-test entry point.
//!
//! All `atc-server` integration tests live as modules under this single binary
//! rather than as separate `tests/*.rs` files. Cargo's default treats each
//! top-level `tests/*.rs` as its own integration-test binary, which forces a
//! relink of every binary whenever the `atc-server` library changes — 27 link
//! steps for a single source edit. Consolidating into one binary collapses
//! that to one link step. Within this binary, tests still run in parallel
//! (cargo test / nextest default), and `#[serial_test::serial]` continues to
//! gate any test that needs serial execution against shared global state.
//!
//! See issue #82 for context and measured savings.

mod common;

mod auth_tests;
mod config_tests;
mod config_watcher_tests;
mod db_readyz_tests;
mod drain_forwards;
mod e2e_tests;
mod eviction_spans_test;
mod gap_healing;
mod graceful_shutdown;
mod in_memory_store_tests;
mod metrics_broadcast_watermark_test;
mod metrics_drain_pass_duration_test;
mod metrics_drain_shutdown_remaining_test;
mod metrics_drain_startup_test;
mod metrics_min_pending_seq_test;
mod metrics_outbox_lag_test;
mod metrics_wake_coalesced_test;
mod no_metrics_endpoint_test;
mod notify_listener_tests;
mod otel_init_test;
mod outbox_retention_tests;
mod outbox_tests;
mod persist_pg_tests;
mod pg_clock_seam_tests;
mod protocol_version_handshake;
mod readyz;
mod restart_recovery;
mod routes_tests;
mod row_lock_serialization;
mod shutdown_otel_flush_test;
mod staleness_sweep_mem_tests;
mod staleness_sweep_tests;
mod state_auth_filter_tests;
mod state_pg_read;
mod state_tests;
mod store_lifecycle_tests;
mod tracing_webhook_spans_test;
mod transactional_writes_tests;
mod webhook_boundary_logging_test;
mod webhook_hmac_tests;
mod webhook_ingestion_tests;
mod ws_auth_filter_tests;
mod ws_tests;
