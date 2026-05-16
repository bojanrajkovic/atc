# CLAUDE.md — atc-core

Last verified: 2026-05-15

> Canonical documentation lives in `docs/architecture/backend-server.md` (Domain Model section). This file provides crate-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Pure domain types, state machine transition rules, and business logic for ATC. Source-agnostic — no GitHub-specific dependencies, no tokio, no interior mutability. The `atc-github` crate maps webhook payloads into these domain types. `predecessors_of()` methods enable predicated UPSERTs in `atc-server::persist::PgStore`. All stateful persistence concerns (HashMap state, RwLock, seq counter, TTL eviction task) live in `atc-server::persist` (issue #69 resolved this layering).

## Modules

| Module | Role |
|--------|------|
| `types` | Newtypes: `RunId`, `JobId`, `RepoKey`, `LabelSet`; `RunnerPoolStats` (frontend-derived; `total: RunnerPoolTotal`, no elasticity flag); `RunnerPoolTotal` (adjacent-tagged enum `Bounded(u32) | Unbounded | Undeclared`); `RunnerPoolCapacity { labels: LabelSet, capacity: Option<u32> }` (operator-declared, surfaced on `StateSnapshot.runnerPoolCapacities`; `None` = `capacity: null` = unbounded; struct-level custom `Deserialize` on `RunnerPoolCapacity` enforces explicit-`capacity`-key presence and rejects unknown fields — `atc-server` deserializes YAML directly into this type, so there is no separate config-side mirror type) |
| `run` | `WorkflowRun`, `RunStatus`, `RunConclusion`; `RunStatus::predecessors_of(target)` |
| `job` | `Job`, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`; `JobStatus::predecessors_of(target)` |
| `event` | `RunEvent`, `JobEvent` and their envelope structs |
| `state_machine` | Pure free functions: `apply_run_event(Option<WorkflowRun>, RunEventEnvelope) -> Result<WorkflowRun, StateMachineError>`, `apply_job_event(Option<Job>, JobEventEnvelope) -> Result<Job, StateMachineError>`, and `is_evictable(&Job, DateTime<Utc>, Duration) -> bool`. No locks, no async, no shared state — `atc_store_mem::InMemoryStore` wraps these with its HashMap + RwLock. |
| `persist` | `PersistError` with `InvalidTransition` and `Backend(Box<dyn Error>)` variants. The `PersistentStore` trait lives in `atc-server::persist` (ADR 0005). |
| `clock` | `Clock` trait (wall-clock only — monotonic latency stays direct, see the trait doc-comment), `SystemClock`, `TestClock` and `fixed_test_timestamp` (both behind `test-support` feature) |

## TypeScript Generation

All public domain types derive `#[derive(TS)]` with `#[ts(export)]`. Generated types are written to `frontend/src/lib/types/generated/` via `just types`. See `docs/architecture/backend-server.md` § Frontend Type Generation for serialization format and adjacently-tagged enum encoding.

## Contracts

Enforced by the pure transition functions and verified by tests including proptest:

- **Forward-only transitions:** `RunStatus` and `JobStatus` only progress forward. Backward transitions return `Err`.
- **Idempotent same-status:** Re-applying the current status succeeds (handles duplicate webhooks).
- **First-sight creation:** `apply_*_event(None, env)` creates a new entity from the envelope (handles out-of-order delivery — jobs may arrive before runs).
- **Snapshot step semantics:** `Vec<Step>` is fully replaced on each `JobEvent`, never appended.
- **Eviction predicate:** `is_evictable(&Job, now, ttl)` returns `true` only for jobs whose status is `Completed` AND whose `completed_at + ttl < now`. Active jobs (queued/waiting/in-progress) are never evictable regardless of age. Server-side iteration + index updates live in `atc_store_mem::InMemoryStore::evict_expired`.
- **Conclusion ↔ status invariant:** If `conclusion.is_some()` then `status == Completed`. Verified by atc-core property tests over random event sequences.
- **Predecessor predicates:** `predecessors_of(target)` returns `&'static [Self]` including the target itself. `atc-server::persist::PgStore` parameterizes SQL WHERE clauses with this slice for predicated UPSERTs; including the target enables idempotent replay.

Index-consistency invariants (every job in `jobs_by_run` under its `run_id`, every job in exactly one `jobs_by_repo` set, no empty index entries) are owned by `InMemoryStore` and verified by its test-only `assert_invariants()` impl in `atc-store-mem` (gated behind the `test-support` feature).

## Testing

```bash
cargo nextest run -p atc-core    # pure-function tests + proptest invariants
cargo clippy -p atc-core -- -D warnings
```

The `test-support` feature exposes `TestClock` and `fixed_test_timestamp()` for deterministic time in downstream crate tests. Test fixtures across the workspace use `fixed_test_timestamp()` for event-envelope timestamps (`created_at`, `started_at`, …) so failures are reproducible run-over-run; a `disallowed-methods` clippy lint (see `backend/clippy.toml`) blocks new direct `Utc::now` / `SystemTime::now` calls in either production or fixture code.

## Key References

- Architecture: `docs/architecture/backend-server.md` § Domain Model
- Design plan: `docs/design-plans/2026-04-09-core-domain-model.md`
