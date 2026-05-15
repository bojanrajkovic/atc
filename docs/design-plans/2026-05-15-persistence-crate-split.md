# Persistence layer crate split (issue #169)

**Slug:** `persistence-crate-split` — final filename in repo: `docs/design-plans/2026-05-15-persistence-crate-split.md`.
**Branch:** `feat/persistence-crate-split`.
**Issue:** [#169 — refactor: extract atc-persist + atc-store-{pg,mem} crates](https://github.com/bojanrajkovic/atc/issues/169).

## Context

Today the entire persistence layer lives inside `atc-server`. The trait `PersistentStore` is in `backend/crates/atc-server/src/persist/mod.rs:54-101`; the wire types `SeqEvent` and `StateSnapshot` are in `backend/crates/atc-server/src/state.rs:56-87`; the in-memory store is in `persist/in_memory.rs` (548 LOC); the Postgres store is in `persist/pg.rs` (1,529 LOC after PR #192's outbox retention work) plus `listener.rs` (580 LOC), `reads.rs` (313 LOC), `db.rs` (8 LOC), and four migrations under `migrations/`. PG-mode and in-memory mode both compile into every binary, even though only one is active at runtime.

Issue #163 ("eviction task machinery should live inside the persistence store") deferred the crate-split question to this issue. A 7-worker / 3-critic exploration (final report at `/tmp/claude-c5b5ffff-b66a-4445-b742-8e5a2fdee59a/fanout-issue169/final-report.md`) converged on a four-crate split with deeply litigated decisions on wire-type placement, trait shape, async-trait promotion, error-shape resolution, test reorganization, and migration sequencing. v1 placed wire types in `atc-persist`; the user pushed back ("wire types in the persistence store feels weird"); follow-up audits surfaced a dedicated `atc-wire` crate as the cleanest answer. v2 closed that question.

Three things have shifted since v2 was written (verified against the current worktree on `main`):

- `pg.rs` has grown from 982 → 1,529 lines via ADR-0007's outbox heartbeat + sweep + retention metrics. Step 2's diff is correspondingly larger.
- `metrics.rs` has grown from ~200 → 487 lines (PgMetrics struct + comprehensive instrument caching).
- A fourth migration `0004_outbox_watermarks.sql` exists alongside the original three.
- `async-trait` is currently pinned per-crate (`atc-server/Cargo.toml:16` → `=0.1.89`); not yet in `[workspace.dependencies]`.

Today's constraint: **`SeqEvent` and `StateSnapshot` live above both store crates' eventual home**. They cannot be peers of `atc-store-{pg,mem}` and must move into a downstream crate that both can name. Combined with the wire-types-in-trait-crate concern, this lands on a four-crate split: `atc-wire` (data types), `atc-persist` (interface + `LivenessError` opaque-box fix), and the two stores.

The intended outcome: PG-mode pods stop compiling the in-memory state machine and vice versa; the trait crate carries no GitHub or sqlx coupling; the test surface stays buildable in one PR per step; and a follow-up issue can later restructure tests to make `cargo test -p atc-store-pg` actually meaningful (deferred — see Out of Scope).

## Definition of Done

**Primary deliverables** (in shipping order):

1. **Phase 0 pre-flight PR**: `release-please-config.json` + `.release-please-manifest.json` updated for four new packages at `"0.1.0"`; `scripts/doc-mapping.sh` updated with case-ordered patterns; root `CLAUDE.md` and `CONTRIBUTING.md` contradiction resolved (Option C two-tier framing); ADR-0005 annotation sweep.
2. **Phase 1 PR — `atc-wire` + `atc-persist`** with the `SeqEvent → CommittedEvent` rename (Rust + frontend), `LivenessError` opaque-box conversion + `impl Error::source()`, `async-trait` promotion to `[workspace.dependencies]`, ADR-0008 (crate-split decision record).
3. **Phase 2 PR — `atc-store-pg`** with PG-B file decomposition (`store/{mod, writes, test_hooks, retention}.rs`) + `invariants.rs` + `PgMetrics` extraction (`register_build_info` and `spawn_process_collector` stay in `atc-server`).
4. **Phase 3 PR — `atc-store-mem`** including `invariants.rs`, single shared `atc_persist::join_with_timeout` consumed by both stores, and retirement of `atc-server`'s `path = "."` self-ref dev-dep if it has no remaining test-only surface.
5. **Four new `CLAUDE.md` + `AGENTS.md` symlink pairs**, one per new crate, following the two-tier convention (skeleton + reactive sharp edges).
6. **Architecture docs updated**: `docs/architecture/backend-server.md` and `docs/architecture/metrics.md` reflect new crate locations.
7. **Frontend rename**: `frontend/src/lib/types/generated/SeqEvent.ts` removed, `CommittedEvent.ts` generated; 4 production .ts files + 7 test files updated by mechanical identifier rename.

**Success criteria**:

- After every step, `just test`, `just lint`, `just fmt`, `just check`, `just types`, `just build` are green.
- `cargo sqlx prepare --check` passes — no `.sqlx/` regeneration required by file moves.
- **`atc-persist/Cargo.toml`**'s `[dependencies]` contains exactly: atc-core, atc-wire, async-trait, tokio (`["sync", "time", "rt"]`), tracing. Disallowed entries (sqlx, serde, ts-rs, atc-github, redis, mongodb) absent.
- **`atc-store-mem/Cargo.toml`**'s `[dependencies]` contains atc-github (required for WebhookEvent construction) but NO sqlx (in-memory store does no DB I/O).
- **Verification method**: manifest inspection, NOT `cargo tree`. `cargo tree -p atc-persist` would show transitive serde/ts-rs/atc-github deps inherited from atc-wire, which doesn't reflect direct-dep correctness.
- `pnpm test` (jsdom + browser projects) green after the rename; `pnpm exec tsc --noEmit` zero errors.
- Doc-staleness gate (`scripts/check-docs-lefthook.sh`) green on every PR push.
- Each step is independently reverttable on `main` without breaking the regression net.
- All ten OC-1 through OC-10 ordering constraints (see Architecture § "Ordering constraints") satisfied per step.

**Key exclusions** — see "Out of Scope" for full list. Headlines: behavioral test files exceeding 500 lines (`in_memory_store_tests.rs`, `outbox_tests.rs`); a shared `atc-test-helpers` crate with per-store `tests/integration/` directories; decoupling `SeqEvent.event` from `WebhookEvent`; PG-off / mem-off feature-gating; native dyn-safe AFIT migration.

## Locked Decisions

These were established by the 7-worker / 3-critic / 1-synthesizer exploration and the user-Q&A rounds in plan-mode. Not open for re-evaluation in implementation.

| Decision | Source |
|---|---|
| Four crates: `atc-wire`, `atc-persist`, `atc-store-pg`, `atc-store-mem`. | Final report v2 § 1, § 3.2 (wire-type Option D). |
| Wire types live in `atc-wire` (flat module — not `atc-wire::wire::*`). | Final report v2 § 3.2 (atc-wire crate decision). Flat-module convention is a **plan-mode Phase 4 brainstorming refinement** (not from v2): Tuvok C3 § 1.1 originally suggested a `wire::*` submodule under the v1 "wire types in atc-persist" decision; under v2's atc-wire as a separate crate, the namespace is already isolated, so flat is cleaner. |
| `atc-persist` carries no `atc-github`, `serde`, or `ts-rs` direct deps; only `atc-core`, `atc-wire`, `tokio` (constrained surface `["sync", "time", "rt"]`), `async-trait`, `tracing`. | Final report v2 § 1 table; Spock C1 § 5; **`tracing` added per Codex blocker 2 — required by `join_with_timeout`'s log calls**. |
| `subscribe()` stays on the `PersistentStore` trait; bus inversion rejected. ADR-0006 preserved. | Final report v2 § 3.2 (rejection of bus inversion); La Forge `W3-followups.md` § 6. |
| Monolithic six-method `PersistentStore` trait. | Final report v1 § 3.4. |
| `LivenessError::DbUnreachable(Box<dyn std::error::Error + Send + Sync + 'static>)` + `impl std::error::Error::source()`. | Final report v1 § 3.3, Tuvok C3 § 4. |
| `async-trait` promoted to `[workspace.dependencies]` in `backend/Cargo.toml` in same PR as `atc-persist`. | Tuvok C3 § 5 (mandatory). |
| Rename `SeqEvent → CommittedEvent` in Step 1 (atomically with the move). | User confirmation (plan-mode Q4). |
| Test reorg follows Option A — unified test binary stays in `atc-server`. `atc-test-helpers` deferred to follow-up issue. | User confirmation (plan-mode Q1, "B-Lite"). |
| Migration sequencing: Scotty's 3-step + Step 0 (4 PRs total). | User confirmation (plan-mode Q2). |
| CLAUDE.md/CONTRIBUTING.md contradiction resolved via Option C — two-tier (mandatory skeleton + reactive sharp edges). Both docs updated in same commit (Phase 0). | User confirmation (plan-mode Q3); Tuvok C3 § 7 Rule 3. |
| Fold PG-B (split `pg.rs` into `store/{mod, writes, test_hooks, retention}`) into Phase 2. | User confirmation (plan-mode Q5). |
| Extract `atc-store-pg/src/invariants.rs` (mirrors `atc-store-mem` from #163). | User confirmation (plan-mode Q6). |
| Single shared `atc_persist::join_with_timeout`; per-crate timeout constants stay per-crate. | User confirmation (plan-mode follow-up). |
| All four new crates join `release-please-config.json`'s `linked-versions` "atc" group; seed `.release-please-manifest.json` at `"0.1.0"` (NOT `0.0.0` — bootstrap bug). | User confirmation (DoD round); reference memory `release_please_version_quirks.md`. |
| New ADR is `0008-persistence-crate-split.md` (0007 already taken by outbox-retention-policy). | Verified `ls docs/architecture-decisions/`. |
| `register_build_info` and `spawn_process_collector` stay in `atc-server::metrics`; only `PgMetrics` moves. | Final report v1 § 3.6, § 4.6. |
| `init_pool` and `migrations/` move to `atc-store-pg`. | Final report v1 § 3.6 (DB-A + M-A); Scotty OC-8. |

## Architecture

### Crate dependency graph (post-split)

```
atc-core (pure domain — no tokio, no I/O)
   │
   ├── atc-wire (CommittedEvent, StateSnapshot — derives TS, depends on atc-core + atc-github)
   │      │
   │      └── atc-persist (PersistentStore trait, LivenessError, PersistError re-export, join_with_timeout)
   │             │             ├── tokio (constrained surface: ["sync", "time", "rt"])
   │             │             ├── async-trait
   │             │             └── tracing (used by join_with_timeout for warn!/error! on shutdown)
   │             ├── atc-store-pg (PgStore, PgMetrics, listener, reads, db, retention,
   │             │                 invariants, migrations, DbInitError)
   │             │      └── + sqlx, atc-github
   │             └── atc-store-mem (InMemoryStore, eviction, invariants)
   │                    └── + atc-github  (NO sqlx)
   │
   └── atc-github (already exists)
          └── (consumed by atc-wire AND both stores — both stores construct
               WebhookEvent::Run/Job to populate CommittedEvent.event)

atc-server (executable)
   ├── atc-core
   ├── atc-github
   ├── atc-wire
   ├── atc-persist
   ├── atc-store-pg
   └── atc-store-mem
   (NO direct sqlx — atc-server matches on atc_store_pg::DbInitError now,
    not sqlx::Error::Migrate; sqlx pulled in only transitively via atc-store-pg)
```

`atc-persist` is the interface waist: trait crate + small shared utilities (`join_with_timeout`). Both stores depend on it; `atc-server` depends on it transitively through both. The trait crate's non-domain deps are `tokio` (constrained feature surface — `["sync", "time", "rt"]` — for `broadcast::Receiver`, `JoinHandle`, `time::timeout`), `async-trait`, and `tracing` (the `join_with_timeout` helper logs on timeout/cancellation).

**Why both stores name `atc-github`**: `CommittedEvent.event: atc_github::WebhookEvent` is the broadcast envelope's payload. Each store *constructs* the `WebhookEvent::Run(env)` / `WebhookEvent::Job(env)` value before broadcasting (see `in_memory.rs:366-369, 419-422` and the equivalent sites in `pg.rs`'s drain pipeline). Construction requires naming the type. The alternative — decoupling `CommittedEvent.event` from `WebhookEvent` (introduce an intermediate domain-event enum) — is explicitly out of scope (see "Out of Scope"). Accepting the dep keeps the split at four crates, not six.

### Wire types — `atc-wire`

`backend/crates/atc-wire/src/lib.rs` carries:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct CommittedEvent {
    pub seq: u64,
    pub event: atc_github::WebhookEvent,
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct StateSnapshot {
    pub last_seq: u64,
    pub runs: Vec<atc_core::WorkflowRun>,
    pub jobs: Vec<atc_core::Job>,
    #[serde(default)]
    pub runner_pool_capacities: Vec<atc_core::RunnerPoolCapacity>,
}
```

The rename motivation (v2 § 3.2 sub-decision): at the point of emission, the type has been validated, applied to state, and assigned a monotonic `seq` by the store's commit-order allocator. It is a committed domain event, not a serialization shape. `StateSnapshot` is already well-named and stays.

Rejected alternatives:
- **Option A — wire types stay in `atc-server::state`** (status quo, opaque): forces a Cargo cycle through `WebhookEvent`. (W2 § Fatal flaw.)
- **Option B — wire types in `atc-core`**: `SeqEvent` blocked by `WebhookEvent` cycle; only `StateSnapshot` could move; `runner_pool_capacities` is operator config not domain state. (W2 § asymmetric.)
- **Option C — wire types in `atc-persist`**: technically works but pollutes the interface crate with `serde`, `ts-rs`, and `atc-github` deps. User pushed back: "feels weird to have wire types in the persistence store." (User feedback round.)
- **Tuple decomposition** (replace `broadcast::Receiver<SeqEvent>` with `broadcast::Receiver<(u64, WebhookEvent)>`): mechanically viable but doesn't help `StateSnapshot`; cycle through `atc-github` survives. (Spock C1-followups § 1, Tuvok post-compaction reply.)
- **Bus inversion** (move `subscribe()` off the trait so `atc-server` owns the channel): equivalent runtime behavior; stores still need `SeqEvent` to construct the injected `Sender` so the wire-type precondition doesn't dissolve; `BROADCAST_CAPACITY` becomes a server-layer concern (regression). (La Forge W3-followups § 6.)

### Trait crate — `atc-persist`

`backend/crates/atc-persist/src/lib.rs`:

```rust
pub use atc_core::PersistError;

pub mod join;     // join_with_timeout
pub use join::join_with_timeout;

#[derive(Debug)]
pub enum LivenessError {
    DbUnreachable(Box<dyn std::error::Error + Send + Sync + 'static>),
    DrainStale { age_ms: i64 },
}

impl std::error::Error for LivenessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            LivenessError::DbUnreachable(e) => Some(e.as_ref()),
            LivenessError::DrainStale { .. } => None,
        }
    }
}

impl std::fmt::Display for LivenessError { /* … */ }

#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError>;
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError>;
    async fn read_snapshot(&self) -> Result<atc_wire::StateSnapshot, PersistError>;
    async fn liveness_check(&self) -> Result<(), LivenessError>;
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<atc_wire::CommittedEvent>;
    async fn shutdown(&self);
}
```

`join_with_timeout` is the lifted-from-`atc-server::shutdown` helper (`shutdown.rs:50`). Single shared copy. Per-store crates import it. `atc-server::shutdown` keeps its own use sites (metrics collector, axum serves, ws handlers) and now imports from `atc_persist::join_with_timeout`.

`Cargo.toml` (atc-persist):
```toml
[dependencies]
atc-core = { path = "../atc-core" }
atc-wire = { path = "../atc-wire" }
async-trait = { workspace = true }
tokio = { version = "...", features = ["sync", "time", "rt"] }
tracing = { workspace = true }   # join_with_timeout logs on timeout/cancellation
```

**Disallowed direct deps**: `sqlx`, `atc-github`, `serde`, `ts-rs`. Verified by manifest inspection (the acceptance test reads `atc-persist/Cargo.toml`'s `[dependencies]` table directly — `cargo tree` is not used because atc-wire's transitive deps would mask the result).

### PG store — `atc-store-pg` (with PG-B fold + invariants.rs + PgMetrics + DbInitError)

`pg.rs` (1,529 LOC today) splits along structural lines already present in the file. **Authoritative line ranges** (verified in the current worktree):

```
backend/crates/atc-store-pg/src/
├── lib.rs              re-exports (PgStore, PgStoreStartError, DbInitError;
│                       PgStoreTestHooks/Handles behind cfg)
├── store/
│   ├── mod.rs          (~550 LOC) Constants (pg.rs:31-72), PgStoreStartError +
│   │                   Display + Error impls (pg.rs:76-130), SqlRepr impls
│   │                   for RunStatus/JobStatus/RunConclusion/JobConclusion
│   │                   (pg.rs:133-211), PgStore struct (pg.rs:217-294), and
│   │                   non-test-only impl PgStore { start, start_inner, ping,
│   │                   private constructor helpers } (the production-only slice
│   │                   of pg.rs:317-571 — production methods only).
│   ├── writes.rs       (~600 LOC) impl PersistentStore for PgStore (pg.rs:572-
│   │                   <impl-end-line>; ~apply_run_event/apply_job_event/
│   │                   read_snapshot/liveness_check/subscribe/shutdown),
│   │                   plus the free-function transaction helpers that
│   │                   sit at module scope after the impl block ends
│   │                   (e.g., upsert_run_in_txn at pg.rs:789, upsert_job_in_txn
│   │                   in the same neighborhood, ~786-1077). The
│   │                   implementation Claude verifies the impl-block end
│   │                   by reading pg.rs and walks the free fns adjacent to it.
│   ├── test_hooks.rs   (~150 LOC) PgStoreTestHooks (pg.rs:296-309) +
│   │                   PgStoreTestHandles (pg.rs:310-315) + the
│   │                   start_with_test_hooks method (pg.rs:354-388 +
│   │                   start_inner-call wiring) + the test-only
│   │                   impl PgStore { outbox_heartbeat_once, replica_id,
│   │                   broadcast_watermark accessor, ... } at pg.rs:1369-1432.
│   │                   Everything in this file is gated behind
│   │                   #[cfg(any(test, feature = "test-support"))].
│   └── retention.rs    (~280 LOC) Free fns spawn_outbox_heartbeat (pg.rs:1113)
│                       + outbox_heartbeat_tick (pg.rs:1169) +
│                       spawn_outbox_sweep (pg.rs:1258) + outbox_sweep_tick
│                       (pg.rs:1298). All four are top-level fns, not impl methods.
├── listener.rs         (580 LOC) move from atc-server/src/listener.rs
├── reads.rs            (313 LOC) move from atc-server/src/persist/reads.rs
├── db.rs               (~30-40 LOC) init_pool — moved from atc-server/src/db.rs (8 LOC)
│                       AND a NEW pub enum DbInitError { Migrate(sqlx::migrate::MigrateError),
│                       Connect(sqlx::Error) } with Display + std::error::Error +
│                       source(). init_pool's signature changes to
│                       Result<PgPool, DbInitError>. This frees atc-server from
│                       pattern-matching on sqlx::Error::Migrate and lets
│                       atc-server drop its direct sqlx dep entirely.
├── metrics.rs          (~287 LOC) PgMetrics struct + register() constructor +
│                       cached OTel instruments + pre-built KeyValue slices,
│                       extracted from atc-server/src/metrics.rs. NOTE:
│                       register_build_info (line 26 in current file) and
│                       spawn_process_collector (line 455) DO NOT move —
│                       they are server-level lifecycle helpers, not store
│                       concerns. They stay in atc-server::metrics.
├── invariants.rs       PG-side outbox/watermark invariant assertions, extracted
│                       from existing test files. Gated behind
│                       #[cfg(any(test, feature = "test-support"))].
├── migrations/         0001_initial_runs_jobs.sql, 0002_outbox.sql,
│                       0003_runs_placeholder.sql, 0004_outbox_watermarks.sql
│                       (four files, all moved from atc-server/migrations/)
└── CLAUDE.md           (+ AGENTS.md symlink) — sharp edges: sqlx, .sqlx/ regen,
                        migrations atomicity rule, test-hooks visibility,
                        DbInitError as the public error type
