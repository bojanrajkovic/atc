# Core Domain Model Design

## Summary

The core domain model establishes the foundational data layer for ATC. It introduces owned Rust types for GitHub Actions entities -- workflow runs, jobs, steps, and runner pools -- that live entirely within `atc-core` with no dependency on GitHub's API shapes. These types are organized as a three-level hierarchy (run -> job -> step) mirroring GitHub's own model, with separate ID newtypes and explicit status/conclusion enums to make illegal states unrepresentable at the type system level.

The central artifact is an in-memory `StateStore` backed by a `tokio::sync::RwLock` that accepts domain events, applies validated state machine transitions, maintains secondary indexes for efficient querying, and evicts completed entries after a configurable TTL. The store is designed as a pure domain layer: it speaks in abstract domain events rather than raw webhook payloads (that translation is deferred to `atc-github` in Phase 8), and it exposes access-controlled queries by org/repo key that feed the WebSocket broadcast layer in Phase 9. Runner pool statistics are computed as derived views over live job state rather than stored separately, keeping the store's write path simple while enabling accurate capacity reporting.

## Definition of Done

**Primary deliverables:**
- Rich domain types in `atc-core` modeling workflow runs, jobs, steps, and runner pools -- normalized and composition-oriented for future DB storage
- A state machine governing job lifecycle transitions (queued -> running -> completed/failed/cancelled)
- An in-memory state store that ingests domain events, maintains current state, and supports configurable TTL eviction of completed runs
- Derived runner pool stats (running/queued counts per label set)
- Domain event types (e.g., `RunEvent` enum) that atc-github will map webhook payloads into (Phase 8)

**Success criteria:**
- `cargo test -p atc-core` passes with comprehensive unit tests covering state transitions, TTL eviction, and edge cases
- Models are self-contained -- no GitHub-specific types leak into atc-core
- State store is queryable by org/repo for filtered WebSocket broadcast (Phase 9)

