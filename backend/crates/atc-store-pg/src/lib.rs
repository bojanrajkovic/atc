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
pub(crate) mod traceparent;

#[cfg(any(test, feature = "test-support"))]
pub mod invariants;

pub use db::{DbInitError, init_pool};

/// Tracing-instrumented Postgres pool used throughout `atc-store-pg`.
///
/// Wraps `sqlx::PgPool` with `sqlx-tracing` so every query execution emits an
/// OTel-compatible span (`sqlx.execute`, `sqlx.fetch_one`, `sqlx.fetch_optional`,
/// `sqlx.fetch_all`, …) with `db.system.name="postgresql"`, `db.query.text`
/// (template SQL only — bind values are inaccessible via sqlx's public
/// `Execute` trait, so they cannot leak), `net.peer.name`, `net.peer.port`,
/// and `db.name`. Errors are auto-annotated. Spans inherit the surrounding
/// `#[tracing::instrument]` context, giving "webhook.handler → … → individual
/// sqlx.execute" trace correlation in Tempo.
///
/// `init_pool` builds this; `PgStore` stores it; all internal helpers thread
/// it through. Construction is a one-time wrap at startup; the inner
/// `sqlx::Pool` is consumed by `sqlx_tracing::Pool::from(_)`.
pub type TracedPool = sqlx_tracing::Pool<sqlx::Postgres>;
pub use store::{PgStore, PgStoreStartError};

#[cfg(any(test, feature = "test-support"))]
pub use store::{PgStoreTestHandles, PgStoreTestHooks};
