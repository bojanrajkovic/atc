# CLAUDE.md — atc-core

Last verified: 2026-05-30

> Canonical documentation lives in `docs/architecture/backend-server.md` (§ Domain model and state-machine invariants). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Pure domain types, state-machine transition logic, and business rules for ATC. No GitHub-specific dependencies, no async runtime, no interior mutability. The `atc-github` crate maps webhook payloads into these domain types. All stateful persistence concerns (locking, secondary indexes, TTL eviction) live in the store crates (`atc-store-mem`, `atc-store-pg`); see ADR-0008.

## Contracts

Invariants enforced by the pure transition functions and verified by unit + proptest suites. The forward-only, idempotent-reapplication, and conclusion-implies-completion invariants are documented in the arch doc (§ Domain model and state-machine invariants); the entries below are atc-core-specific implementation decisions not covered there.

**`completed_at` preserve-first:** The `Completed` transition sets `completed_at` using `envelope.completed_at.or(existing.completed_at)`. Because the FSM is forward-only, once a timestamp is recorded it cannot be overwritten by idempotent replay of the same event. Both run and job completion follow this pattern; the field is typed `Option<DateTime<Utc>>` with `#[ts(optional)]` for rolling-deploy tolerance on the wire.

**Predecessor predicate includes self:** `predecessors_of(target)` returns `&'static [Self]` containing every valid predecessor status *and the target status itself*. The self-inclusion is intentional — it enables the Postgres store to issue a predicated UPSERT (`WHERE status = ANY(predecessors)`) that acts as both a forward-only guard and an idempotent no-op when the row is already at the target state. Removing self from the slice would break idempotent replay in `atc-store-pg`.

**`run_attempt` is carried, not interpreted — the FSM stays forward-only.** `RunEventEnvelope`/`WorkflowRun` and `JobEventEnvelope`/`Job` all carry `run_attempt: i32` (1-based; GitHub increments it on re-runs while reusing the same `run_id`, assigning fresh job IDs per attempt). `apply_run_event` / `apply_job_event` only copy the envelope's `run_attempt` onto the result; they do **not** treat a higher attempt as a reason to reset terminal state or filter jobs. Re-run detection is deliberately a persistence concern (the store decides whether to pass `None` for a fresh run, reset run columns, or filter jobs to the current attempt on read) — keeping it out of atc-core preserves the forward-only, side-effect-free transition contract. Do not add attempt-comparison branching to the pure functions.

**`test-support` feature gate:** `TestClock` and `fixed_test_timestamp()` are compiled only under the `test-support` feature, so cross-crate dev-deps (`atc-core = { path = "...", features = ["test-support"] }`) opt in explicitly. The workspace-wide `disallowed-methods` lint that pairs with this gate is documented in `backend-server.md` § Wall-clock seam — don't restate it here.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain model and state-machine invariants
- ADR-0008: `docs/architecture-decisions/0008-persistence-crate-split.md`
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
