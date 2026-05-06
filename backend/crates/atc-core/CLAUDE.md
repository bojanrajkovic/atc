# CLAUDE.md — atc-core

Last verified: 2026-05-05 (Phase 3b: `pool_stats()` deleted, `snapshot()` returns `QueryResult` only — pool stats now derived on the frontend)

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Core domain types, state store, and business logic for ATC. Source-agnostic — no GitHub-specific dependencies. The `atc-github` crate maps webhook payloads into these domain types. **Phase 2b:** Adds `PersistentStore` trait and `predecessors_of()` methods to enable durable shadow writes via `atc-server::persist::PgStore`.

## Modules

| Module | Role |
|--------|------|
| `types` | Newtypes: `RunId`, `JobId`, `RepoKey`, `LabelSet`; structs: `RunnerPoolStats` with `is_elastic` and `total` fields |
| `run` | `WorkflowRun`, `RunStatus`, `RunConclusion`, state transitions; `RunStatus::predecessors_of(target)` for predicates (Phase 2b) |
| `job` | `Job`, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`, state transitions; `JobStatus::predecessors_of(target)` for predicates (Phase 2b) |
| `event` | `RunEvent`, `JobEvent` and their envelope structs |
| `store` | `StateStore` — in-memory state with event ingestion, queries, `snapshot()` (atomic consistent read returning `QueryResult { runs, jobs }`), TTL eviction; implements `PersistentStore` (Phase 2b). `pool_stats()` was removed in Phase 3b — pool stats are now derived on the frontend (see ADR 0004 / `frontend/src/lib/stores/runners.svelte.ts::computePoolStats`) |
| `persist` | `PersistentStore` trait with `apply_run_event()` and `apply_job_event()` methods; returns `PersistError` for invalid transitions (Phase 2b) |
| `clock` | `Clock` trait, `SystemClock`, `TestClock` (behind `test-support` feature) |

## TypeScript Generation

All domain types derive `#[derive(TS)]` with `#[ts(export)]` to generate TypeScript interfaces. This enables strict type safety in the frontend. Generated types are written to `frontend/src/lib/types/generated/` via `just types` recipe, driven by ts-rs export tests.

**Serialization format:**
- All structs use `#[serde(rename_all = "camelCase")]` for consistent JSON naming across the API
- `RunEvent` and `JobEvent` enums use `#[serde(tag = "type", content = "data")]` (adjacently-tagged) to generate discriminated unions in TypeScript (e.g., `{ type: "Completed", data: { conclusion: "Success" } }`)
- Status enums (`RunStatus`, `JobStatus`, `StepStatus`, `RunConclusion`, `JobConclusion`) generate as PascalCase string literal unions in TypeScript

See `docs/architecture/backend-server.md` § Frontend Type Generation for full details.

## RunnerPoolStats Type

The `RunnerPoolStats` type still derives `#[derive(TS)]` so the frontend `computePoolStats` returns the same shape — but as of Phase 3b the backend no longer computes or ships this type on the wire. The fields are:

- `labels: Vec<String>` — Sorted runner label set
- `group_name: Option<String>` — Friendly pool name
- `running: usize`, `queued: usize` — Counts
- `is_elastic: bool` — Derived from runner `group_id == Some(0)`. Indicates whether the pool auto-scales (true) or has fixed capacity (false).
- `total: Option<u32>` — Maximum capacity of the pool. Always `None` until operator capacity configuration is implemented.

Pool stats are now computed by the frontend (`frontend/src/lib/stores/runners.svelte.ts::computePoolStats`); see ADR 0004.

## Contracts

These rules are enforced by the state machine and verified by 131 tests including proptest:

- **Forward-only transitions:** `RunStatus` and `JobStatus` only progress forward. Backward transitions return `Err`.
- **Idempotent same-status:** Re-applying the current status succeeds without error (handles duplicate webhooks).
- **First-sight creation:** Events for unknown IDs create entities on the spot (handles out-of-order delivery).
- **Snapshot step semantics:** `Vec<Step>` is fully replaced on each `JobEvent`, never appended.
- **Index consistency:** Every job appears in exactly one `jobs_by_repo` set and one `jobs_by_run` set. `assert_invariants()` (test-only) verifies this.
- **Eviction safety:** Only completed jobs past TTL are evicted. Active jobs are never removed regardless of age.
- **Snapshot read shape (Phase 3b):** `snapshot()` returns `QueryResult { runs, jobs }` only. The previous tuple form `(QueryResult, Vec<RunnerPoolStats>)` and the standalone `pool_stats()` method were removed; pool stats and their lexicographic sort by `labels` are now produced by the frontend (`computePoolStats` in `runners.svelte.ts`).
- **PersistentStore predicates (Phase 2b):** `RunStatus::predecessors_of(target)` and `JobStatus::predecessors_of(target)` return `&'static [Self]` including the target itself. These are used by `PgStore` to parameterize SQL WHERE clauses for predicated UPSERTs. The predicate includes the target status to enable idempotent replay (same-status reapplication succeeds).

## Testing

```bash
cargo test -p atc-core        # 131 tests including proptest (256 random cases)
cargo clippy -p atc-core -- -D warnings
```

Phase 3b deleted `store/tests/runner_pools.rs` (~30 cases) since pool stats no longer live in this crate.

The `test-support` feature exposes `TestClock` for deterministic time in downstream crate tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain Model
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
