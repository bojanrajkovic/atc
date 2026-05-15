//! Persistence backends for domain events.
//!
//! The `PersistentStore` trait, `LivenessError`, and `PersistError` re-export
//! live in the `atc-persist` crate (issue #169, ADR-0008). This module is
//! the in-tree home for the two concrete backends until the per-store crate
//! extractions land in follow-up PRs:
//!
//! - [`PgStore`] — PostgreSQL-backed. Opens its own transaction per event:
//!   UPSERT + outbox INSERT + `pg_notify` → commit → returns allocated seq.
//! - [`InMemoryStore`] — In-memory-only (dev/test). Owns `StateData` with
//!   HashMaps + secondary indexes, seq counter, clock, and broadcast sender.
//!   Uses pure `atc_core::apply_*_event` functions for transitions.
//!
//! Each store owns the background tasks that read its internal state and emit
//! domain events. `PgStore::start` constructs and spawns the listener and
//! drain tasks; `InMemoryStore::start` constructs and spawns the eviction
//! task. The trait surfaces this as `subscribe()` (event stream) and
//! `shutdown()` (cooperative join of all owned tasks).
//!
//! The `reads` submodule provides `read_all_runs` / `read_all_jobs` free
//! functions used by the PG read path.

pub mod in_memory;
pub mod pg;
pub(crate) mod reads;

pub use in_memory::InMemoryStore;
pub use pg::PgStore;
