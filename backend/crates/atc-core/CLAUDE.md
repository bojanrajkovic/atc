# CLAUDE.md — atc-core

Last verified: 2026-04-11

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Core domain types, state store, and business logic for ATC. Source-agnostic — no GitHub-specific dependencies. The `atc-github` crate maps webhook payloads into these domain types.

## Modules

| Module | Role |
|--------|------|
| `types` | Newtypes: `RunId`, `JobId`, `RepoKey`, `LabelSet` |
| `run` | `WorkflowRun`, `RunStatus`, `RunConclusion`, state transitions |
| `job` | `Job`, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`, state transitions |
| `event` | `RunEvent`, `JobEvent` and their envelope structs |
| `store` | `StateStore` — in-memory state with event ingestion, queries, pool stats, TTL eviction |
| `clock` | `Clock` trait, `SystemClock`, `TestClock` (behind `test-support` feature) |

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