```

**Implementation note**: the impl-block end line for `impl PersistentStore for PgStore` (which starts at pg.rs:572) is not directly visible in the line-anchored grep output — the implementing context reads pg.rs to locate the closing brace and partition the impl methods from the adjacent free-function transaction helpers (`upsert_run_in_txn`, `upsert_job_in_txn`) that follow it.

Per-crate timeout constants (move these from `atc-server::shutdown` to `atc-store-pg::store::mod`):

- `SHUTDOWN_TIMEOUT_DRAIN: Duration = Duration::from_secs(5);`
- `SHUTDOWN_TIMEOUT_LISTENER: Duration = Duration::from_secs(1);`
- `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT: Duration = Duration::from_secs(2);`
- `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP: Duration = Duration::from_secs(2);`

Values verified at `backend/crates/atc-server/src/shutdown.rs:30-42`. The `PersistentStore::shutdown()` impl on `PgStore` uses these constants when calling `atc_persist::join_with_timeout` on its four `JoinHandle`s.

`Cargo.toml` (atc-store-pg):
```toml
[dependencies]
atc-core = { path = "../atc-core" }
atc-wire = { path = "../atc-wire" }
atc-persist = { path = "../atc-persist" }
atc-github = { path = "../atc-github" }
sqlx = { workspace = true, features = [...] }   # current per-crate pin
opentelemetry = { workspace = true }
tokio = { ... }
async-trait = { workspace = true }
tracing = { ... }