**Out of scope:**
- Raw webhook JSON parsing (atc-github, Phase 8)
- WebSocket/HTTP layer (atc-server, Phase 9)
- Database persistence (issue #7)
- Configurable runner pool capacity (issue #16)

## Acceptance Criteria

### core-domain.AC1: Domain types model runs, jobs, steps, and runner pools
- **core-domain.AC1.1 Success:** WorkflowRun, Job, and Step structs compile with all fields from the design (identity, lifecycle, timestamps, git context)
- **core-domain.AC1.2 Success:** RunId and JobId newtypes prevent accidental cross-use (different types, not interchangeable)
- **core-domain.AC1.3 Success:** LabelSet normalizes and deduplicates labels -- `["linux", "self-hosted"]` equals `["self-hosted", "linux"]`
- **core-domain.AC1.4 Success:** All domain types derive Serialize/Deserialize and round-trip through JSON
- **core-domain.AC1.5 Success:** RunnerInfo is a composed struct with id, name, group_id, group_name

### core-domain.AC2: State machine governs job lifecycle transitions
- **core-domain.AC2.1 Success:** Valid transitions succeed: Queued->InProgress, InProgress->Completed (for each conclusion)
- **core-domain.AC2.2 Success:** Run transitions succeed: Queued->InProgress->Completed
- **core-domain.AC2.3 Failure:** Invalid transitions return `Err(InvalidTransition)` -- e.g., Completed->InProgress, Queued->Completed
- **core-domain.AC2.4 Edge:** Idempotent re-application of same status updates fields without erroring

### core-domain.AC3: In-memory state store ingests events and maintains state
- **core-domain.AC3.1 Success:** RunEvent creates/updates WorkflowRun entries in the store
- **core-domain.AC3.2 Success:** JobEvent creates/updates Job entries with correct run_id back-reference
- **core-domain.AC3.3 Success:** Secondary indexes (jobs_by_run, jobs_by_repo) stay consistent after insert/update/remove
- **core-domain.AC3.4 Success:** `Vec<Step>` is fully replaced on each JobEvent (snapshot semantics)
- **core-domain.AC3.5 Edge:** Event for unknown job creates it on first sight (out-of-order tolerance)
- **core-domain.AC3.6 Edge:** Duplicate events are handled idempotently

### core-domain.AC4: Store is queryable by org/repo with derived runner pool stats
- **core-domain.AC4.1 Success:** Query with repo key set returns only jobs for those repos
- **core-domain.AC4.2 Success:** Query returns owned snapshots (no references into the lock)
- **core-domain.AC4.3 Success:** Runner pool stats report correct queued/running counts per label set
- **core-domain.AC4.4 Success:** Pool stats include group_name from most recently observed RunnerInfo
- **core-domain.AC4.5 Failure:** Query with empty repo set returns no jobs
- **core-domain.AC4.6 Edge:** Multi-repo query isolates results -- repo A jobs don't appear in repo B results

### core-domain.AC5: TTL eviction removes completed runs
- **core-domain.AC5.1 Success:** Completed job within TTL is retained
- **core-domain.AC5.2 Success:** Completed job past TTL is evicted from primary map and all indexes
- **core-domain.AC5.3 Success:** Run with no remaining jobs is also evicted
- **core-domain.AC5.4 Failure:** Active jobs (queued/running) are never evicted regardless of age
- **core-domain.AC5.5 Success:** TTL duration is configurable
- **core-domain.AC5.6 Success:** Clock trait enables deterministic time in tests

### core-domain.AC6: Store invariants hold under arbitrary event sequences
- **core-domain.AC6.1 Success:** Every job in jobs_by_repo exists in jobs primary map, and vice versa
- **core-domain.AC6.2 Success:** Every job's run_id points to an existing run (no orphans)
- **core-domain.AC6.3 Success:** Completed jobs never revert to running under random event sequences
- **core-domain.AC6.4 Success:** Eviction never removes active jobs under random event sequences
- **core-domain.AC6.5 Edge:** Out-of-order, duplicate, and unknown-ID events don't panic or corrupt state

### core-domain.AC7: No GitHub-specific types in atc-core
- **core-domain.AC7.1 Success:** atc-core has no dependency on atc-github or any GitHub API crate
- **core-domain.AC7.2 Success:** Domain event types are source-agnostic (could be produced by non-GitHub CI systems)

## Glossary

- **`atc-core`**: The Rust crate in this workspace that owns all domain types and business logic. Has no dependencies on GitHub-specific libraries.
- **`atc-github`**: Sibling crate responsible for parsing raw GitHub webhook JSON and translating it into `atc-core` domain events. Not implemented in this phase.
- **`atc-server`**: Sibling crate providing the Axum HTTP and WebSocket layer. Consumes the `StateStore` exposed by `atc-core`.
- **Clock trait / `TestClock`**: A dependency-injection pattern where the store accepts an abstract `Clock` instead of calling system time directly. `TestClock` allows advancing time manually without sleeping.
- **Domain event**: A value type describing something that happened (e.g., `JobQueued`, `RunCompleted`). The state store is fed events; it never receives mutable entity references directly.
- **Idempotent**: An operation that produces the same result when applied multiple times. Required here because GitHub webhook delivery is at-least-once.
- **`InvalidTransition`**: A domain error returned when an event would move an entity to an illegal state (e.g., a completed job being marked in-progress).
- **`LabelSet`**: A newtype wrapping `BTreeSet<String>` that normalizes runner label arrays for equality comparison and use as a map key.
- **Newtype**: A Rust pattern of wrapping a primitive in a single-field struct (e.g., `RunId(i64)`) to give it a distinct type at compile time.
- **Property-based testing / `proptest`**: A testing approach where the framework generates hundreds of random inputs and checks that invariants hold across all of them.
- **`RepoKey`**: A `(org, repo)` pair used as the filter unit for access-controlled queries.
- **`RunnerInfo`**: A composed struct holding the identity of the runner that picked up a job. Optional on `Job` because it is only populated once a runner is assigned.
- **`RunnerPoolStats`**: A derived view computed from live job state. Reports queued and running counts per unique label set.
- **Secondary index**: An auxiliary data structure (`jobs_by_run`, `jobs_by_repo`) that allows efficient lookup by a non-primary key.
- **Snapshot semantics**: The `Vec<Step>` inside a `Job` is fully replaced on every incoming event rather than merged.
- **TTL (time-to-live)**: A duration after which completed entries are eligible for eviction from the store.
- **`RwLock`**: A reader-writer lock allowing multiple concurrent readers or one exclusive writer. `tokio::sync::RwLock` is the async variant used here.
- **`workflow_job` / `workflow_run`**: The two GitHub webhook event types ATC subscribes to. `workflow_run` maps to `RunEvent`; `workflow_job` maps to `JobEvent`.

## Architecture

### Domain Type Hierarchy

Three entity levels mirror GitHub's own model using ATC-owned types (no GitHub-specific types in atc-core):

**WorkflowRun** -- top-level container created/updated by `RunEvent`s:
- Identity: `RunId(i64)`, org, repo, workflow name, workflow path
- Git context: branch, head SHA, commit message, triggering event (push/PR/schedule), display title
- Lifecycle: `RunStatus` enum (`Queued`, `InProgress`, `Completed`) + `RunConclusion` enum (`Success`, `Failure`, `Cancelled`, `TimedOut`, etc.)
- Timestamps: `created_at`, `run_started_at`, `updated_at`
- Link: `html_url` for one-click navigation to GitHub

**Job** -- owned by a run, created/updated by `JobEvent`s:
- Identity: `JobId(i64)`, name, `run_id` back-reference
- Lifecycle: `JobStatus` enum (`Queued`, `Waiting`, `InProgress`, `Completed`) + `JobConclusion` enum (same variants as run)
- Runner: optional `RunnerInfo { id, name, group_id, group_name }` -- populated when runner assigned
- Labels: `Vec<String>` -- runner labels this job requires
- Steps: `Vec<Step>` -- ordered by step number, full snapshot replacement on each event
- Timestamps: `created_at`, `started_at`, `completed_at`

**Step** -- value object inside a Job:
- Identity: `number` (position), `name`
- Lifecycle: `StepStatus` enum (`Queued`, `InProgress`, `Completed`), optional conclusion
- Timestamps: `started_at`, `completed_at`

### Key Type Design Choices

- **Status and Conclusion are separate enums** -- mirrors GitHub's model, avoids combinatorial explosion
- **All IDs are newtypes** (`RunId(i64)`, `JobId(i64)`) for type safety
- **`RunnerInfo` is a composed struct**, not flattened fields -- enables runner pool derivation
- **Jobs own Steps as `Vec<Step>`** -- steps are never queried independently, always in job context
- **Timestamps are `chrono::DateTime<Utc>`** -- serializable, timezone-aware, matches GitHub's ISO 8601
- **`LabelSet` newtype wraps `BTreeSet<String>`** -- deterministic ordering, deduplication, `Eq + Hash` for use as map key

### Domain Events

Domain events are the input boundary. `atc-github` (Phase 8) will parse raw webhook JSON and produce these; the state store consumes them.

**`RunEvent`** -- from `workflow_run` webhooks:
- `RunRequested` -- new run appeared
- `RunInProgress` -- run started executing
- `RunCompleted { conclusion }` -- run finished

**`JobEvent`** -- from `workflow_job` webhooks:
- `JobQueued { labels, steps }` -- job entered the queue
- `JobInProgress { runner_info, steps }` -- job started on a runner
- `JobCompleted { conclusion, runner_info, steps }` -- job finished

Each event variant carries only data that changes at that transition. Common identity (run_id, job_id, org, repo, timestamps) lives in a shared envelope struct.

### State Store

Central in-memory container behind a `tokio::sync::RwLock`:

```
StateStore {
    inner: RwLock<StoreInner>
}

StoreInner {
    runs: HashMap<RunId, WorkflowRun>,
    jobs: HashMap<JobId, Job>,
    jobs_by_run: HashMap<RunId, HashSet<JobId>>,
    jobs_by_repo: HashMap<RepoKey, HashSet<JobId>>,
    completed_ttl: Duration,
}
```

- **Runs and Jobs are separate HashMaps** -- O(1) lookup by JobId for every webhook, avoids nested borrow issues. `jobs_by_run` provides parent-child relationship.
- **`RepoKey` is `(org, repo)`** -- primary filter for access-controlled queries.
- **No `by_status` or `by_runner` secondary indexes** -- job counts per repo are small enough that linear scan is faster than maintaining indexes that churn on every state change.

**Mutation flow:** Event arrives -> acquire write lock -> look up or create entity -> apply state transition via enum match (reject invalid transitions) -> update indexes if needed -> release lock.

**Query flow:** Caller provides set of `RepoKey`s (from user's verified access list) -> acquire read lock -> collect jobs via `jobs_by_repo` index -> include parent run data -> release lock, return owned snapshot.

**TTL eviction:** Background `tokio::spawn` task on configurable interval (e.g., 60s). Acquires write lock, scans for completed jobs where `completed_at + ttl < now`, removes from primary map and all indexes. Runs with no remaining jobs are also removed.

**Clock injection:** Store accepts a `Clock` trait for time, with `SystemClock` (production) and `TestClock` (tests with manually advanced time) implementations.

### State Machine Transitions

Plain enum-based, no external crate. Rust's exhaustive pattern matching ensures all states are handled:

```rust
impl JobStatus {
    fn apply(self, event: &JobEvent) -> Result<JobStatus, InvalidTransition> {
        match (self, event) {
            (Queued, JobEvent::Started { .. }) => Ok(InProgress),
            (InProgress, JobEvent::Completed { .. }) => Ok(Completed),
            // ...
            (from, event) => Err(InvalidTransition { from, event }),
        }
    }
}
```

Idempotent re-application of the same status updates fields (timestamps, steps) without erroring.

### Runner Pool Derivation

Runner pools are derived views, not stored entities. Computed on read by iterating jobs and grouping by label set:

- **Queued count:** Jobs with `Queued` status, grouped by `labels` (what they request)
- **Running count:** Jobs with `InProgress` status, grouped by runner's labels (where they're actually running)

Result is `Vec<RunnerPoolStats>` with label set, queued count, running count, and optional `group_name` (from most recently observed `runner_group_name` for that label set).

`LabelSet` normalization via `BTreeSet<String>` ensures `["linux", "self-hosted"]` and `["self-hosted", "linux"]` map to the same pool.

## Existing Patterns

Investigation found `atc-core` is a blank slate -- empty `lib.rs` with lint attributes, no dependencies. No existing domain patterns to follow within atc-core.

The server crate (`atc-server`) establishes these patterns that the domain model should integrate with:
- **Tokio async runtime** with `full` features -- state store uses `tokio::sync::RwLock` and `tokio::spawn` for eviction
- **Serde serialization** -- domain types derive `Serialize`/`Deserialize` for WebSocket JSON (Phase 9)
- **Figment configuration** -- TTL and eviction interval will be configurable via `ATC_*` env vars (wired in Phase 9)
- **Strict lints** -- `#![deny(missing_docs)]` and `clippy::pedantic` already enforced on atc-core

No divergence from existing patterns. The domain model introduces new modules in an empty crate.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Core Types and Newtypes
**Goal:** Establish the foundational type system -- IDs, enums, and shared types that all other modules depend on.

**Components:**
- `types.rs` in `backend/crates/atc-core/src/` -- `RunId`, `JobId`, `RepoKey`, `LabelSet` newtypes
- `run.rs` in `backend/crates/atc-core/src/` -- `WorkflowRun` struct, `RunStatus`, `RunConclusion` enums
- `job.rs` in `backend/crates/atc-core/src/` -- `Job` struct, `JobStatus`, `JobConclusion`, `Step`, `StepStatus`, `RunnerInfo`
- `clock.rs` in `backend/crates/atc-core/src/` -- `Clock` trait, `SystemClock`, `TestClock`
- `lib.rs` updated with module declarations and re-exports
- `Cargo.toml` updated with dependencies: `chrono` (serde), `serde` (derive), `tokio` (sync, time)

**Dependencies:** None (first phase)

**Done when:** `cargo check -p atc-core` compiles, all types have doc comments, serde derives work. Covers `core-domain.AC1.*` (domain types).
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Domain Events and State Transitions
**Goal:** Event types and state machine transition logic with comprehensive tests.

**Components:**
- `event.rs` in `backend/crates/atc-core/src/` -- `RunEvent`, `JobEvent` enums, event envelope struct
- Transition methods on `RunStatus` and `JobStatus` -- `apply()` returning `Result<Self, InvalidTransition>`
- Unit tests for all valid transitions, invalid transitions, and idempotent re-application

**Dependencies:** Phase 1 (types)

**Done when:** All state transition tests pass, including invalid transition rejection and idempotent updates. Covers `core-domain.AC2.*` (state transitions).
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: State Store -- Core Operations
**Goal:** In-memory state store that ingests events and maintains indexed state.

**Components:**
- `store.rs` in `backend/crates/atc-core/src/` -- `StateStore`, `StoreInner`, event ingestion methods
- Secondary indexes: `jobs_by_run`, `jobs_by_repo`
- Integration tests: insert runs/jobs via events, verify index consistency, multi-repo isolation

**Dependencies:** Phase 2 (events and transitions)

**Done when:** Store correctly processes event sequences, indexes stay consistent, queries by repo return correct results. Covers `core-domain.AC3.*` (state store operations).
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: State Store -- Queries and Runner Pools
**Goal:** Query interface for WebSocket broadcast and derived runner pool stats.

**Components:**
- Query methods on `StateStore` -- filter by repo key set, return owned snapshots
- Runner pool derivation -- `pool_stats()` method computing `RunnerPoolStats` from job state
- `LabelSet` grouping and `group_name` tracking from `RunnerInfo`
- Tests for query filtering, pool stat derivation, label set normalization

**Dependencies:** Phase 3 (store core)

**Done when:** Queries return correct filtered results, runner pool stats match expected counts, label normalization works. Covers `core-domain.AC4.*` (queries and runner pools).
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: TTL Eviction
**Goal:** Background eviction of completed runs/jobs past TTL, with `Clock` trait for testability.

**Components:**
- Eviction logic in `store.rs` -- scan and remove expired completed jobs and empty runs
- Background task spawning -- `StateStore::start_eviction_task()` using `tokio::spawn` and `tokio::time::interval`
- `TestClock` usage in eviction tests -- deterministic time advancement
- Tests: within-TTL retained, past-TTL evicted, active jobs never evicted, empty runs cleaned up

**Dependencies:** Phase 4 (queries -- eviction tests verify queries still work after eviction)

**Done when:** Eviction correctly removes expired entries, indexes stay consistent after eviction, active jobs unaffected. Covers `core-domain.AC5.*` (TTL eviction).
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: Property Tests and Edge Cases
**Goal:** Property-based tests for store invariants under random event sequences, plus edge case coverage.

**Components:**
- `proptest` added as dev-dependency
- Arbitrary strategies for domain events (random IDs, label sets, step counts, event sequences)
- Invariant assertions: index consistency, no orphans, status monotonicity, eviction safety
- Edge case tests: out-of-order events, duplicate events, unknown job IDs, events for missing runs

**Dependencies:** Phase 5 (full store functionality)

**Done when:** Property tests pass with default proptest config (256 cases), edge cases handled gracefully. Covers `core-domain.AC6.*` (invariants and edge cases).
<!-- END_PHASE_6 -->

## Documents to Update

| Document | Update Required |
|----------|----------------|
| `CLAUDE.md` | Add atc-core module descriptions to Project Structure |
| `docs/architecture/backend-server.md` | Add domain model section, state store architecture |
| `scripts/doc-mapping.sh` | Add mapping: `backend/crates/atc-core/**` -> `docs/architecture/backend-server.md` |

## Additional Considerations

**Webhook edge cases:** GitHub webhook delivery is at-least-once with no ordering guarantee. The store must handle: events for unknown jobs (create on first sight), duplicate events (idempotent application), and out-of-order delivery (e.g., `JobCompleted` before `JobQueued` -- create the job in completed state rather than rejecting).

**Step data is snapshot-based.** GitHub only sends step arrays at job-level transitions, not per-step. The store replaces `Vec<Step>` entirely on each event. Live per-step polling is tracked as a future enhancement (issue #17).

**Future DB storage.** Types use `serde` derives and normalized relationships (separate maps with ID references rather than nested ownership) specifically to ease future persistence. The `StateStore` trait boundary can be extracted when issue #7 is addressed.
