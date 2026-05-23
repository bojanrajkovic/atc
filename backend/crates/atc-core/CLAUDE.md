# CLAUDE.md — atc-core

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Pure domain types, state-machine transition logic, and business rules for ATC. No GitHub-specific dependencies, no async runtime, no interior mutability. The `atc-github` crate maps webhook payloads into these domain types. All stateful persistence concerns (locking, secondary indexes, TTL eviction) live in the store crates (`atc-store-mem`, `atc-store-pg`); see ADR-0008.

## Contracts

Invariants enforced by the pure transition functions and verified by unit + proptest suites. The forward-only, idempotent-reapplication, and conclusion-implies-completion invariants are documented in the arch doc (§ State Machine Invariants); the entries below are atc-core-specific implementation decisions not covered there.

**`completed_at` preserve-first:** The `Completed` transition sets `completed_at` using `envelope.completed_at.or(existing.completed_at)`. Because the FSM is forward-only, once a timestamp is recorded it cannot be overwritten by idempotent replay of the same event. Both run and job completion follow this pattern; the field is typed `Option<DateTime<Utc>>` with `#[ts(optional)]` for rolling-deploy tolerance on the wire.

**Predecessor predicate includes self:** `predecessors_of(target)` returns `&'static [Self]` containing every valid predecessor status *and the target status itself*. The self-inclusion is intentional — it enables the Postgres store to issue a predicated UPSERT (`WHERE status = ANY(predecessors)`) that acts as both a forward-only guard and an idempotent no-op when the row is already at the target state. Removing self from the slice would break idempotent replay in `atc-store-pg`.

**`test-support` feature gate:** `TestClock` and `fixed_test_timestamp()` are compiled only under the `test-support` feature. A workspace-level `disallowed-methods` clippy lint (see `backend/clippy.toml`) blocks direct `Utc::now` / `SystemTime::now` calls in both production and fixture code, keeping time sources deterministic across the workspace.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain Model
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
