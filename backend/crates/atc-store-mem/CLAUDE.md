# CLAUDE.md — atc-store-mem

Last verified: 2026-05-18

> Canonical documentation lives in `docs/architecture/backend-server.md` (Persistence / Storage modes section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

In-memory backend for the `PersistentStore` trait. `InMemoryStore` owns the full domain state (HashMap tables, secondary indexes, monotonic seq counter, `Arc<dyn Clock>` for TTL eviction, broadcast sender) and a single eviction `JoinHandle`. State transitions delegate to the pure `apply_run_event` / `apply_job_event` free functions in `atc_core::state_machine`; this crate owns locking (seq mutex, state RwLock), seq accounting, secondary-index maintenance on first-sight, and broadcasting `CommittedEvent` envelopes to WS subscribers. `read_snapshot` returns the full state; `read_snapshot_for_repos(&[RepoKey])` filters runs via an (org, repo) check against the primary `runs` map and reuses the `jobs_by_repo` secondary index to materialize jobs without scanning every row.

## Sharp edges

**`read_snapshot_for_repos` keeps the live cursor.** Even when the supplied `repos` slice is empty or matches nothing, the returned `StateSnapshot` must carry the current seq counter as `last_seq` — not the max seq of the matched rows. A scoped caller whose accessible repos are quiet would otherwise reconcile against a stale low seq and re-process every WS event ≥ that value. The acquire-seq-then-acquire-state lock order mirrors `read_snapshot_inner` so snapshot content and cursor describe the same point in time.

**No `sqlx`, no DB I/O.** This crate must not pull in any storage-library dependency — that is the architectural separation between `atc-store-mem` and `atc-store-pg`. `atc-github` is required because `apply_*_event` constructs `WebhookEvent::Run(env)` / `WebhookEvent::Job(env)` to populate the `CommittedEvent.event` field before broadcasting.

**Eviction handle ownership.** `InMemoryStore::start` spawns the eviction task and stores its `JoinHandle` inside the returned `Arc<Self>`. Callers MUST cancel the same `CancellationToken` they passed to `start()` before invoking `shutdown()`; otherwise the eviction task never observes cancellation and `shutdown()` waits the full `EVICTION_SHUTDOWN_TIMEOUT` (1 second) before aborting. Stores constructed via `new_for_test` skip the spawn and `shutdown()` returns immediately.

**Test-support feature gate.** `new_for_test`, `assert_invariants`, and the per-field inspection helpers (`get_job`, `get_run`, `jobs_for_run`, `jobs_for_repo`, `current_seq`) live behind `#[cfg(any(test, feature = "test-support"))]`. Integration tests in `atc-server` activate them via the cross-crate dev-dep `atc-store-mem = { path = "../atc-store-mem", features = ["test-support"] }`. The same gate keeps `mod invariants;` itself out of release builds.

## Key References

- Architecture: `docs/architecture/backend-server.md` § "InMemoryStore Architecture" and § "Storage modes — operator guidance"
- Trait surface: `atc-persist` crate (`PersistentStore`, `LivenessError`, `join_with_timeout`)
- Pure transitions: `atc-core::state_machine`
- ADR-0005, ADR-0006, ADR-0008