[features]
test-support = []

[dev-dependencies]
atc-store-pg = { path = ".", features = ["test-support"] }
```

`PgMetrics` extraction note — only the struct and its `register()` constructor move. `register_build_info` (one-shot startup gauge with build metadata) and `spawn_process_collector` (wraps `opentelemetry-system-metrics::init_process_observer`) STAY in `atc-server::metrics`. They are server-level lifecycle helpers, not store concerns. Verified anchor: `atc-server/src/metrics.rs:26` and `:455`.

`sqlx::query!` cache safety: I verified that the `.sqlx/` cache files at `backend/.sqlx/` are keyed by SQL string hash + bind types, not by source path or module. Moving the 19 query-macro sites between files (without changing the SQL) does not invalidate the cache. AC: `cargo sqlx prepare --check` passes.

### In-memory store — `atc-store-mem`

```
backend/crates/atc-store-mem/src/
├── lib.rs              InMemoryStore + spawn_eviction (move from atc-server/persist/in_memory.rs)
└── invariants.rs       assert_invariants (gated behind #[cfg(any(test, feature = "test-support"))])
```

Per-crate timeout constant:

- `EVICTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);` (matches `atc-server::shutdown::SHUTDOWN_TIMEOUT_EVICTION` at line 34).

`Cargo.toml` (atc-store-mem):
```toml
[dependencies]
atc-core = { path = "../atc-core" }
atc-wire = { path = "../atc-wire" }
atc-persist = { path = "../atc-persist" }
atc-github = { path = "../atc-github" }   # InMemoryStore constructs WebhookEvent::Run/Job
                                          # to populate CommittedEvent.event before broadcasting
                                          # (in_memory.rs:366-369, 419-422). NOT a transitive dep
                                          # — the construction sites name the type directly.
tokio = { ... }
async-trait = { workspace = true }
tracing = { ... }
# NO sqlx — in-memory store does no DB I/O

[features]
test-support = []

