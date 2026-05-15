//! PostgreSQL-backed [`PersistentStore`] implementation.
//!
//! `PgStore` owns the connection pool, the broadcast sender that fans
//! committed events out to WS subscribers, and the four background `JoinHandle`s
//! (listener, drain, outbox heartbeat, outbox sweep). Constructed via
//! [`PgStore::start`] (production) or, behind the `test-support` feature, via
//! `PgStore::start_with_test_hooks` (integration tests).
//!
//! The PG-side metrics surface (`PgMetrics`), the LISTEN/NOTIFY background tasks
//! ([`listener`]), the snapshot-read helpers ([`reads`]), the pool init + the
//! [`DbInitError`] wrapper ([`db`]), and the four embedded SQL migrations live
//! here too — co-located with the only crate that depends on them.
//!
//! [`PersistentStore`]: atc_persist::PersistentStore

pub mod db;
pub mod listener;
pub mod metrics;
pub mod reads;
pub mod store;

#[cfg(any(test, feature = "test-support"))]
pub mod invariants;

pub use db::{DbInitError, init_pool};
pub use store::{PgStore, PgStoreStartError};

#[cfg(any(test, feature = "test-support"))]
pub use store::{PgStoreTestHandles, PgStoreTestHooks};
