//! Consolidated integration-test entry point for `atc-store-pg`.
//!
//! Single binary rather than separate `tests/*.rs` files — mirrors
//! `atc-server`'s `tests/integration/main.rs` rationale (one link step
//! instead of one per file; see issue #82).

mod common;

mod session_store_tests;
