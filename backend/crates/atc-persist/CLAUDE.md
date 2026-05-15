# CLAUDE.md — atc-persist

Last verified: 2026-05-15

> Canonical documentation lives in `docs/architecture/backend-server.md` (Persistence section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

The interface waist between `atc-server` and its store implementations. Defines `PersistentStore` (the trait every store implements), `LivenessError` (the `/readyz` error shape — `DbUnreachable` wraps the inner error opaquely so this crate stays free of storage-library deps), and `join_with_timeout` (the shared shutdown-join helper consumed by both stores). Re-exports `atc_core::PersistError` so call sites get one canonical name.

## Sharp edges

**`[dependencies]` must NOT include `sqlx`, `redis`, `mongodb`, or any storage backend library.** The trait crate names interfaces; concrete backends live in `atc-store-*` crates. The opaque-box shape of `LivenessError::DbUnreachable` exists precisely to keep this invariant intact — if a future change tempts you to pull in `sqlx` here, the architectural answer is to keep the box and translate at the store boundary.

**`tokio` is constrained to `["sync", "time", "rt"]`.** Trait surface only needs `broadcast::Receiver`, `JoinHandle`, and `time::timeout`. The full feature set is for executable crates.

**`tracing` is a workspace dep.** `join_with_timeout` calls `warn!` / `error!` on cancellation and timeout; without `tracing` here the trait crate could not own the shutdown-join helper.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Persistence
- ADR-0005 (PersistentStore trait relocation — superseded geographic claim, see ADR-0008)
- ADR-0006 (stores own background task lifecycle)
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
