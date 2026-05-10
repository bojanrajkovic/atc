# CLAUDE.md — atc-core

Last verified: 2026-05-08

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Core domain types, state machine, and business logic for ATC. Source-agnostic — no GitHub-specific dependencies. The `atc-github` crate maps webhook payloads into these domain types. `predecessors_of()` methods enable predicated UPSERTs in `atc-server::persist::PgStore`.

## Modules

| Module | Role |
|--------|------|
| `types` | Newtypes: `RunId`, `JobId`, `RepoKey`, `LabelSet`; `RunnerPoolStats` struct |
| `run` | `WorkflowRun`, `RunStatus`, `RunConclusion`; `RunStatus::predecessors_of(target)` |
| `job` | `Job`, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`; `JobStatus::predecessors_of(target)` |
| `event` | `RunEvent`, `JobEvent` and their envelope structs |
| `state_machine` | `RunStateMachine` — in-memory state with event ingestion, `snapshot()` (atomic consistent read returning `QueryResult { runs, jobs }`), and TTL eviction. `apply_run_event()` and `apply_job_event()` are inherent methods. |
| `persist` | `PersistError` with `InvalidTransition` and `Backend(Box<dyn Error>)` variants. The `PersistentStore` trait lives in `atc-server::persist` (ADR 0005). |
| `clock` | `Clock` trait, `SystemClock`, `TestClock` (behind `test-support` feature) |

## TypeScript Generation

All public domain types derive `#[derive(TS)]` with `#[ts(export)]`. Generated types are written to `frontend/src/lib/types/generated/` via `just types`. See `docs/architecture/backend-server.md` § Frontend Type Generation for serialization format and adjacently-tagged enum encoding.

## Contracts

Enforced by the state machine and verified by tests including proptest:

- **Forward-only transitions:** `RunStatus` and `JobStatus` only progress forward. Backward transitions return `Err`.
- **Idempotent same-status:** Re-applying the current status succeeds (handles duplicate webhooks).
- **First-sight creation:** Events for unknown IDs create entities on the spot (handles out-of-order delivery).
- **Snapshot step semantics:** `Vec<Step>` is fully replaced on each `JobEvent`, never appended.
- **Index consistency:** Every job appears in exactly one `jobs_by_repo` set and one `jobs_by_run` set. `assert_invariants()` (test-only) verifies this.
- **Eviction safety:** Only completed jobs past TTL are evicted. Active jobs are never removed regardless of age.
- **Snapshot read shape:** `snapshot()` returns `QueryResult { runs, jobs }`. Pool stats are derived on the frontend (ADR 0004).
- **Predecessor predicates:** `predecessors_of(target)` returns `&'static [Self]` including the target itself. `atc-server::persist::PgStore` parameterizes SQL WHERE clauses with this slice for predicated UPSERTs; including the target enables idempotent replay.

## Testing

```bash
cargo test -p atc-core        # ~131 tests including proptest
cargo clippy -p atc-core -- -D warnings
```

The `test-support` feature exposes `TestClock` for deterministic time in downstream crate tests.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain Model
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