[dev-dependencies]
atc-store-mem = { path = ".", features = ["test-support"] }
```

### `atc-server` after the split

- `atc-server/src/state.rs` — keeps `AppState` (5 fields), drops `SeqEvent` and `StateSnapshot` definitions. Updates `use crate::persist::PersistentStore;` (line 9) → `use atc_persist::PersistentStore;`. Imports `atc_wire::{CommittedEvent, StateSnapshot}` if call sites still reference them in this module (mostly references move to the call sites that need them).
- `atc-server/src/persist/` — directory **deleted** (`mod.rs`, `pg.rs`, `in_memory.rs`, `reads.rs` all moved to their new homes by Phase 3).
- `atc-server/src/listener.rs` — moved into `atc-store-pg/src/listener.rs`.
- `atc-server/src/db.rs` — moved into `atc-store-pg/src/db.rs` AND extended to define `DbInitError`.
- `atc-server/src/migrations/` — moved into `atc-store-pg/migrations/`.
- `atc-server/src/metrics.rs` — keeps `register_build_info` (line 26) + `spawn_process_collector` (line 455) + `ProcessCollectorHandle` (including its `#[cfg(any(test, feature = "test-support"))]` `from_join_handle` constructor at line 436); loses `PgMetrics`. **The `feature = "test-support"` surface persists** through `ProcessCollectorHandle::from_join_handle` — this is why the self-ref dev-dep (see below) is unlikely to be retired.
- `atc-server/src/shutdown.rs` — keeps the orchestration; updates `use crate::persist::PersistentStore;` (line 26) → `use atc_persist::PersistentStore;`. Drops the local `pub fn join_with_timeout` definition (lines 50-68) and switches call sites to `atc_persist::join_with_timeout`. Per-non-store-task timeout constants (`SHUTDOWN_TIMEOUT_WS`, `SHUTDOWN_TIMEOUT_SERVES`, `SHUTDOWN_TIMEOUT_METRICS`) stay here. Per-store-task timeout constants (`SHUTDOWN_TIMEOUT_DRAIN`, `_LISTENER`, `_OUTBOX_HEARTBEAT`, `_OUTBOX_SWEEP`, `_EVICTION`) move to their respective store crates and are not referenced from atc-server::shutdown anymore (the store's `shutdown()` impl owns the join budget).
- `atc-server/src/main.rs` — imports `atc_store_pg::{PgStore, DbInitError}` and `atc_store_mem::InMemoryStore` at the dispatch site (around line 132 where the `pg_pool.is_some()` branch lives). The migration-vs-connect discriminator at line 158 changes from `matches!(e, sqlx::Error::Migrate(_))` to `matches!(e, DbInitError::Migrate(_))`. atc-server now has no `sqlx::*` references in source.
- `atc-server/Cargo.toml` — gains production deps on `atc-wire`, `atc-persist`, `atc-store-pg`, `atc-store-mem`. **Moves `sqlx` from `[dependencies]` to `[dev-dependencies]`** — production source has no `sqlx::*` references after the DbInitError abstraction, but integration tests under `tests/integration/` use sqlx 132+ times for ephemeral-DB CREATE/DROP and query helpers. Per the user-confirmed B-Lite test reorg, integration tests live with atc-server, so sqlx must remain a dev-dep. The `atc-server = { path = ".", features = ["test-support"] }` self-ref dev-dep is **retired in Phase 3** — `from_join_handle` (the only `feature = "test-support"`-gated symbol) is consumed by unit tests inside the same crate, so `cfg(test)` alone suffices and the self-ref becomes vestigial.

### Ordering constraints (carry-over from Scotty C2 § 7)

These are correctness gates, not stylistic preferences. Each step's PR must atomically satisfy these:

| # | Constraint | Phase | Failure mode if violated |
|---|---|---|---|
| OC-1 | `LivenessError` opaque-box change lands WITH `atc-persist` creation. | 1 | `atc-store-mem` gets transitive sqlx dep; breaking API change later. |
| OC-2 | Wire types land WITH `atc-persist` (here: in same PR as `atc-wire`). | 1 | Trait references undefined types; compile error. |
| OC-3 | `atc-persist` exists on `main` before any store crate PR merges. | 1 → 2/3 | Store crates implement a non-existent trait. |
| OC-4 | Integration test import fixups in SAME PR as store crate creation. | 1, 2, 3 | Test binary fails to compile; CI red. |
| OC-5 | `atc-persist::join_with_timeout` exists before any store crate's `shutdown()` impl is moved. | 1 → 2/3 | Cycle dep or compile error. |
| OC-6 | `PgMetrics` extraction in Phase 2 PR — **mandatory**. | 2 | Cycle `atc-store-pg → atc-server → atc-store-pg` because `PgStore` would need to call back into atc-server for metric instruments. |
| OC-7 | `ws.rs` `CommittedEvent` import updated in Phase 1 — **production code**. | 1 | Production binary fails to compile. |
| OC-8 | `sqlx::migrate!` path + `migrations/` move atomically. | 2 | Runtime panic (no migrations) or compile error. |
| OC-9 | `atc-server` production dep on new store crates lands in same PR as the store crate creation. | 2, 3 | `main.rs` loses access to PgStore/InMemoryStore. |
| OC-10 | New crates added to `.release-please-manifest.json` at `"0.1.0"` not `"0.0.0"`. | 0 | release-please's 0.0.0 bootstrap bug (see memory). |

## Implementation Phases

**Note on TDD ordering**: this is a **behavior-preserving refactor** (per `docs/implementation-guidance.md` rule 2's second paragraph: "For refactors that preserve behavior, the regression net must stay green throughout. Extending or restructuring tests is fine, but the 'red phase' of TDD does not apply"). The existing test suite (~35 integration tests + per-crate units) IS the regression net. The exceptions where new behavior lands and TDD does apply:

- **Phase 1**: `LivenessError`'s new opaque-box variant + `impl Error::source()` is a small behavior change. Add a unit test in `atc-persist` that wraps a synthetic error in `LivenessError::DbUnreachable(Box::new(io::Error::other("test")))` and asserts `.source().is_some()`. Land the test alongside the impl.
- **Phase 2**: `db::init_pool`'s return type changes from `Result<PgPool, sqlx::Error>` → `Result<PgPool, DbInitError>`. Add a unit test that exercises both `DbInitError::Migrate` and `DbInitError::Connect` discriminator branches.
- **All other code movement** is verified by the existing regression net — no new red-phase tests required.

The phases below are **migration checkpoints**, not red/green TDD steps.

### Phase 0 — Pre-flight (config + rule alignment)

**Branch creation + plan checkpoint (do this BEFORE any other work):**

1. Create branch: `git switch -c feat/persistence-crate-split` from `main`.
2. Copy this plan: `cp ~/.claude/plans/splendid-coalescing-pony.md docs/design-plans/2026-05-15-persistence-crate-split.md`.
3. Commit the plan only: `chore(persist): add design plan for persistence crate split (#169)`.

**Phase 0 PR contents:**

4. Update `release-please-config.json`:
   - Add four new entries to the `packages` **object** (release-please's `packages` is a map keyed by path, not an array — verify the existing config's shape before editing): `backend/crates/atc-wire`, `backend/crates/atc-persist`, `backend/crates/atc-store-pg`, `backend/crates/atc-store-mem`. Each entry mirrors the per-crate config of the existing `backend/crates/atc-core` (release-as type, changelog sections, etc.).
   - Add the same four paths to the existing `linked-versions` group's `components` **array** (the field is `components`, not `linked-versions`-array-of-paths) — this group already contains the existing 4 components (atc-core, atc-github, atc-server, frontend).
5. Update `.release-please-manifest.json`: add four new entries at `"0.1.0"` (NOT `"0.0.0"` — bootstrap bug; see memory `release_please_version_quirks.md`).
6. Update `scripts/doc-mapping.sh` with case-ordered patterns for the new crates. The current script has **no workspace-level `backend/crates/*` catch-all** (verify: read `scripts/doc-mapping.sh` to confirm — as of plan-write time, the most-general entry is `backend/crates/atc-server/src/*`). The ordering constraint is therefore that the new specific straddler entries `atc-store-pg/src/metrics.rs|atc-store-pg/src/listener.rs` MUST precede the new `atc-store-pg/src/*` catch-all (Tuvok C3 § 7 Rule 4 — `case` is first-match). The proposed insertion order:

   ```bash
   backend/crates/atc-store-pg/src/metrics.rs|backend/crates/atc-store-pg/src/listener.rs)
       echo "docs/architecture/backend-server.md"
       echo "docs/architecture/metrics.md"
       return
       ;;
   backend/crates/atc-store-pg/src/*|backend/crates/atc-store-pg/migrations/*)
       echo "docs/architecture/backend-server.md"
       return
       ;;
   backend/crates/atc-store-mem/src/*|backend/crates/atc-wire/src/*|backend/crates/atc-persist/src/*)
       echo "docs/architecture/backend-server.md"
       return
       ;;
   ```

7. Resolve CLAUDE.md ↔ CONTRIBUTING.md contradiction (Option C two-tier). Both edits in same commit:
   - Edit root `CLAUDE.md` "Slim CLAUDE.md in every domain directory" invariant — replace mandatory framing with two-tier:

     > **Slim CLAUDE.md in every domain directory:** Every subdirectory that represents a distinct domain (crates, frontend, helm chart, .github, etc.) must have a slim `CLAUDE.md` providing the **mandatory skeleton**: purpose, pointer to canonical architecture doc, AGENTS.md symlink. **Sharp-edges sections** (testing gotchas, common foot-guns, file-specific guidance) are added **reactively** when agents encounter friction in that directory. Do not pre-author sharp edges speculatively. Every CLAUDE.md needs an AGENTS.md symlink (`ln -s CLAUDE.md AGENTS.md`).

   - Edit `CONTRIBUTING.md` "Directory-Level CLAUDE.md Files" section — align to the same two-tier framing. Drop the "high-risk only" wording. Reference root CLAUDE.md as the source of truth for the skeleton requirement.
8. ADR-0005 + ADR-0006 annotation sweep AND broader superseded-reference sweep across the repo:
   - Annotate `docs/architecture-decisions/0005-persistentstore-trait-relocation.md` with:

     ```markdown
     > **Revised by ADR-0008:** The `PersistentStore` trait moves out of `atc-server::persist` into a dedicated `atc-persist` crate, alongside `atc-wire` (data types) and the per-implementation `atc-store-pg` / `atc-store-mem` crates. The geographic claim ("trait lives in `atc-server::persist`") is superseded; the architectural reasoning (trait owned by the layer that wires it) is preserved by the four-crate split. See `docs/architecture-decisions/0008-persistence-crate-split.md`.
     ```

   - Annotate `docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md` if it makes geographic claims about `atc-server::persist::*`. Verify with `cd /Users/brajkovic/Projects/atc && git grep "atc-server::persist\|atc_server::persist" docs/architecture-decisions/0006-*`. Add an annotation per the same pattern noting the post-split locations.
   - **Broader sweep** (Codex important concern, per `docs/implementation-guidance.md` rule 6): every doc, code comment, and test reference to the soon-to-be-moved module paths needs review. Specifically:
     - `cd /Users/brajkovic/Projects/atc && git grep "atc-server::persist\|atc_server::persist" docs/ backend/ frontend/` — every hit outside `docs/architecture-decisions/0008-*` and `docs/design-plans/2026-05-15-*` needs evaluation. Stale references in unchanged source code annotate; references in docs that are NOT being rewritten in this PR series get a brief `> See [ADR-0008]` callout.
     - `cd /Users/brajkovic/Projects/atc && git grep "atc_server::state::SeqEvent\|atc_server::state::StateSnapshot"` — same rule.
   - Phase 0 lands the annotation skeletons (header callouts on ADR-0005/0006). Phases 1/2/3 update individual doc surfaces as they touch them. Phase 3's PR body confirms the broader sweep is complete and documents any deliberate exceptions.

9. Update `docs/architecture/release-pipeline.md` (if it exists) to enumerate the four new packages.
10. Run `cd /Users/brajkovic/Projects/atc && bash scripts/doc-mapping.sh backend/crates/atc-store-pg/src/listener.rs` — must print BOTH `docs/architecture/backend-server.md` AND `docs/architecture/metrics.md` (verifies straddler ordering).

**Estimated diff:** ~80-100 lines, ~6-8 files, **zero source changes**.

**PR title:** `chore(persist): pre-flight for persistence crate split (#169)`

### Phase 1 — `atc-wire` + `atc-persist` crates + LivenessError fix + CommittedEvent rename

This phase has the most atomic constraints because the rename, the trait move, the wire-type move, the workspace `async-trait` promotion, and the frontend regeneration must all land together — partial application breaks compile.

11. **Promote `async-trait` AND `tracing` to `[workspace.dependencies]`** in `backend/Cargo.toml`:
    ```toml
    [workspace.dependencies]
    ts-rs = { version = "=12.0.1", features = ["serde-compat", "chrono-impl"] }
    async-trait = "=0.1.89"
    tracing = "0.1"      # version per the existing per-crate pin in atc-server/Cargo.toml; verify exact value at edit time
    ```
    Update `backend/crates/atc-server/Cargo.toml` line 16 to `async-trait = { workspace = true }`. Update the existing per-crate `tracing` entry to `tracing = { workspace = true }`. Other new crates (atc-persist, atc-store-pg, atc-store-mem) reference both as `{ workspace = true }`. **Why `tracing` too**: atc-persist's `join_with_timeout` uses `tracing::warn!`/`error!`; atc-store-pg + atc-store-mem use `tracing::info_span!` / `#[tracing::instrument]` extensively. Three new consumers + per-crate version pinning would diverge without the workspace dep.

12. **Create `atc-wire` crate**:
    - `backend/crates/atc-wire/Cargo.toml` — deps: atc-core, atc-github, serde, ts-rs (workspace). `publish = false`.
    - `backend/crates/atc-wire/src/lib.rs` — `CommittedEvent` (with `#[derive(TS)]` + `#[ts(export)]`), `StateSnapshot` (same).
    - `backend/crates/atc-wire/CLAUDE.md` (skeleton tier: purpose, pointer to `docs/architecture/backend-server.md` § "SeqEvent Wire Contract" — to be renamed in same edit pass).
    - `backend/crates/atc-wire/AGENTS.md` symlink: `ln -s CLAUDE.md AGENTS.md`.
    - Add to `backend/Cargo.toml` `members`.

13. **Create `atc-persist` crate**:
    - `backend/crates/atc-persist/Cargo.toml` — deps: atc-core, atc-wire, async-trait (workspace), tokio (constrained surface: `["sync", "time", "rt"]`), **tracing (workspace — required by `join_with_timeout`'s `warn!`/`error!` calls)**. Disallowed direct deps: sqlx, atc-github, serde, ts-rs. `publish = false`.
    - `backend/crates/atc-persist/src/lib.rs` — `PersistentStore` trait, `LivenessError` (opaque-box + `impl Error::source()`), `pub use atc_core::PersistError;`.
    - `backend/crates/atc-persist/src/join.rs` — lift `join_with_timeout` from `atc-server/src/shutdown.rs:50-68` verbatim (both the `JoinError::is_cancelled()` branch and the `tracing::warn!`/`tracing::error!` log lines).
    - `backend/crates/atc-persist/CLAUDE.md` (skeleton tier: purpose + sqlx-free invariant as a sharp edge):

      > **`atc-persist` rule:** `[dependencies]` must NOT include sqlx, redis, mongodb, or any storage backend library. The trait crate names interfaces; concrete backends live in `atc-store-*` crates.

    - `backend/crates/atc-persist/AGENTS.md` symlink.
    - Add to `backend/Cargo.toml` `members`.

14. **Move `SeqEvent` (rename to `CommittedEvent`) and `StateSnapshot` from `atc-server::state` into `atc-wire`**. Delete the type defs from `atc-server/src/state.rs`. **Do NOT add a `pub use atc_wire::{CommittedEvent, StateSnapshot};` re-export from `atc-server::state`** — the cleaner end-state is that every call site imports directly from `atc_wire` (this also satisfies AC12 which catches `atc_server::state::CommittedEvent` references via grep). After the move, `atc-server::state` carries `AppState` only.

15. **Move `PersistentStore`, `LivenessError`, and `PersistError` re-export from `atc-server::persist::mod` into `atc-persist::lib`**. Apply the opaque-box change to `LivenessError::DbUnreachable`. Add `impl std::error::Error::source()`. Delete `backend/crates/atc-server/src/persist/mod.rs` if its only contents were these moves; or trim to whatever remains (tests, helper functions). Verify by grep — see AC list.

16. **Update production code imports** in `atc-server` (every site that names the moved types):
    - `src/state.rs:9` — `use crate::persist::PersistentStore;` → `use atc_persist::PersistentStore;`. Drop the `SeqEvent`/`StateSnapshot` definitions (moved to atc-wire).
    - `src/shutdown.rs:26` — `use crate::persist::PersistentStore;` → `use atc_persist::PersistentStore;`. Drop the local `pub fn join_with_timeout` (lines 50-68) and switch call sites to `atc_persist::join_with_timeout`. Add `atc-persist` to atc-server's deps.
    - `src/ws.rs` — `atc_server::state::SeqEvent` → `atc_wire::CommittedEvent` (OC-7 — production code).
    - `src/routes.rs` — imports for `StateSnapshot` and `CommittedEvent`.
    - `src/main.rs` — imports for `PersistentStore`, `CommittedEvent`. (PgStore/InMemoryStore imports update in Phase 2/3.)
    - `src/persist/pg.rs` (still in atc-server in this phase) — imports for trait, `LivenessError`, `CommittedEvent`, `StateSnapshot`. Update every `LivenessError::DbUnreachable(e)` construction site to wrap the inner error as `Box::new(e)`.
    - `src/persist/in_memory.rs` (still in atc-server) — imports for trait, `CommittedEvent`, `StateSnapshot`.
    - `src/persist/mod.rs` — trim to keep only `pub use in_memory::InMemoryStore; pub use pg::PgStore;` re-exports + `pub use atc_persist::{PersistentStore, PersistError, LivenessError};` for back-compat with any in-tree call sites that still reference the local namespace until Phase 2/3.
    - `src/listener.rs` (still in atc-server) — imports for `CommittedEvent`.

17. **Update test imports** — `tests/integration/common/mod.rs` lines 10-13 + everywhere downstream:
    - `use atc_server::otel::exponential_histogram_view;` — unchanged (otel stays in atc-server).
    - `use atc_server::persist::pg::PgStoreTestHooks;` — unchanged in this phase (PgStore moves in Phase 2).
    - `use atc_server::persist::{InMemoryStore, PersistentStore, PgStore};` → keep until Phases 2/3, but add `use atc_persist::PersistentStore;` and `use atc_wire::{CommittedEvent, StateSnapshot};` if those are referenced separately.

18. **Broad rename** (mechanical) — every `SeqEvent` reference in the repo, not just the generated types:
    - Run `just types`. The generated `frontend/src/lib/types/generated/SeqEvent.ts` is removed and `CommittedEvent.ts` is created.
    - Global identifier rename:
      - `SeqEvent → CommittedEvent` (type, all references)
      - `seqEvent → committedEvent` (local variable instances)
      - `runSeqEvent → runCommittedEvent`, `jobSeqEvent → jobCommittedEvent` (test variable instances)
      - `makeRunSeqEvent → makeRunCommittedEvent`, `makeUnknownSeqEvent → makeUnknownCommittedEvent`, `makeJobSeqEvent → makeJobCommittedEvent` (helpers, including the EXPORTED `makeJobSeqEvent` in `frontend/e2e/lib/ws-mock.ts` which is the harness's public surface)
      - `$lib/types/generated/SeqEvent` → `$lib/types/generated/CommittedEvent` (import paths)
    - **Frontend code files** (verified by `grep -rl SeqEvent frontend/src/`):
      - production: `frontend/src/lib/connection.ts`, `frontend/src/lib/dispatcher.ts`, `frontend/src/lib/aria/transition-kinds.ts`, `frontend/src/lib/aria/live-region.svelte.ts`
      - tests: `frontend/src/lib/dispatcher.test.ts`, `frontend/src/lib/dispatcher.browser.test.ts`, `frontend/src/lib/dispatcher.perf.browser.test.ts`, `frontend/src/lib/connection.aria-silence.test.ts`, `frontend/src/lib/connection.buffering.test.ts`, `frontend/src/lib/aria/live-region.test.ts`, `frontend/src/lib/aria/transition-kinds.test.ts`
    - **Frontend e2e harness + e2e tests** (Codex blocker 1 widened by pass-2): `frontend/e2e/lib/ws-mock.ts` — 3 references including the exported helper `makeJobSeqEvent`. Renames the harness's public API. Plus 3 e2e tests that import the wire type via the harness: `frontend/e2e/pool-filter.test.ts`, `frontend/e2e/pool-indicators.test.ts`, `frontend/e2e/run-detail-panel.test.ts` — update import sites + helper call names accordingly.
    - **Architecture docs** (Codex blocker 1): `docs/architecture/frontend-app.md` — 6 references at lines 524, 549, 576, 584, 729, 752 (rename type references AND the `makeJobSeqEvent` helper-name reference at line 752). `docs/architecture/deployment.md` — 3 references at lines 411, 419, 433 (operator runbook commands and acceptance text).
    - **Domain `CLAUDE.md` files**: `frontend/CLAUDE.md` — 2 references (lines 22, 51 — both reference the e2e helper name + a typed-union convention example). `backend/crates/atc-server/CLAUDE.md` — every `SeqEvent` reference.
    - **`scripts/doc-mapping.sh`**: line 62 maps `frontend/src/*` → `docs/architecture/frontend-app.md`. The doc-staleness gate will require `frontend-app.md` updated when frontend files change in this PR — the rename above handles it.

19. **Author ADR-0008** at `docs/architecture-decisions/0008-persistence-crate-split.md`:
    - Status: Accepted (issue #169, 2026-05-15).
    - Context: summarize the cycle constraints (`SeqEvent` above store crates), the wire-type-in-trait-crate concern, and the four-crate convergence.
    - Decision: four crates as listed.
    - Consequences: `atc-server` no longer compiles unused store; trait crate is sqlx-free; per-crate `CLAUDE.md` files appear for each new crate.
    - Records the bus-inversion + tuple-decomposition rejections (link back to this design plan).
    - Notes the `SeqEvent → CommittedEvent` rename.

20. **Update docs** (Phase 1 scope only — Phase 2/3 update further):
    - `docs/architecture/backend-server.md` — rename every `SeqEvent` to `CommittedEvent`. Update `atc_server::state::{SeqEvent, StateSnapshot}` references to `atc_wire::{CommittedEvent, StateSnapshot}`. Update the `PersistentStore` trait location reference from `atc-server::persist::mod` to `atc-persist`.
    - `docs/architecture/frontend-app.md` — already covered in step 18 (6 references).
    - `docs/architecture/deployment.md` — already covered in step 18 (3 references).
    - `docs/architecture/metrics.md` — no changes in Phase 1 (metrics layout unchanged until PgMetrics extracts in Phase 2).
    - `backend/crates/atc-server/CLAUDE.md` — update the persist-module description (the trait + LivenessError have moved out); add pointers to `atc-persist`, `atc-wire`. Replace every `SeqEvent` reference. **Use the two-tier framing from Phase 0**: the "Modules" table entry for `state` shrinks (no longer holds wire types); add `wire types` row pointing to `atc-wire`.
    - `frontend/CLAUDE.md` — already covered in step 18 (2 references).

21. **Update `scripts/doc-mapping.sh`** if Phase 0 didn't already include the new crate paths.

**Estimated diff:** ~400-450 lines, ~25-30 files (incl. 11 frontend files). The frontend rename dominates the file count.

**PR title:** `feat(persist): extract atc-wire and atc-persist crates, rename SeqEvent → CommittedEvent (#169)`

### Phase 2 — `atc-store-pg` crate (with PG-B fold + invariants.rs + PgMetrics extraction)

This is the largest phase. Step 2's goal is a single irreducible PR — the move-and-restructure of all PG-mode code.

22. **Create `atc-store-pg` crate skeleton** (file map matches the corrected Architecture § PG store):
    - `backend/crates/atc-store-pg/Cargo.toml` — deps as described in Architecture above. `publish = false`.
    - `backend/crates/atc-store-pg/src/lib.rs` — re-exports (PgStore, PgStoreStartError, DbInitError; PgStoreTestHooks/Handles behind cfg).
    - `backend/crates/atc-store-pg/src/store/mod.rs` (~550 LOC) — extract: 7 constants from `pg.rs:31-72`; `PgStoreStartError` + Display + Error impls from `pg.rs:76-130`; SqlRepr impls for RunStatus/JobStatus/RunConclusion/JobConclusion from `pg.rs:133-211`; `PgStore` struct from `pg.rs:217-294`; the **production-only** slice of the `impl PgStore` block at `pg.rs:317-571` covering `start`, `start_inner`, `ping`, and the private constructor helpers (the test-only methods at `pg.rs:1369-1432` and the test-only `start_with_test_hooks` move to `test_hooks.rs`). Plus the 4 PG timeout constants moved from `atc-server::shutdown`: `SHUTDOWN_TIMEOUT_DRAIN`, `SHUTDOWN_TIMEOUT_LISTENER`, `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT`, `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP`.
    - `backend/crates/atc-store-pg/src/store/writes.rs` (~600 LOC) — extract: `impl PersistentStore for PgStore` starting at `pg.rs:572` (the implementing context locates the closing brace by reading the file), PLUS the free-function transaction helpers that sit at module scope between the impl block and the heartbeat spawn fn (e.g., `upsert_run_in_txn` at `pg.rs:789`, `upsert_job_in_txn` adjacent — full neighborhood `pg.rs:786-1077` based on pass-2 codex's identification). The `shutdown()` impl uses `atc_persist::join_with_timeout` for all four PG task handles (drain, listener, heartbeat, sweep).
    - `backend/crates/atc-store-pg/src/store/test_hooks.rs` (~150 LOC) — extract: `PgStoreTestHooks` from `pg.rs:296-309`; `PgStoreTestHandles` from `pg.rs:310-315`; the `start_with_test_hooks` method (anchor at `pg.rs:354-388` which calls `start_inner` from store/mod.rs); the test-only `impl PgStore { outbox_heartbeat_once, replica_id, broadcast_watermark accessor, ... }` from `pg.rs:1369-1432`. Everything in this file gated `#[cfg(any(test, feature = "test-support"))]`.
    - `backend/crates/atc-store-pg/src/store/retention.rs` (~280 LOC) — extract free fns: `spawn_outbox_heartbeat` (`pg.rs:1113`), `outbox_heartbeat_tick` (`pg.rs:1169`), `spawn_outbox_sweep` (`pg.rs:1258`), `outbox_sweep_tick` (`pg.rs:1298`). All four are top-level fns, not impl methods.
    - `backend/crates/atc-store-pg/src/listener.rs` — move from `atc-server/src/listener.rs` (580 LOC). Update imports: `atc_server::state::SeqEvent` → `atc_wire::CommittedEvent`. Update `LivenessError::DbUnreachable` constructor sites to use `Box::new(e)`.
    - `backend/crates/atc-store-pg/src/reads.rs` — move from `atc-server/src/persist/reads.rs` (313 LOC). `pub(crate)` visibility preserved.
    - `backend/crates/atc-store-pg/src/db.rs` — move from `atc-server/src/db.rs` (8 LOC) AND extend with `pub enum DbInitError { Migrate(Box<sqlx::migrate::MigrateError>), Connect(sqlx::Error) }` + `impl Display + std::error::Error::source()`. **Note**: `sqlx 0.8.6` defines `Error::Migrate(Box<MigrateError>)` (not bare `MigrateError`) — the wrapper Box is required to match sqlx's actual variant. Change `init_pool` signature to `pub async fn init_pool(url: &str) -> Result<PgPool, DbInitError>` and translate `sqlx::Error::Migrate(boxed) → DbInitError::Migrate(boxed)` / other → `DbInitError::Connect(e)`. The new `sqlx::migrate!()` call still anchors against the moved `migrations/` (now at `atc-store-pg/migrations/`); per OC-8 these move atomically.
    - `backend/crates/atc-store-pg/src/metrics.rs` — extract `PgMetrics` struct and its `register()` constructor from `atc-server/src/metrics.rs` (the ~287 LOC of cached instruments + KeyValue slices). Register-time visibility is `pub`. `register_build_info` and `spawn_process_collector` STAY in `atc-server::metrics`.
    - `backend/crates/atc-store-pg/src/invariants.rs` — extract any PG-side outbox/watermark invariant assertions currently sitting in tests or test-only modules; re-anchor under `#[cfg(any(test, feature = "test-support"))]`. (Concrete content TBD by implementor — find existing assertions via grep.)
    - `backend/crates/atc-store-pg/migrations/` — move all four SQL files from `atc-server/migrations/`.
    - `backend/crates/atc-store-pg/CLAUDE.md` — two-tier: skeleton (purpose, pointer to backend-server.md + metrics.md) + sharp edges (sqlx, .sqlx/ regen, migrations atomicity rule, test-hooks visibility convention).
    - `backend/crates/atc-store-pg/AGENTS.md` symlink.
    - Add to `backend/Cargo.toml` `members`.

23. **Update `atc-server`** (every PG-mode call site, named explicitly with real signatures):
    - Delete `atc-server/src/persist/{pg.rs, reads.rs}`, `atc-server/src/listener.rs`, `atc-server/src/db.rs`.
    - Delete `atc-server/migrations/` directory.
    - Update `atc-server/Cargo.toml`: add `atc-store-pg = { path = "../atc-store-pg" }` to `[dependencies]`. Add cross-crate dev-dep: `atc-store-pg = { path = "../atc-store-pg", features = ["test-support"] }` in `[dev-dependencies]`. **Move `sqlx` from `[dependencies]` to `[dev-dependencies]`** — atc-server's production source has no `sqlx::*` use sites after the DbInitError abstraction, but its integration tests under `tests/integration/` use sqlx 132+ times for ephemeral-DB CREATE/DROP and query helpers. AC25 reflects this: no sqlx in `[dependencies]`; sqlx in `[dev-dependencies]` is acceptable.
    - Update `atc-server/src/main.rs` (every site that names a moved type — verified by grep):
      - Line 13 ish — change `use crate::{db, listener, ...};` to `use atc_store_pg::{db, listener, DbInitError, PgStore};` (drop the `crate::db, crate::listener` imports; atc-server no longer owns those modules).
      - Line 158 — `db::init_pool(db_url).await.unwrap_or_else(|e| { if matches!(e, sqlx::Error::Migrate(_)) { ... } })` becomes `db::init_pool(db_url).await.unwrap_or_else(|e| { if matches!(e, DbInitError::Migrate(_)) { ... } })`. Source unchanged structurally; only the namespace and the inner enum match change.
      - Line 172 — `listener::connect_listener(&listener_url)` continues to work (now `atc_store_pg::listener::connect_listener` via the import update at line 13).
      - Line 179 — `PgStore::start(...)` call site — keep the **real signature** unchanged: `PgStore::start(clock, pool, listener_conn, shutdown, retention).await`. Only the namespace changes (now from `atc_store_pg::PgStore`); the parameter list and order are not modified by this refactor.
    - Update `atc-server/src/metrics.rs`: drop `PgMetrics` struct + emitter (now in atc-store-pg). Keep `register_build_info` (line 26), `spawn_process_collector` (line 455), and `ProcessCollectorHandle::from_join_handle` (line 436). **Change line 436's gate** from `#[cfg(any(test, feature = "test-support"))]` to `#[cfg(test)]` — the only consumers of `from_join_handle` are unit tests inside `shutdown.rs::tests` (lines 316, 362), which see `cfg(test)` automatically; no integration test consumes it (verified: `cd /Users/brajkovic/Projects/atc && grep -rn "from_join_handle" backend/crates/atc-server/tests/` returns zero hits). This narrowing prepares the self-ref dev-dep retirement in Phase 3.
    - Update `atc-server/src/shutdown.rs`: drop `SHUTDOWN_TIMEOUT_DRAIN` (line 30), `SHUTDOWN_TIMEOUT_LISTENER` (line 33), `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT` (line 38), `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP` (line 42) — these become per-store-crate constants. Keep `SHUTDOWN_TIMEOUT_WS` (line 31), `SHUTDOWN_TIMEOUT_SERVES` (line 32), `SHUTDOWN_TIMEOUT_METRICS` (line 35). The shutdown orchestration's `state.persist.shutdown()` call delegates per-store join budget into the store crate.
    - **Integration test compile anchors** (Codex pass-2 important concern 6 — name every one):
      - `tests/integration/common/mod.rs:11` — `use atc_server::persist::pg::PgStoreTestHooks;` → `use atc_store_pg::PgStoreTestHooks;`
      - `tests/integration/common/mod.rs:12` — `use atc_server::persist::{InMemoryStore, PersistentStore, PgStore};` → `use atc_persist::PersistentStore; use atc_store_pg::PgStore; use atc_server::persist::InMemoryStore;` (InMemoryStore still in atc-server::persist::in_memory until Phase 3).
      - `tests/integration/common/mod.rs:422` ish (codex pass-2 anchor) — `sqlx::*` use sites for ephemeral DB setup. These continue to work because sqlx is now in atc-server's `[dev-dependencies]`.
      - `tests/integration/common/mod.rs:511` — `atc_server::db::init_pool(&db_url)` → `atc_store_pg::db::init_pool(&db_url)`. Match return-type change to `Result<PgPool, DbInitError>` (translate to `.expect("init_pool failed")` or surface the error).
      - `tests/integration/common/mod.rs:714` ish — `connect_listener` call site (codex pass-2). Update import path to `atc_store_pg::listener::connect_listener`.
      - `tests/integration/db_readyz_tests.rs:110` — anchor on `atc_server::db::init_pool` or `sqlx::migrate!()`. Update import to `atc_store_pg::db`.
      - `tests/integration/notify_listener_tests.rs` — anchors on `NOTIFY_CHANNEL` constant + `atc_server::listener` paths. Update to `atc_store_pg::listener::*`.
      - All other integration test files referencing `atc_server::persist::pg::*`, `atc_server::listener::*`, `atc_server::db::*`, or `atc_server::metrics::PgMetrics` — update imports. The implementing context resolves the full list via `cd /Users/brajkovic/Projects/atc && git grep -l "atc_server::persist::pg\|atc_server::listener\|atc_server::db\|atc_server::metrics::PgMetrics" backend/crates/atc-server/tests/`.

24. **Verify sqlx cache safety**: run `cargo sqlx prepare --check` from `backend/`. Must pass without regenerating `.sqlx/`. If it fails, the macro invocations must be inspected — but moving across files (without changing SQL strings or bind types) should not affect the cache hash.

25. **Update docs**:
    - `docs/architecture/backend-server.md` — update PG-mode section: file references move from `atc-server/src/persist/pg.rs` → `atc-store-pg/src/store/{mod, writes, retention}.rs`; from `atc-server/src/listener.rs` → `atc-store-pg/src/listener.rs`; etc.
    - `docs/architecture/metrics.md` — update `PgMetrics` location reference. The struct and its emit sites are now in `atc-store-pg/src/metrics.rs`. `register_build_info` reference unchanged.
    - `backend/crates/atc-server/CLAUDE.md` — update the persist-module description to reflect that pg-mode code now lives in `atc-store-pg`.

26. **Run `cd /Users/brajkovic/Projects/atc && bash scripts/doc-mapping.sh backend/crates/atc-store-pg/src/store/writes.rs`** — must print `docs/architecture/backend-server.md`.

**Estimated diff:** ~3,800-4,000 lines, ~30-40 files. Largest single PR in the series.

**PR title:** `feat(persist): extract atc-store-pg crate (#169)`

### Phase 3 — `atc-store-mem` crate

27. **Create `atc-store-mem` crate skeleton**:
    - `backend/crates/atc-store-mem/Cargo.toml` — deps: atc-core, atc-wire, atc-persist, **atc-github (required: in_memory.rs:17 imports `WebhookEvent` and lines 366-369, 419-422 construct `WebhookEvent::Run(env)` / `WebhookEvent::Job(env)` to populate `CommittedEvent.event` before broadcasting)**, tokio, async-trait (workspace), tracing. NO sqlx (in-memory store does no DB I/O). `publish = false`.
    - `backend/crates/atc-store-mem/src/lib.rs` — move `InMemoryStore` from `atc-server/src/persist/in_memory.rs` (548 LOC) verbatim. Includes the private `spawn_eviction` associated function (per #163 — already inside `InMemoryStore`).
    - `backend/crates/atc-store-mem/src/invariants.rs` — extract `assert_invariants` + any test-support inspection helpers (currently in `in_memory.rs` behind `#[cfg(any(test, feature = "test-support"))]`).
    - `EVICTION_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);` const at top of `lib.rs` (matching `atc-server::shutdown::SHUTDOWN_TIMEOUT_EVICTION`).
    - `InMemoryStore::shutdown()` impl uses `atc_persist::join_with_timeout(eviction_handle, EVICTION_SHUTDOWN_TIMEOUT, "eviction")`.
    - `backend/crates/atc-store-mem/CLAUDE.md` (skeleton tier: purpose + pointer to backend-server.md). Sharp edges added reactively.
    - `backend/crates/atc-store-mem/AGENTS.md` symlink.
    - Add to `backend/Cargo.toml` `members`.

28. **Update `atc-server`**:
    - Delete `atc-server/src/persist/in_memory.rs` and any other persist-module leftovers (the directory itself may now be empty — delete `persist/` if so).
    - Update `atc-server/Cargo.toml`: add `atc-store-mem = { path = "../atc-store-mem" }`. Add cross-crate dev-dep with `features = ["test-support"]`.
    - Update `atc-server/src/main.rs`: in-memory dispatch branch calls `atc_store_mem::InMemoryStore::start(...)`.
    - Update `atc-server/src/shutdown.rs`: drop `SHUTDOWN_TIMEOUT_EVICTION` constant — moved to atc-store-mem.
    - Update integration test imports:
      - `tests/integration/common/mod.rs:12` — drop `atc_server::persist::InMemoryStore`; add `use atc_store_mem::InMemoryStore;`.
      - Other test files referencing `atc_server::persist::in_memory::*` — update.

29. **Retire the `atc-server` self-ref dev-dep** (Phase 2 narrowed `metrics.rs:436` to `cfg(test)`; see Phase 2 step 23. Pass-2 codex confirmed retention is unsupported by live code — `from_join_handle` is consumed only by unit tests inside the same crate, not integration tests):
    - Verify: `cd /Users/brajkovic/Projects/atc && git grep "feature.*test-support" backend/crates/atc-server/src/` returns zero hits AND `cd /Users/brajkovic/Projects/atc && git grep "from_join_handle" backend/crates/atc-server/tests/` returns zero hits.
    - Remove from `backend/crates/atc-server/Cargo.toml`:
      ```toml
      [features]
      test-support = []

      [dev-dependencies]
      atc-server = { path = ".", features = ["test-support"] }
      ```
    - Document in the PR body: "self-ref dev-dep retired; `from_join_handle` narrowed to `#[cfg(test)]` since its only consumers are unit tests inside the same crate. AC36 verifies."

30. **Update docs**:
    - `docs/architecture/backend-server.md` — in-memory mode section: file references move to `atc-store-mem/src/lib.rs` + `invariants.rs`.
    - `backend/crates/atc-server/CLAUDE.md` — drop the in-memory module description; reference the `atc-store-mem` crate.

**Estimated diff:** ~700-800 lines, ~12-15 files.

**PR title:** `feat(persist): extract atc-store-mem crate, retire atc-server self-ref dev-dep (#169)`

## Acceptance Criteria

Numbered for explicit checkoff. Each AC has either a unique automated check or a clearly-described manual verification step. Failure ACs (`AC-fail-N`) describe the exact wrong behavior.

### Phase 0 (pre-flight PR)

- **AC1**: `release-please-config.json`'s `packages` **object** (keyed by path) contains entries for all four new crate paths: `backend/crates/atc-wire`, `backend/crates/atc-persist`, `backend/crates/atc-store-pg`, `backend/crates/atc-store-mem`. The existing `linked-versions` "atc" group's `components` **array** lists all 8 components (4 new + atc-core, atc-github, atc-server, frontend).
- **AC2**: `.release-please-manifest.json` has 9 entries total (8 listed in the linked-versions group + helm, which exists separately at `deploy/helm/atc`). The four NEW entries are exactly `"0.1.0"` (`cd /Users/brajkovic/Projects/atc && grep '"0.0.0"' .release-please-manifest.json` returns zero hits for the new crates).
- **AC3**: `cd /Users/brajkovic/Projects/atc && bash scripts/doc-mapping.sh backend/crates/atc-store-pg/src/listener.rs` prints both `docs/architecture/backend-server.md` AND `docs/architecture/metrics.md` on separate lines.
- **AC4**: `cd /Users/brajkovic/Projects/atc && bash scripts/doc-mapping.sh backend/crates/atc-store-pg/src/store/writes.rs` prints `docs/architecture/backend-server.md` (catch-all reachable for non-straddler paths).
- **AC5**: `cd /Users/brajkovic/Projects/atc && git grep -E "must have a slim CLAUDE\.md" CLAUDE.md` returns content with the two-tier framing (skeleton + reactive). Same for `CONTRIBUTING.md` "Directory-Level CLAUDE.md Files".
- **AC6**: `docs/architecture-decisions/0005-persistentstore-trait-relocation.md` contains a `> **Revised by ADR-0008:**` annotation block.
- **AC-fail-1**: `release-please-config.json`'s `linked-versions` "atc" group's `components` array does NOT have any unrelated package added (regression check — verify only the 4 new crates joined the existing 4).

### Phase 1 (atc-wire + atc-persist + rename)

- **AC7**: Both crate directories exist: `backend/crates/atc-wire/{Cargo.toml,src/lib.rs,CLAUDE.md,AGENTS.md}` and `backend/crates/atc-persist/{Cargo.toml,src/lib.rs,src/join.rs,CLAUDE.md,AGENTS.md}`.
- **AC8**: `cd /Users/brajkovic/Projects/atc/backend && cargo build -p atc-wire` succeeds; `cd /Users/brajkovic/Projects/atc/backend && cargo build -p atc-persist` succeeds.
- **AC9**: `backend/crates/atc-persist/Cargo.toml`'s `[dependencies]` table contains exactly: `atc-core`, `atc-wire`, `async-trait` (workspace), `tokio` (with features `["sync", "time", "rt"]`), `tracing` (workspace). Disallowed entries (`sqlx`, `serde`, `ts-rs`, `atc-github`, `redis`, `mongodb`) absent. Verify by reading the manifest directly — `cargo tree -p atc-persist` shows transitive deps from atc-wire (serde, ts-rs, atc-github) and is therefore unsuitable for direct-dep checking.
- **AC10**: `backend/crates/atc-wire/Cargo.toml`'s `[dependencies]` contains atc-core, atc-github, serde, ts-rs (workspace). Disallowed: sqlx, tokio, async-trait. Verify by reading the manifest directly.
- **AC11**: `cd /Users/brajkovic/Projects/atc && git grep -l "SeqEvent" -- backend/ frontend/src/ frontend/e2e/ docs/architecture/ ':!docs/architecture/state-externalization-research/'` returns zero hits. **Path-scope rationale**: includes active code (backend/, frontend/src/, frontend/e2e/) and canonical architecture docs; **excludes `docs/architecture/state-externalization-research/`** because those are historical pre-design research artifacts (~27 SeqEvent refs documenting the team's THINKING before the wire-type was named). Renaming retroactively would distort the historical record. **All affected files in scope**: `frontend/src/lib/{connection,dispatcher}.ts`, `frontend/src/lib/aria/{transition-kinds,live-region.svelte}.ts`, all 7 `frontend/src/lib/**/*.test.ts` files, `frontend/src/lib/types/generated/CommittedEvent.ts` (new) with `SeqEvent.ts` removed, `frontend/e2e/lib/ws-mock.ts`, `frontend/e2e/{pool-filter,pool-indicators,run-detail-panel}.test.ts` (codex pass-2 found these import the wire type via test fixtures), `frontend/CLAUDE.md`, `docs/architecture/{backend-server,frontend-app,deployment}.md`, `backend/crates/atc-server/CLAUDE.md`. The plan file at `docs/design-plans/2026-05-15-persistence-crate-split.md` and ADR-0008 at `docs/architecture-decisions/0008-persistence-crate-split.md` are excluded by the path filter — both intentionally retain "SeqEvent" references in the historical context discussion.
- **AC12**: `cd /Users/brajkovic/Projects/atc && git grep -l "atc_server::state::CommittedEvent\|atc_server::state::StateSnapshot\|atc_server::state::SeqEvent" backend/` returns zero hits.
- **AC13**: `frontend/src/lib/types/generated/SeqEvent.ts` does NOT exist; `frontend/src/lib/types/generated/CommittedEvent.ts` exists.
- **AC14**: `LivenessError::DbUnreachable` accepts `Box<dyn std::error::Error + Send + Sync + 'static>` (verify by reading `backend/crates/atc-persist/src/lib.rs` and confirming the variant signature).
- **AC15**: `LivenessError` implements `std::error::Error` with a non-empty `source()` for the `DbUnreachable` variant (verify code; failure mode: a synthetic test that wraps a known error in `DbUnreachable` and asserts `.source().is_some()` should pass).
- **AC16**: `backend/Cargo.toml` `[workspace.dependencies]` contains `async-trait = "=0.1.89"`; `backend/crates/atc-server/Cargo.toml` references `async-trait = { workspace = true }`.
- **AC17**: `just types && just check && just lint && just test` all green.
- **AC18**: `pnpm test` (jsdom + browser projects) green; `pnpm exec tsc --noEmit` zero errors.
- **AC19**: `docs/architecture-decisions/0008-persistence-crate-split.md` exists with Status: Accepted and references issue #169.
- **AC20**: `backend/crates/atc-wire/CLAUDE.md` has a corresponding `AGENTS.md` symlink (`ls -la backend/crates/atc-wire/AGENTS.md` shows it points to `CLAUDE.md`); same for `atc-persist`.
- **AC-fail-2**: `cd /Users/brajkovic/Projects/atc && git grep "fn join_with_timeout" backend/crates/atc-server/` returns zero hits (the local definition was moved out, not duplicated).
- **AC-fail-3**: `backend/crates/atc-persist/Cargo.toml`'s `tokio` entry has features exactly `["sync", "time", "rt"]` and no others (no `macros`, `net`, `process`, `signal`, `io-util`, `io-std`, `fs`, `rt-multi-thread`). Verify by reading the manifest.

### Phase 2 (atc-store-pg)

- **AC21**: Directory structure exists: `backend/crates/atc-store-pg/src/{lib.rs,listener.rs,reads.rs,db.rs,metrics.rs,invariants.rs}`, `backend/crates/atc-store-pg/src/store/{mod.rs,writes.rs,test_hooks.rs,retention.rs}`, `backend/crates/atc-store-pg/migrations/{0001,0002,0003,0004}*.sql`, `backend/crates/atc-store-pg/{Cargo.toml,CLAUDE.md,AGENTS.md}`.
- **AC22**: `cd /Users/brajkovic/Projects/atc/backend && cargo sqlx prepare --check` passes (no `.sqlx/` regeneration required).
- **AC23**: `cd /Users/brajkovic/Projects/atc && git grep -l "atc_server::persist::pg\|atc_server::listener\|atc_server::db" backend/" returns zero hits — call sites all moved.
- **AC24**: `cd /Users/brajkovic/Projects/atc/backend && cargo tree -p atc-store-pg | grep -E "(^|[│ ])sqlx "` returns at least one match (sqlx IS a direct dep).
- **AC25**: `backend/crates/atc-server/Cargo.toml`'s `[dependencies]` table contains NO `sqlx` entry; `cd /Users/brajkovic/Projects/atc && git grep -E "use sqlx|sqlx::" backend/crates/atc-server/src/` returns zero hits. **However, sqlx remains in atc-server's `[dev-dependencies]`** (132+ use sites in `tests/integration/` for ephemeral-DB CREATE/DROP and query helpers — these stay with atc-server per the B-Lite test reorg). The DbInitError abstraction removes the last production-source sqlx use site; atc-server's production binary pulls sqlx only transitively through atc-store-pg.
- **AC26**: `cd /Users/brajkovic/Projects/atc && git grep "PgMetrics::register\|PgMetrics {" backend/crates/atc-server/` returns zero hits (PgMetrics moved entirely).
- **AC27**: `cd /Users/brajkovic/Projects/atc && git grep "fn register_build_info\|fn spawn_process_collector" backend/crates/atc-server/src/metrics.rs` returns 2 matches (these stayed).
- **AC28**: `cd /Users/brajkovic/Projects/atc && git grep "atc-server/migrations" backend/` returns zero hits; `ls backend/crates/atc-store-pg/migrations/` shows all 4 SQL files.
- **AC29**: `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-store-pg --features test-support` succeeds (compile + run; even with no tests in the crate yet, must compile).
- **AC30**: `just test` green for full workspace.
- **AC31**: `backend/crates/atc-store-pg/CLAUDE.md` has the sqlx + .sqlx/ regen sharp-edge sections.
- **AC-fail-4**: `cd /Users/brajkovic/Projects/atc && git grep "spawn_listener_task\|spawn_drain_task" backend/crates/atc-server/src/" returns zero hits (these spawn functions moved to atc-store-pg).
- **AC-fail-5**: `cd /Users/brajkovic/Projects/atc/backend && cargo build -p atc-store-mem 2>&1 | grep "sqlx"` returns zero hits (atc-store-mem must NOT pull in sqlx through any transitive path). The `atc-github` part of the original AC is **dropped** per Codex blocker 3 — atc-store-mem requires direct atc-github for `WebhookEvent` construction.

### Phase 3 (atc-store-mem)

- **AC32**: Directory structure: `backend/crates/atc-store-mem/{Cargo.toml,src/lib.rs,src/invariants.rs,CLAUDE.md,AGENTS.md}`.
- **AC33**: `backend/crates/atc-store-mem/Cargo.toml`'s `[dependencies]` table contains `atc-github` (required for `WebhookEvent` construction in broadcast path) but NO `sqlx` (in-memory store does no DB I/O). Verify by reading the manifest. The "atc-store-mem stays sqlx-free" property holds; the "atc-store-mem has no atc-github dep" claim from the original synthesis is **inaccurate** and is corrected here per Codex blocker 3.
- **AC34**: `cd /Users/brajkovic/Projects/atc && git grep "atc_server::persist::in_memory\|atc_server::persist::InMemoryStore" backend/" returns zero hits.
- **AC35**: `cd /Users/brajkovic/Projects/atc && git grep -l "atc_store_mem::InMemoryStore" backend/crates/atc-server/" returns at least 2 matches (main.rs + integration test imports).
- **AC36**: `backend/crates/atc-server/Cargo.toml` has NO `[features] test-support = []` block AND NO `atc-server = { path = ".", ... }` self-ref in `[dev-dependencies]`. `metrics.rs:436`'s `from_join_handle` is gated `#[cfg(test)]` (not the wider `cfg(any(test, feature = "test-support"))`). Verify by reading the manifest + the metrics.rs cfg attr.
- **AC37**: `just test` green for full workspace.
- **AC38**: `backend/crates/atc-server/src/persist/` directory does not exist (or contains only `mod.rs` re-exports if any non-store code remained; verify by `ls`).
- **AC-fail-6**: `cd /Users/brajkovic/Projects/atc && git grep "atc_persist::join_with_timeout" backend/crates/atc-store-mem/src/` returns at least 1 match (in `InMemoryStore::shutdown()`); same for atc-store-pg.

### Cross-cutting (every phase)

- **AC39**: After every PR merges to `main`, on a fresh `git checkout main && just setup && just test`, all tests are green.
- **AC40**: `bash scripts/check-docs-lefthook.sh` (the doc-staleness pre-push gate) returns clean on every PR push.
- **AC41**: `cargo deny check` (or whatever workspace-level dep audit is in CI) succeeds on every PR.

## Documents to Update

| Document | Phase | Change |
|---|---|---|
| `docs/design-plans/2026-05-15-persistence-crate-split.md` | 0 | NEW — copy of this plan, committed at branch creation. |
| `release-please-config.json` | 0 | Add 4 new packages; add to linked-versions "atc" group. |
| `.release-please-manifest.json` | 0 | Add 4 entries at "0.1.0". |
| `scripts/doc-mapping.sh` | 0 | Add case branches for new crate paths; preserve straddler ordering. |
| `CLAUDE.md` (root) | 0 | Rephrase "Slim CLAUDE.md in every domain directory" invariant to two-tier framing. |
| `CONTRIBUTING.md` | 0 | Align "Directory-Level CLAUDE.md Files" section to two-tier framing. |
| `docs/architecture-decisions/0005-persistentstore-trait-relocation.md` | 0 | Add `> **Revised by ADR-0008:**` annotation. |
| `docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md` | 0 | Annotate any geographic claims (verify with grep first). |
| `docs/architecture/release-pipeline.md` | 0 | Document the 4 new packages (if file exists). |
| `docs/architecture-decisions/0008-persistence-crate-split.md` | 1 | NEW — ADR documenting the four-crate split, the rename, the bus-inversion + tuple-decomposition rejections. |
| `docs/architecture/backend-server.md` | 1, 2, 3 | Phase 1: rename `SeqEvent → CommittedEvent` + update trait location. Phase 2: PG-mode file references + DbInitError abstraction note. Phase 3: in-memory mode file references. |
| `docs/architecture/frontend-app.md` | 1 | 6 references to rename: type references at lines 524, 549, 576, 584, 729 + the `makeJobSeqEvent` helper-name reference at line 752. The doc-staleness gate maps `frontend/src/*` → this file (`scripts/doc-mapping.sh:62`), so the doc must change in the same PR as the frontend rename. |
| `docs/architecture/deployment.md` | 1 | 3 references to rename at lines 411, 419, 433 — operator runbook commands and acceptance text describing WS-tap log inspection. |
| `frontend/CLAUDE.md` | 1 | 2 references to rename at lines 22 (e2e helper name) and 51 (typed-union convention example using the helper). |
| `frontend/e2e/lib/ws-mock.ts` | 1 | 3 references to rename, including the EXPORTED helper `makeJobSeqEvent` → `makeJobCommittedEvent` (harness public surface). |
| `docs/architecture/metrics.md` | 2 | Update `PgMetrics` location reference (now in atc-store-pg). `register_build_info` and `spawn_process_collector` references stay (those helpers stay in atc-server). |
| `backend/crates/atc-server/CLAUDE.md` | 1, 2, 3 | Phase 1: drop `SeqEvent` references; add atc-wire/atc-persist pointers; rephrase `state` and `persist` module entries. Phase 2: drop pg-mode file references; add note that DbInitError is now imported from atc-store-pg. Phase 3: drop in-memory mode references. |
| `backend/crates/atc-wire/CLAUDE.md` (+ AGENTS.md symlink) | 1 | NEW — skeleton tier. |
| `backend/crates/atc-persist/CLAUDE.md` (+ AGENTS.md symlink) | 1 | NEW — skeleton + sqlx-free invariant sharp edge. |
| `backend/crates/atc-store-pg/CLAUDE.md` (+ AGENTS.md symlink) | 2 | NEW — skeleton + sqlx + .sqlx/ regen + migrations atomicity sharp edges. |
| `backend/crates/atc-store-mem/CLAUDE.md` (+ AGENTS.md symlink) | 3 | NEW — skeleton tier. |

## Out of Scope

- **`atc-test-helpers` crate + per-store `tests/integration/` reorganization** — Test reorg Option B. Deferred to follow-up issue. Requires resolving the `exponential_histogram_view` placement (3 candidates: leave in atc-server / new `atc-otel-config` micro-crate / inline into atc-test-helpers) and `register_build_info` policy (3 candidates: move / skip / duplicate). Owned by a future issue, not this PR series.
- **Splitting `in_memory_store_tests.rs` (1170 LOC) and `outbox_tests.rs` (1058 LOC)** — both exceed the 500-line behavioral test rule. Tuvok C3 § 7 Rule 1 gap. Future cleanup; not blocking #169.
- **Decoupling `SeqEvent.event` (now `CommittedEvent.event`) from `WebhookEvent`** — full domain-event envelope rework. W2's "coordination note to Riker." Out of scope; current coupling is pragmatic.
- **Decoupling `atc-store-pg::listener` from `atc_github::WebhookEvent`** — W3 § Open Question 3. Scope creep; defer.
- **PG-off / mem-off feature-gating** — Spock C1 § 4 explicit out-of-scope. Future architectural choice if/when alternative storage backends materialize.
- **Native `dyn`-safe AFIT migration** — waits on rust compiler. No committed date. `async-trait` macro stays in place.
- **ADR-0006 task ownership inversion** — discussed in plan-mode follow-up; would require re-doing ADR-0006 (accepted 2026-05-13). If ever revisited, that's a separate issue.
- **Visual / UI changes to the frontend** — the `SeqEvent → CommittedEvent` rename is purely internal to the TypeScript module graph; no user-facing changes.

## Glossary

- **Wire types**: serializable types (with `#[derive(TS)]` + `#[ts(export)]`) that cross the WebSocket / REST boundary to the frontend. Today: `SeqEvent`, `StateSnapshot`. After this PR: `CommittedEvent`, `StateSnapshot`.
- **`CommittedEvent`** (formerly `SeqEvent`): the broadcast envelope a store emits AFTER committing a write — `seq` is monotonic in commit order, `event` is the validated `WebhookEvent`. The rename clarifies that this is a committed domain event, not a serialization shape.
- **Trait crate**: a Rust crate whose only purpose is to define a trait + small shared types. Carries minimum deps. Here: `atc-persist`.
- **PG-B**: La Forge W3's option of decomposing `pg.rs` into `store/{mod, writes, test_hooks, retention}.rs`. Folded into Phase 2 per user confirmation.
- **OC-N**: Ordering Constraint N. Atomic gates each phase must satisfy. Originally enumerated by Scotty C2 § 7.
- **Skeleton tier (CLAUDE.md)**: the mandatory minimum content for a directory CLAUDE.md — purpose, pointer to canonical architecture doc, AGENTS.md symlink presence. Sharp-edges content is added reactively when agents encounter friction.
- **Self-ref dev-dep**: the `<crate-name> = { path = ".", features = ["test-support"] }` pattern in `[dev-dependencies]` that activates a crate's `test-support` feature when building its own tests. Standard Cargo idiom for exposing test-only inspection helpers.
- **Straddler case (`doc-mapping.sh`)**: a path-pattern that maps to MORE than one architecture doc (e.g., `atc-store-pg/src/listener.rs` maps to BOTH `backend-server.md` AND `metrics.md`). Must be listed before the workspace-level catch-all because `case` matching is first-match.
