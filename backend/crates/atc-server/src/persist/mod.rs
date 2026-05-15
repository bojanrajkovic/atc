//! Persistence backends for domain events.
//!
//! The `PersistentStore` trait, `LivenessError`, and `PersistError` re-export
//! live in the `atc-persist` crate (issue #169, ADR-0008). The in-memory
//! backend is provided by the `atc-store-mem` crate. This module is the
//! in-tree home for the remaining PG-backed backend until the
//! `atc-store-pg` crate extraction lands:
//!
//! - [`PgStore`] — PostgreSQL-backed. Opens its own transaction per event:
//!   UPSERT + outbox INSERT + `pg_notify` → commit → returns allocated seq.
//!
//! Each store owns the background tasks that read its internal state and emit
//! domain events. `PgStore::start` constructs and spawns the listener and
//! drain tasks. The trait surfaces this as `subscribe()` (event stream) and
//! `shutdown()` (cooperative join of all owned tasks).
//!
//! The `reads` submodule provides `read_all_runs` / `read_all_jobs` free
//! functions used by the PG read path.

pub mod pg;
pub(crate) mod reads;

pub use pg::PgStore;
