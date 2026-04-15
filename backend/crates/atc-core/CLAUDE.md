# CLAUDE.md — atc-core

Last verified: 2026-04-12

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Core domain types, state store, and business logic for ATC. Source-agnostic — no GitHub-specific dependencies. The `atc-github` crate maps webhook payloads into these domain types.

## Modules

| Module | Role |
|--------|------|
| `types` | Newtypes: `RunId`, `JobId`, `RepoKey`, `LabelSet`; structs: `RunnerPoolStats` with `is_elastic` and `total` fields |
| `run` | `WorkflowRun`, `RunStatus`, `RunConclusion`, state transitions |
| `job` | `Job`, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`, state transitions |
| `event` | `RunEvent`, `JobEvent` and their envelope structs |
| `store` | `StateStore` — in-memory state with event ingestion, queries, `snapshot()` (atomic consistent read), pool stats, TTL eviction |
| `clock` | `Clock` trait, `SystemClock`, `TestClock` (behind `test-support` feature) |

## TypeScript Generation

All domain types derive `#[derive(TS)]` with `#[ts(export)]` to generate TypeScript interfaces. This enables strict type safety in the frontend. Generated types are written to `frontend/src/lib/types/generated/` via `just types` recipe, driven by ts-rs export tests.

**Serialization format:**
- All structs use `#[serde(rename_all = "camelCase")]` for consistent JSON naming across the API
- `RunEvent` and `JobEvent` enums use `#[serde(tag = "type", content = "data")]` (adjacently-tagged) to generate discriminated unions in TypeScript (e.g., `{ type: "Completed", data: { conclusion: "Success" } }`)
- Status enums (`RunStatus`, `JobStatus`, `StepStatus`, `RunConclusion`, `JobConclusion`) generate as PascalCase string literal unions in TypeScript

See `docs/architecture/backend-server.md` § Frontend Type Generation for full details.

## RunnerPoolStats Extension

The `RunnerPoolStats` type has been extended with two new fields to support pool capacity visualization in the frontend:

- `is_elastic: bool` — Derived from runner `group_id == Some(0)` during pool stats computation. Indicates whether the pool auto-scales (true) or has fixed capacity (false). Used by the frontend to adjust capacity bar rendering and threshold colors.
- `total: Option<u32>` — Maximum capacity of the pool. Always `None` until operator capacity configuration is implemented. Will be used by the frontend to render capacity bars and determine if a pool is over capacity.

Pool stats are computed on-demand by `StateStore` query methods and do not require separate storage.

## Contracts

These rules are enforced by the state machine and verified by 105 tests including proptest:

- **Forward-only transitions:** `RunStatus` and `JobStatus` only progress forward. Backward transitions return `Err`.
- **Idempotent same-status:** Re-applying the current status succeeds without error (handles duplicate webhooks).
- **First-sight creation:** Events for unknown IDs create entities on the spot (handles out-of-order delivery).
- **Snapshot step semantics:** `Vec<Step>` is fully replaced on each `JobEvent`, never appended.
- **Index consistency:** Every job appears in exactly one `jobs_by_repo` set and one `jobs_by_run` set. `assert_invariants()` (test-only) verifies this.
- **Eviction safety:** Only completed jobs past TTL are evicted. Active jobs are never removed regardless of age.

## Testing

```bash
cargo test -p atc-core        # 105 tests including proptest (256 random cases)
cargo clippy -p atc-core -- -D warnings
```

The `test-support` feature exposes `TestClock` for deterministic time in downstream crate tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain Model
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
