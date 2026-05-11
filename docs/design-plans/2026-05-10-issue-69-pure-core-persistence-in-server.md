# Issue #69 — Pure atc-core, all persistence in atc-server

## Context

Issue #69 (open, labels: design + server) asks to unify the `GET /v1/state` read path through `PersistentStore` so the handler no longer branches on `state.pg_pool.is_some()`. The owner comment (2026-05-09) widened scope: also separate persistence from the state machine — eviction needs `tokio-util` in `atc-core`, which is a server concern. The user's prompt frames the end goal as **all state persistence lives in `atc-server`** and **only the state machine and encoding of allowed state transitions live in `atc-core`, as close to pure as possible**.

What is true today (worktree at `fix/otlp-https-tls`, prerequisites #50 and #60 closed):

- `backend/crates/atc-core/src/state_machine.rs` (604 lines) defines `RunStateMachine` with `RwLock<StateData>`, `Arc<dyn Clock>`, `completed_ttl: Duration`, async `apply_run_event` / `apply_job_event` (acquire write lock, validate transition, build new entity, insert), `evict_expired`, and `start_eviction_task` (spawns a tokio task using `tokio_util::sync::CancellationToken`). It also has `query_by_repos`, `query_all`, `snapshot`, `get_run`, `get_job`, `jobs_for_run`, `jobs_for_repo`. Investigator confirmed: **no production callers in `atc-server` for the query methods — all 70+ call sites are inside atc-core test files**. `tokio::sync::RwLock` and `tokio_util::sync::CancellationToken` are the **only** tokio surface in atc-core; no other module in atc-core is async.
- `backend/crates/atc-core/src/persist.rs` (85 lines, post-#50) holds only `PersistError`. The trait moved to atc-server per ADR 0005 (`docs/architecture-decisions/0005-persistentstore-trait-relocation.md`).
- `backend/crates/atc-server/src/persist.rs` (1310 lines) holds the `PersistentStore` trait, `PgStore`, `InMemoryStore` (currently wraps `Arc<RunStateMachine>` from atc-core), outbox transaction helpers, `read_all_runs` / `read_all_jobs` free fns. The in-memory `apply_*_event` impls hold the `seq` mutex across atc-core state mutation + broadcast.
- `backend/crates/atc-server/src/state.rs` defines `AppState` with fields `state_machine`, `seq`, `pg_pool`, `broadcast_watermark`, `min_pending_seq`, `last_drain_pass_at`, `webhook_tx`, `webhook_secret`, `shutdown`, `ws_tracker`.
- `backend/crates/atc-server/src/routes.rs::state_handler` (~lines 124-201) branches on `state.pg_pool.is_some()`. PG branch: loads `state.broadcast_watermark`, opens REPEATABLE READ on `state.pg_pool`, calls `read_all_runs` / `read_all_jobs`. In-memory branch: locks `state.seq`, calls `state.state_machine.snapshot().await`. Both build a private `StateSnapshot { last_seq, runs, jobs }` defined in routes.rs.
- `backend/crates/atc-server/src/routes.rs::readyz` (~lines 59-100) reads `state.pg_pool` (`SELECT 1`), `state.last_drain_pass_at` (heartbeat staleness threshold 30 s), and `state.shutdown`.
- Eviction is spawned **unconditionally** in `main.rs:224-226` via `app_state.state_machine.start_eviction_task(Duration::from_secs(60), shutdown.clone())` — even in PG mode where the state machine is dormant. Issue #60 (closed 2026-05-09) standardized supervision: `CancellationToken` + `JoinHandle<()>` + biased `tokio::select!` for cooperative cancellation, plus `TaskTracker` for WS handlers.
- Frontend ts-rs exports already include `StateSnapshot.ts`, `SeqEvent.ts`, `RunnerPoolStats.ts` (21 generated files total under `frontend/src/lib/types/generated/`). Moving a struct between backend crates does not change the generated path.

## Definition of Done

1. **`/v1/state` route handler does not branch on storage mode** — it dispatches once through `state.persist.read_snapshot().await`.
2. **`/readyz` route handler does not branch on storage mode** — it dispatches through `state.persist.liveness_check().await` and maps the result to the existing `{ok, db_unreachable, drain_stale, shutting_down}` JSON status surface.
3. **`AppState` shrinks to handler-only fields**: `persist`, `webhook_tx`, `webhook_secret`, `shutdown`, `ws_tracker`. Removed: `state_machine`, `seq`, `pg_pool`, `broadcast_watermark`, `min_pending_seq`, `last_drain_pass_at`. (Storage-mode-specific Arcs move into the appropriate `PersistentStore` impl; spawn-only plumbing stays in `main.rs` locals.)
4. **`atc-core` has no dependency on `tokio` or `tokio-util`** — those are removed from `backend/crates/atc-core/Cargo.toml`. atc-core retains `chrono`, `serde`, `ts-rs`, `tracing`.
5. **`RunStateMachine` (the struct) is deleted from atc-core.** atc-core's `state_machine.rs` retains only: `StateMachineError`, pure free functions `apply_run_event(existing: Option<WorkflowRun>, env: RunEventEnvelope) -> Result<WorkflowRun, StateMachineError>`, `apply_job_event(existing: Option<Job>, env: JobEventEnvelope) -> Result<Job, StateMachineError>`, and predicate `is_evictable(job: &Job, now: DateTime<Utc>, ttl: Duration) -> bool`.
6. **`InMemoryStore` (in atc-server) owns** the HashMap state (`runs`, `jobs`, `jobs_by_run`, `jobs_by_repo`), `Arc<dyn Clock>`, `completed_ttl`, `seq: Mutex<u64>`, and a clone of `webhook_tx`. Its `apply_*_event` methods delegate to atc-core's pure functions, then maintain the secondary indexes and seq under the lock. Its `evict_expired` iterates the HashMap calling `atc_core::is_evictable` and mutates indexes locally.
7. **`PgStore` (in atc-server) owns** the `Arc<AtomicI64>` for `broadcast_watermark` and `last_drain_pass_at`; exposes them to the drain spawn via a `drain_handles()` accessor returning `DrainHandles { watermark, heartbeat }`. `read_snapshot` and `liveness_check` are implemented against these owned Arcs.
8. **The eviction task is a free function** `spawn_eviction_task(store: Arc<InMemoryStore>, interval: Duration, cancel: CancellationToken) -> JoinHandle<()>` in atc-server, following the #60 supervision shape (`biased` select, cooperative cancel). It is spawned **only in in-memory mode** from `main.rs`.
9. **Existing behavior preserved end-to-end**: webhook → outbox → drain → broadcast invariants (ADR 0003) hold; `/v1/state` REPEATABLE READ + watermark-before-snapshot invariant (ADR 0002 / Phase 3c) holds; HMAC-SHA256 webhook verification still gates writes; OTel boundary spans (`persist.apply.run_event`, `persist.apply.job_event`, `persist.notify.emit`) fire from `PersistentStore` impl methods.
10. **Test coverage maintained**: pure-transition tests in atc-core (forward-only, idempotent, first-sight) test the new free functions directly. Store-level tests (eviction sweep, index consistency, query semantics) migrate to atc-server alongside `InMemoryStore`. Test fixtures (`build_app_with_secret`, `build_app_no_secret`, PG fixture in `tests/integration/common/mod.rs`) construct `InMemoryStore` or `PgStore` directly and pass via `persist: Arc<dyn PersistentStore>`. `cargo nextest run` is green for both crates.
11. **Documentation updated**: `backend-server.md` reflects new module boundaries; both `CLAUDE.md` files (atc-core, atc-server) updated; `scripts/doc-mapping.sh` mapping for `state_machine.rs` retargets if its content scope changes; ADR 0002 / 0003 cross-references stay accurate.

## Locked Decisions

These were established in prior phases and are **not open for re-evaluation**:

- **`PersistentStore` lives in `atc-server::persist`** — ADR 0005 (`docs/architecture-decisions/0005-persistentstore-trait-relocation.md`), shipped in PR for #50.
- **Outbox semantics** — `BIGSERIAL` pre-commit allocation, drain commit-order watermark, REPEATABLE READ snapshot, drain as sole broadcaster in PG mode — ADR 0002 + ADR 0003 (`docs/architecture-decisions/0002-state-externalization-postgres-outbox.md`, `0003-state-cursor-contract-and-operator-policy.md`).
- **`predecessors_of()`** stays in atc-core; `PgStore` parameterizes SQL WHERE clauses with these slices for predicated UPSERTs (`atc-core/CLAUDE.md` § Contracts).
- **Snapshot wire shape** — `{ lastSeq, runs, jobs }` camelCase — frontend contract from Phase 3c. Not changing here.
- **Supervision pattern** from issue #60 — `CancellationToken` + `JoinHandle<()>` + `biased` select; `TaskTracker` for WS — `docs/design-plans/2026-05-09-supervision-and-shutdown.md`. The new `spawn_eviction_task` follows this shape.
- **OTel boundary instrumentation** is at the `PersistentStore` impl boundary, not the route handler.

User choices confirmed for this plan:
- **Single PR** delivering the full target.
- **Per-job evictability predicate** (`is_evictable(&Job, now, ttl) -> bool`) — atc-core exposes only the predicate; atc-server iterates the HashMap.
- **`liveness_check()` on the trait** — `pg_pool` drops from `AppState` entirely.

## Architecture

### Module shape after the change

**`atc-core` (pure):**

```
backend/crates/atc-core/src/
├── lib.rs              # re-exports trimmed (no RunStateMachine, no QueryResult)
├── clock.rs            # Clock trait, SystemClock, TestClock (unchanged)
├── event.rs            # RunEvent/JobEvent + envelopes (unchanged)
├── job.rs + job/       # Job, JobStatus, Step, etc. (unchanged)
├── run.rs              # WorkflowRun, RunStatus, predecessors_of (unchanged)
├── types.rs            # RunId, JobId, RepoKey, LabelSet, RunnerPoolStats (unchanged; RunnerPoolStats stays here as a derived ts-rs wire type with no methods)
├── persist.rs          # PersistError (unchanged)
└── state_machine.rs    # NEW SHAPE: only StateMachineError + pure free fns
                        #   apply_run_event(Option<WorkflowRun>, RunEventEnvelope)
                        #     -> Result<WorkflowRun, StateMachineError>
                        #   apply_job_event(Option<Job>, JobEventEnvelope)
                        #     -> Result<Job, StateMachineError>
                        #   is_evictable(&Job, DateTime<Utc>, Duration) -> bool
```

`Cargo.toml`: drop `tokio`, `tokio-util`. Keep `chrono`, `serde`, `ts-rs`, `tracing`. (`tracing` stays — `tracing::debug!` calls in pure functions are fine; the crate doesn't require an async runtime.)

**`atc-server::persist` (module split for readability):**

```
backend/crates/atc-server/src/persist/
├── mod.rs              # PersistentStore trait, LivenessError, PersistError re-export
├── in_memory.rs        # InMemoryStore (HashMap + RwLock + clock + ttl + seq + webhook_tx)
│                       #   - state: RwLock<StateData> { runs, jobs, jobs_by_run, jobs_by_repo }
│                       #   - apply_*_event: lock seq → write-lock state → call atc_core::apply_*_event
│                       #     → update indexes if first-sight → increment seq → broadcast → return seq
│                       #   - read_snapshot: lock seq → read-lock state → build StateSnapshot { last_seq, runs, jobs }
│                       #   - liveness_check: returns Ok(()) immediately
│                       #   - evict_expired: write-lock state → filter by atc_core::is_evictable → remove + reindex
├── pg.rs               # PgStore (pool + broadcast_watermark Arc + last_drain_pass_at Arc + metrics)
│                       #   - drain_handles() -> DrainHandles { watermark, heartbeat } for drain spawn
│                       #   - apply_*_event: existing outbox INSERT + NOTIFY (unchanged)
│                       #   - read_snapshot: load watermark, REPEATABLE READ tx, read_all_runs/read_all_jobs
│                       #   - liveness_check: SELECT 1 → DbUnreachable if fails;
│                       #     then check heartbeat age → DrainStale if > 30s threshold
├── reads.rs            # read_all_runs, read_all_jobs (current free fns; relocated module-internal)
└── eviction.rs         # spawn_eviction_task(Arc<InMemoryStore>, Duration, CancellationToken)
                        #   -> JoinHandle<()>
```

`PersistentStore` trait gains two methods:

```rust
#[async_trait]
pub trait PersistentStore: Send + Sync {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError>;
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError>;
    async fn read_snapshot(&self) -> Result<StateSnapshot, PersistError>;
    async fn liveness_check(&self) -> Result<(), LivenessError>;
}

pub enum LivenessError {
    DbUnreachable(sqlx::Error),
    DrainStale { age_ms: i64 },
}
```

**`atc-server::state` (AppState shrinks):**

```rust
pub struct AppState {
    pub persist: Arc<dyn PersistentStore>,
    pub webhook_tx: broadcast::Sender<SeqEvent>,
    pub webhook_secret: Option<String>,
    pub shutdown: CancellationToken,
    pub ws_tracker: TaskTracker,
}

// Wire types live in state.rs alongside SeqEvent:
//   pub struct SeqEvent      (existing)
//   pub struct StateSnapshot (moved in from routes.rs, pub-visible so it can appear in PersistentStore trait signature)
```

Removed fields: `state_machine`, `seq`, `pg_pool`, `broadcast_watermark`, `min_pending_seq`, `last_drain_pass_at`. Their roles:

| Removed field | New home |
|---|---|
| `state_machine: Arc<RunStateMachine>` | Deleted. `InMemoryStore` owns `StateData` directly. |
| `seq: Arc<Mutex<u64>>` | Private field inside `InMemoryStore`. |
| `pg_pool: Option<sqlx::PgPool>` | Local in `main.rs` until handed to `PgStore::new(pool).await?`; listener/drain spawn args take their own clones from the `main.rs` local. |
| `broadcast_watermark: Arc<AtomicI64>` | Private field inside `PgStore`. Exposed to drain via `pg_store.drain_handles().watermark`. |
| `min_pending_seq: Arc<AtomicI64>` | Local in `main.rs`; cloned into listener and drain spawns. |
| `last_drain_pass_at: Arc<AtomicI64>` | Private field inside `PgStore`. Exposed to drain via `pg_store.drain_handles().heartbeat`. `liveness_check` reads it directly. |

**`atc-server::routes` collapses:**

```rust
async fn state_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.persist.read_snapshot().await {
        Ok(snap) => Json(snap).into_response(),
        Err(e) => {
            tracing::error!(error = ?e, "state_handler: snapshot failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "snapshot failed"}))).into_response()
        }
    }
}

async fn readyz(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if state.shutdown.is_cancelled() {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { status: "shutting_down" })).into_response();
    }
    match state.persist.liveness_check().await {
        Ok(()) => (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response(),
        Err(LivenessError::DbUnreachable(e)) => {
            tracing::warn!(error = %e, "readyz: db check failed");
            (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { status: "db_unreachable" })).into_response()
        }
        Err(LivenessError::DrainStale { age_ms }) => {
            tracing::warn!(age_ms, "readyz: drain heartbeat stale");
            (StatusCode::SERVICE_UNAVAILABLE, Json(HealthResponse { status: "drain_stale" })).into_response()
        }
    }
}
```

### Key design decisions and rejected alternatives

1. **Pure free functions vs encapsulated struct in atc-core.** Picked pure functions. Rejected keeping `RunStateMachine` as an interior-mutable struct in atc-core (Option B in prior draft) because it would retain tokio in atc-core — defeating the "as close to pure as possible" goal. The signature `apply_run_event(Option<WorkflowRun>, env) -> Result<WorkflowRun, _>` matches the existing remove-then-insert pattern (see `feedback_cow_semantics.md`).

2. **Per-job predicate vs sweep helper.** Picked per-job predicate (`is_evictable(&Job, now, ttl) -> bool`). Server iterates `state.jobs.values()` and calls the predicate. atc-core has no notion of collections — purer surface. Confirmed by user.

3. **`liveness_check()` on trait vs `pg_pool` on AppState.** Picked trait method. Confirmed by user. Cost: a new `LivenessError` enum and a trivially-Ok implementation on `InMemoryStore`. Benefit: AppState drops `pg_pool` entirely; readyz uniformly dispatches through `persist`.

4. **`broadcast_watermark` and `last_drain_pass_at` ownership.** `PgStore::new(pool).await -> sqlx::Result<Self>` becomes async and encapsulates outbox-table knowledge: it runs `SELECT COALESCE(MAX(seq), 0) FROM outbox`, allocates `broadcast_watermark = Arc::new(AtomicI64::new(initial_watermark))` from that, and allocates `last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()))`. `main.rs` does **not** know the SQL — it just `let pg_store = PgStore::new(pool.clone()).await?;`. `PgStore` exposes `drain_handles() -> DrainHandles { watermark, heartbeat }` (cloned Arcs) so `main.rs` can pass the writer Arcs into `spawn_drain_task` without coupling to internals. The drain task continues to be the sole writer; `PgStore` is the sole reader through `read_snapshot` and `liveness_check`. **Boot-order invariant preserved** (per ADR 0002): `init_pool → connect_listener (LISTEN registered) → PgStore::new (await, seeds watermark) → spawn_listener_task → spawn_drain_task`. The LISTEN registration happens before the watermark seed, so any commits landing during seeding register a NOTIFY that the listener task (spawned next) drains via `min_pending_seq`. **Memory-ordering contract**: drain task writes via `Release` (`watermark.store(seq, Release)`, `heartbeat.store(now_ms, Release)`); `PgStore::read_snapshot` reads via `Acquire` (`watermark.load(Acquire)` BEFORE opening the REPEATABLE READ transaction); `PgStore::liveness_check` reads heartbeat via `Acquire` (or `Relaxed` — Relaxed matches the current `routes.rs:84` pattern and is sufficient because heartbeat staleness is monotonic from the reader's perspective). This matches the existing ordering in `routes.rs::state_handler` (Acquire load before the tx) and `listener.rs` (Release stores on watermark advancement and heartbeat refresh). Rejected: passing `initial_watermark` as a constructor parameter — leaks outbox-table knowledge into `main.rs` and every test fixture; encapsulation argues for internal seeding. Rejected: shared Arc passed independently between AppState/drain/PgStore (existing pattern) — gratuitously plumbs the Arc through three places.

5. **Eviction task as free function vs trait method.** Picked free function `spawn_eviction_task(Arc<InMemoryStore>, …)`. Rejected adding `start_eviction` to the trait because it would be a no-op on `PgStore` (PG mode has no in-memory state to evict). Main.rs naturally spawns it only in in-memory mode by holding the concrete `Arc<InMemoryStore>` before erasing to `Arc<dyn PersistentStore>`. This also fixes a latent waste: today's main.rs spawns eviction even in PG mode, where the state machine is dormant.

6. **`StateSnapshot` placement.** Moved from `routes.rs` (private) to `state.rs` (public) alongside `SeqEvent`. Reason: the trait method signature `read_snapshot() -> Result<StateSnapshot, _>` requires the type to be importable from `persist`. ts-rs `#[ts(export)]` annotation carries over; the generated path `frontend/src/lib/types/generated/StateSnapshot.ts` is unchanged.

7. **`QueryResult` deletion.** Currently `RunStateMachine::snapshot()` returns `QueryResult { runs, jobs }` which `state_handler` then wraps in `StateSnapshot`. After unify, `InMemoryStore::read_snapshot` returns `StateSnapshot` directly. `QueryResult` is removed. The query methods (`query_by_repos`, `query_all`, `get_run`, etc.) are also removed — investigator confirmed zero production callers; all 70+ uses are in atc-core tests that we will reorganize (pure-transition tests stay using the new free functions; store-level tests move to atc-server with direct `InMemoryStore` access).

### Coupling-site enumeration (per planning-workflow.md § "Coupling-site enumeration")

Concrete edit set, by surface:

**Removed-field handler/main references in `backend/crates/atc-server/src/` (8 sites):**
- `main.rs:225` — `app_state.state_machine.start_eviction_task(...)` → conditional `spawn_eviction_task(in_memory_clone, ...)` only in in-memory mode.
- `routes.rs:191` — `state.state_machine.snapshot()` → removed (replaced by trait dispatch).
- `routes.rs:190` — `state.seq.lock()` → removed.
- `persist.rs:319, 345` — `self.seq.lock()` references inside `InMemoryStore` impl — these are *internal* and remain (just become private field accesses on the new `InMemoryStore`).
- `routes.rs:69, 125` — `state.pg_pool.is_some()` / pool checks → removed; readyz/state_handler dispatch through trait.
- `routes.rs:127` — `state.broadcast_watermark.load(Acquire)` → removed; moves inside `PgStore::read_snapshot`.
- `routes.rs:84` — `state.last_drain_pass_at.load(Relaxed)` → removed; moves inside `PgStore::liveness_check`.

**`AppState` struct-literal constructors (23 sites — 1 production + 22 tests):**
- `main.rs:197` (production) — rewrite to construct with 5 fields.
- 22 test fixtures across `tests/integration/`: `transactional_writes_tests.rs` (×2), `readyz.rs` (×5), `webhook_ingestion_tests.rs` (×1), `state_tests.rs` (×1), `graceful_shutdown.rs` (×1), `outbox_tests.rs` (×1), `no_metrics_endpoint_test.rs` (×1), plus the helpers in `common/mod.rs` (`build_app_with_secret` at line 337, `build_app_no_secret` at line 370, PG fixture at line 622). All 22 rewrite to construct via the helpers (collapsing duplication is welcome but not required).

**Store constructor sites (63 sites):**
- `PgStore::new(pool).await?` — 23 sites (1 production at `main.rs:181`, 22 in tests). Signature becomes `async fn new(pool: PgPool) -> sqlx::Result<Self>` (was sync `fn new(pool: PgPool) -> Self`). Encapsulates the watermark seeding query; every existing call site is already inside an async context (`#[tokio::main]` or `#[tokio::test]`) so only `.await?` is added.
- `InMemoryStore::new(clock, completed_ttl, webhook_tx)` — 16 sites (1 production at `main.rs:182`, 15 in tests). Signature changes to absorb `clock`, `completed_ttl`, and a clone of `webhook_tx` (currently these are passed via the `RunStateMachine` plus AppState fields).
- `RunStateMachine::new(...)` — 24 sites all **deleted** (1 production at `main.rs:154` + 23 in tests). Tests that need an in-memory store reach for `InMemoryStore::new` directly via the helpers.

**Total edit count:** 8 handler/main field references + 23 AppState constructors + 23 PgStore sites + 16 InMemoryStore sites + 24 RunStateMachine deletions = **~94 call sites touched**. The bulk is in `tests/integration/`. Mechanical but real; the implementation context should expect this surface, not the prior "8 cited" undercount.

### Test reorganization (per-file classification from investigator)

| File | Test | Class | Disposition |
|------|------|-------|-------------|
| `event_ingestion.rs` | test_create_run_from_requested | PURE | Stay; rewrite to call pure fn |
| `event_ingestion.rs` | test_update_run_to_in_progress | PURE | Stay |
| `event_ingestion.rs` | test_complete_run_with_conclusion | PURE | Stay |
| `event_ingestion.rs` | test_idempotent_requested_twice | PURE | Stay |
| `event_ingestion.rs` | test_create_job_from_queued | PURE | Stay |
| `event_ingestion.rs` | test_update_job_to_in_progress | PURE | Stay |
| `event_ingestion.rs` | test_jobs_by_run | STORE | Move to atc-server (`tests/integration/in_memory_store/indexes.rs`) |
| `event_ingestion.rs` | test_jobs_by_repo | STORE | Move to atc-server |
| `event_ingestion.rs` | test_steps_snapshot_replacement | STORE | Move (verifies `get_job` HashMap state) |
| `event_ingestion.rs` | test_first_sight_completed_job | PURE | Stay |
| `event_ingestion.rs` | test_idempotent_queued_twice | PURE | Stay |
| `eviction.rs` | (all 7 tests) | STORE | Move to atc-server (`in_memory_store/eviction.rs`); rewrite `start_eviction_task` calls to `spawn_eviction_task` |
| `queries.rs` | (all 5 tests) | DEAD | **Delete** — `query_by_repos` is removed and has no production callers |
| `edge_cases.rs` | test_out_of_order_job_before_run | MIXED | Split: pure portion stays as atc-core test on `apply_*_event`; `assert_invariants` calls move to atc-server |
| `edge_cases.rs` | test_out_of_order_completed_before_queued | MIXED | Split (same shape) |
| `edge_cases.rs` | test_duplicate_queued_events | MIXED | Split |
| `edge_cases.rs` | test_duplicate_completed_events | MIXED | Split |
| `edge_cases.rs` | test_unknown_run_id_on_job | MIXED | Split |
| `edge_cases.rs` | test_rapid_status_cycling | MIXED | Split |
| `edge_cases.rs` | test_interleaved_multi_job | MIXED | Split |
| `edge_cases.rs` | test_eviction_with_mixed_state | STORE | Move to atc-server |
| `webhook_domain_updates.rs` | (all 6 tests) | PURE | Stay; rewrite to pure-function call shape |
| `property_tests.rs` | store_invariants_hold | MIXED | Split: forward-only / idempotency / first-sight properties stay (test pure fns on random sequences); index-coupling properties move to atc-server |

**`assert_invariants` split (state_machine.rs:514-598, 8 INDEX + 1 DOMAIN assertions):**
- **DOMAIN** (stays in atc-core): "if conclusion is `Some`, status must be `Completed`" — becomes a property test on `apply_*_event` outputs (proptest: for any sequence of random `JobEvent`s applied via `apply_job_event`, the resulting `Job` has `conclusion.is_some() ↔ status == Completed`).
- **INDEX** (moves to atc-server): 8 assertions covering `jobs_by_repo` / `jobs_by_run` consistency, empty-index cleanup, and active-job presence in both indexes — become `#[cfg(test)] impl InMemoryStore { pub(crate) async fn assert_invariants(&self) { ... } }`.

**AppState test fixtures (`tests/integration/common/mod.rs`):**
- `build_app_with_secret` (line 337) and `build_app_no_secret` (line 370) — both construct `InMemoryStore::new(clock, ttl, webhook_tx.clone())` → `Arc::new` → set on AppState as `persist`. Drop all `state_machine`, `seq`, `pg_pool`, `broadcast_watermark`, `min_pending_seq`, `last_drain_pass_at` assignments. Helpers retain the concrete `Arc<InMemoryStore>` (returned alongside AppState or stored on a fixture struct) so eviction-task tests and assertion tests can call `.evict_expired().await` or `.assert_invariants().await` directly.
- PG fixture (line 622) — construct `PgStore::new(pool).await?`; pull `DrainHandles` from `pg_store.drain_handles()` if the test exercises the drain path. Test sites no longer hardcode `initial_watermark` — the store seeds itself from the test database (which starts with an empty outbox → watermark 0).

**Tests that reach into removed AppState fields (per investigator):**
- `graceful_shutdown.rs:103` — `state.state_machine.start_eviction_task(...)` → use `spawn_eviction_task(in_memory_clone, …)`.
- `outbox_tests.rs:626,843`, `transactional_writes_tests.rs:426,532` — `state.state_machine.snapshot()` → `state.persist.read_snapshot().await?`.
- `webhook_ingestion_tests.rs:594,636`, `row_lock_serialization.rs:134`, `drain_forwards.rs:180`, `persist.rs:1261` — `state.seq.lock().await` / `store.seq.lock().await` reads → capture seq from webhook response, or read `state.persist.read_snapshot().await?.last_seq`.

## Implementation Phases

TDD order. Each phase begins with failing tests and ends with `cargo nextest run -p <crate>` and `cargo clippy -p <crate> -- -D warnings` green for the affected crates.

### Phase 1 — Pure functions in atc-core (TDD foundation)

1. Write failing tests in `atc-core/src/state_machine/tests/pure_application.rs` covering every pure transition behavior: forward-only, idempotent same-status, first-sight creation (from `None`), struct-update merge for partial-field envelopes, snapshot-step replacement on `JobEvent`. Tests assert on the `WorkflowRun` / `Job` value returned by the pure function given specific `Option<existing>` inputs. (Index invariants — `jobs_by_run`, `jobs_by_repo` — are not tested here; they move to InMemoryStore-level tests in Phase 2.)
2. Implement `apply_run_event(Option<WorkflowRun>, RunEventEnvelope) -> Result<WorkflowRun, StateMachineError>` and `apply_job_event(Option<Job>, JobEventEnvelope) -> Result<Job, StateMachineError>` as free functions in `state_machine.rs`. Lift the struct-construction logic out of the current async methods. No state, no async, no tokio.
3. Implement `is_evictable(&Job, DateTime<Utc>, Duration) -> bool` as a free function.
4. **Keep** `RunStateMachine` for now — its async methods delegate to the new free functions internally. This preserves the existing public surface so atc-server compiles.
5. Existing atc-core tests still pass; new tests for the pure functions also pass.

### Phase 2 — InMemoryStore in atc-server uses pure functions; RunStateMachine deleted from atc-core

1. **Write failing tests first.** In `atc-server/tests/integration/in_memory_store/`:
   - `apply_events.rs`: tests `InMemoryStore::apply_run_event` / `apply_job_event` end-to-end (seq increments, broadcast emits, first-sight indexes update). Build a tiny in-process `InMemoryStore` with a stub `broadcast::Sender`.
   - `indexes.rs`: tests `jobs_by_run` and `jobs_by_repo` invariants after random event sequences — relocated from atc-core `event_ingestion.rs` (`test_jobs_by_run`, `test_jobs_by_repo`, `test_steps_snapshot_replacement`).
   - `eviction.rs`: relocated 7 tests from atc-core's eviction.rs; verifies TTL boundary, active-job preservation, orphan-run cleanup, eviction-task cooperative cancel (replaces `start_eviction_task` calls with `spawn_eviction_task`).
   - `eviction_task.rs`: tests for the new free function `spawn_eviction_task` — runs to completion under cancel; absent in PG mode (skipped here; Phase 5 owns the PG-mode-absent assertion).
   - `invariants.rs`: an `#[cfg(test)] impl InMemoryStore { pub(crate) async fn assert_invariants(&self) }` mirroring the 8 INDEX assertions from atc-core's `assert_invariants`; called by `edge_cases`-relocated tests.
   - All tests fail because `InMemoryStore` doesn't exist in its new shape and `spawn_eviction_task` doesn't exist yet.
2. **Restructure `atc-server/src/persist.rs` into the module form** (`persist/mod.rs`, `persist/in_memory.rs`, `persist/pg.rs`, `persist/reads.rs`). Pure mechanical move — no behavior change yet; existing tests still pass.
3. **Relocate `RunnerPoolStats`** from `atc-core/src/state_machine.rs` to `atc-core/src/types.rs`. It has no state-machine coupling — just a `#[derive(TS)]` wire struct derived on the frontend. ts-rs `#[ts(export)]` annotation stays; generated path `frontend/src/lib/types/generated/RunnerPoolStats.ts` is unchanged.
4. **Rewrite `InMemoryStore` in `persist/in_memory.rs`** to own `StateData` (HashMap + indexes) + `RwLock` + `Arc<dyn Clock>` + `completed_ttl` + `Mutex<u64>` for seq + `broadcast::Sender<SeqEvent>` directly. Move `StateData` struct here. Its `apply_*_event` implementations:
   - Lock `seq`
   - Write-lock `state`
   - Clone `existing = state.runs.get(&id).cloned()` (or `state.jobs.get` etc.)
   - Call `atc_core::apply_run_event(existing, env.clone())` / `atc_core::apply_job_event(existing, env.clone())`
   - Insert result; if `existing.is_none()` (first-sight) on a `Job`, update `jobs_by_run` / `jobs_by_repo`
   - Increment seq, broadcast `SeqEvent`, return seq
5. Implement `InMemoryStore::evict_expired` using `atc_core::is_evictable` for the predicate. **Hold a single `state.write().await` lock for the entire sweep** (matching `state_machine.rs:410`): collect expired IDs by iterating `state.jobs.iter()` under the write lock, then remove from `state.jobs`, `state.jobs_by_run`, `state.jobs_by_repo`, and `state.runs` (for orphaned runs) without releasing. Splitting into read-then-write would create a TOCTOU between predicate evaluation and removal.
6. Migrate atc-core test files per the per-file classification table in "Test reorganization":
   - Move STORE tests (eviction.rs, queries-flagged tests, store-coupled portions of edge_cases.rs, MIXED property test's index portions) to `atc-server/tests/integration/in_memory_store/`.
   - Delete `queries.rs` (DEAD — `query_by_repos` and friends are removed).
   - Rewrite PURE tests (event_ingestion.rs's 8 PURE tests, all of webhook_domain_updates.rs, PURE portions of edge_cases.rs) to call `atc_core::apply_run_event(None, env)` / `apply_run_event(Some(existing), env)` directly and assert on the returned value. Tests stay in `atc-core/src/state_machine/tests/`.
   - Add the DOMAIN invariant (`conclusion.is_some() ↔ status == Completed`) as a property test in atc-core.
7. **Add `spawn_eviction_task`** to `atc-server/src/persist/eviction.rs`: `pub fn spawn_eviction_task(store: Arc<InMemoryStore>, interval: Duration, cancel: CancellationToken) -> JoinHandle<()>` following the #60 supervision pattern (biased select on cancel + ticker). At this phase it is spawned **unconditionally** from `main.rs` (same as today's behavior in PG mode — a no-op cost) — Phase 5 makes it conditional. This lands here in Phase 2 because `atc-core::start_eviction_task` is about to be deleted, and the project must compile at the end of every phase.
8. **Delete `RunStateMachine`** (and `StateData`, `QueryResult`, `start_eviction_task`, all `query_*` and `get_*` methods, `snapshot`, `evict_expired`, `assert_invariants`) from `atc-core/state_machine.rs`. Trim `lib.rs` re-exports.
9. **Update all 24 `RunStateMachine::new` call sites** to construct `InMemoryStore::new(...)` directly (or via the updated test helpers). Update `main.rs:225` to call `spawn_eviction_task(in_memory.clone(), Duration::from_secs(60), shutdown.clone())`. The 16 existing `InMemoryStore::new` sites get their signature updated to absorb clock/ttl/webhook_tx parameters.
10. **Drop `tokio` and `tokio-util`** from `backend/crates/atc-core/Cargo.toml` (lands here, after eviction relocates, so atc-core has no remaining tokio consumer).
11. `cargo nextest run -p atc-core` and `cargo nextest run -p atc-server` green; `cargo clippy -- -D warnings` green for both crates.

### Phase 3 — `read_snapshot` + `liveness_check` on the trait

1. **Write failing tests first.** In `atc-server/tests/integration/`:
   - `state_handler_unified.rs`: parametric test that mounts each store impl behind the same router and asserts identical `/v1/state` wire output (modulo seq).
   - `state_handler_pg.rs`: asserts watermark-before-snapshot invariant — under a concurrent insert during the snapshot, `lastSeq` reflects only committed-and-broadcast rows.
   - `state_handler_in_memory.rs`: asserts seq mutex covers snapshot + seq read atomically (no torn read between concurrent writes).
   - `readyz_unified.rs`: asserts `{ok, shutting_down, db_unreachable, drain_stale}` status mapping for each `LivenessError` variant via the trait.
   - All tests fail because the trait methods don't exist yet.
2. Add `StateSnapshot` to `atc-server/src/state.rs` (public) with `#[derive(Serialize, Deserialize, ts_rs::TS)]` + `#[ts(export)]`. Delete the duplicate from `routes.rs`.
3. Add `LivenessError` enum to `atc-server/src/persist/mod.rs`: `DbUnreachable(sqlx::Error)` and `DrainStale { age_ms: i64 }`.
4. Extend `PersistentStore` trait with `read_snapshot` and `liveness_check` methods.
5. Implement `InMemoryStore::read_snapshot` (lock seq → read-lock state → build `StateSnapshot { last_seq, runs, jobs }`) and `InMemoryStore::liveness_check` (return `Ok(())`).
6. Update `PgStore::new` signature: `pub async fn new(pool: PgPool) -> sqlx::Result<Self>` (becomes async; today it is sync at `persist.rs:151`). Internally: runs `SELECT COALESCE(MAX(seq), 0) FROM outbox` against `pool`, allocates `broadcast_watermark = Arc::new(AtomicI64::new(initial_watermark))` and `last_drain_pass_at = Arc::new(AtomicI64::new(now_millis()))`, returns `Ok(Self { ... })`. Add `pub fn drain_handles(&self) -> DrainHandles { watermark, heartbeat }` returning cloned Arcs. Encapsulates outbox-table knowledge inside `PgStore` — `main.rs` and test fixtures no longer mention `outbox` or `MAX(seq)` directly.
7. Implement `PgStore::read_snapshot`: `let watermark = self.broadcast_watermark.load(Acquire)` BEFORE opening the REPEATABLE READ tx (preserves ADR 0002 ordering), then call existing `read_all_runs` / `read_all_jobs`, build `StateSnapshot`.
8. Implement `PgStore::liveness_check`: `SELECT 1` (return `DbUnreachable` on error), then check `self.last_drain_pass_at.load(Relaxed)` age against 30 s threshold (return `DrainStale { age_ms }` on stale).
9. **Update all 23 `PgStore::new` call sites** to add `.await?` (or `.await.unwrap()` in test contexts that already do this for other awaits). All sites are in async contexts already — production at `main.rs:181` is inside `#[tokio::main]`, every test call site is inside `#[tokio::test]`. No site has to switch sync→async; only the await is added.
10. `cargo nextest run -p atc-server` green; failing tests from step 1 now pass.

### Phase 4 — Collapse route handlers + reshape AppState

1. **Write failing tests first.** In `atc-server/tests/integration/app_state_shape.rs`: a compile-time test (e.g., a `#[test] fn appstate_has_exactly_five_public_fields()` using `std::mem::size_of` or a destructure that names all five fields) that fails if any of the removed fields linger. Also: behavioral tests `state_handler_calls_trait` and `readyz_calls_trait` using a mock `PersistentStore` impl that records calls.
2. Rewrite `routes::state_handler` to dispatch through `state.persist.read_snapshot().await`. Map `Err` to 503 + `{"error": "snapshot failed"}`. Source has zero references to `pg_pool`, `state_machine`, `seq`, `broadcast_watermark`.
3. Rewrite `routes::readyz` to: shutdown check → `state.persist.liveness_check().await` → map `Ok` to 200/ok, `Err(DbUnreachable)` to 503/db_unreachable, `Err(DrainStale)` to 503/drain_stale.
4. Drop fields from `AppState`: `state_machine`, `seq`, `pg_pool`, `broadcast_watermark`, `min_pending_seq`, `last_drain_pass_at`. Update field documentation in `state.rs`.
5. Rewire `main.rs`. Boot sequence (PG mode) — `main.rs` no longer queries `outbox` directly; encapsulated inside `PgStore::new`:
   - `let pool = init_pool(url).await?;` (already runs migrations)
   - `let listener_conn = connect_listener(url).await?;` (registers LISTEN — must precede watermark seed so commits during seed get NOTIFY-queued)
   - `let pg_store = Arc::new(PgStore::new(pool.clone()).await?);` (internally seeds watermark from `MAX(outbox.seq)`)
   - `let DrainHandles { watermark, heartbeat } = pg_store.drain_handles();`
   - `let min_pending_seq = Arc::new(AtomicI64::new(i64::MAX));` (preserves today's initial value — "no in-flight handlers at boot" per `state.rs:72`; must be allocated BEFORE `spawn_listener_task` so the listener can register the first observed seq into it)
   - `let listener_handle = spawn_listener_task(listener_conn, min_pending_seq.clone(), shutdown.clone(), ...);`
   - `let drain_handle = spawn_drain_task(pool.clone(), watermark, heartbeat, min_pending_seq.clone(), webhook_tx.clone(), shutdown.clone(), ...);`
   - `let persist: Arc<dyn PersistentStore> = pg_store;`
   - `let app_state = Arc::new(AppState { persist, webhook_tx, webhook_secret, shutdown, ws_tracker });`
   - In-memory mode: skip pool init / listener / drain; construct `InMemoryStore`, keep the concrete `Arc<InMemoryStore>` for the eviction spawn in Phase 5; build AppState with `persist: in_memory.clone() as Arc<dyn _>`.
6. **Update all 23 AppState constructor sites** (1 production + 22 tests). The 3 helpers in `common/mod.rs` shrink to construct the new shape; the remaining 22 inline constructors fold into helper calls where reasonable (a separate cleanup is welcome but not required).
7. Update the 8 test sites that read removed fields per "Tests that reach into removed AppState fields" above.
8. `cargo nextest run -p atc-server` green for both tiers; `just test` green; `cargo clippy -- -D warnings` green.

### Phase 5 — Eviction conditional + shutdown plumbing

(Phase 2 already introduced `spawn_eviction_task`. This phase just makes its spawn conditional on storage mode and tightens the shutdown join chain.)

1. **Write failing tests first.** `atc-server/tests/integration/in_memory_store/eviction_task_absent_in_pg.rs`: a harness that boots the PG-mode path against the test container and asserts no eviction handle is produced (e.g., the orchestrator's eviction slot is `None`). Fails because Phase 2's spawn is unconditional.
2. In `main.rs`, hoist eviction-task creation into the in-memory-mode branch only. PG mode returns `None`. The handle type at the top of `main()` becomes `Option<JoinHandle<()>>`.
3. Update `shutdown.rs` join chain: eviction handle is `Option<JoinHandle<()>>`; the orchestrator awaits only when `Some`. Update the supervision-invariant comment block to enumerate the new conditional case.
4. Confirm `tokio_util` is gone from atc-core's Cargo.toml (Phase 2 already did this; double-check `cargo tree -p atc-core --depth 1`).
5. `cargo nextest run` green; `cargo clippy -- -D warnings` green; `cargo build` green for the workspace; `just test` green.

### Phase 6 — Documentation sweep

Execute every item in the "Documents to Update" section below. In order:

1. Root `CLAUDE.md` — Tech Stack and Project Structure references to `RunStateMachine` and atc-core eviction.
2. `backend/crates/atc-core/CLAUDE.md` — module table (state_machine row → pure functions only), drop `RunStateMachine`/`QueryResult` references, refresh "Last verified".
3. `backend/crates/atc-server/CLAUDE.md` — module table (new `persist/` submodules + `eviction.rs`), AppState field list (5 fields), Storage modes operator guidance (eviction conditional), refresh "Last verified".
4. `docs/architecture/backend-server.md` — Domain Model section (pure functions), State snapshot section (unified `read_snapshot`), AppState shape, `/readyz` contract for `liveness_check`, Storage modes section.
5. `docs/architecture/metrics.md` — update OTel span path references to reflect `persist/{in_memory,pg}.rs` module split.
6. `docs/architecture-decisions/0005-persistentstore-trait-relocation.md` — append a "Subsequent work" note linking this PR.
7. `scripts/doc-mapping.sh` — update the `persist.rs` dual-map entry (today routes to both `backend-server.md` and `metrics.md`) so each new submodule file routes to the same destinations.
8. Run `scripts/check-docs-lefthook.sh` locally to confirm the pre-push doc-staleness gate passes.

## Acceptance Criteria

Numbered for executor check-off:

- **AC1** `GET /v1/state` returns the same JSON shape (`{ lastSeq, runs, jobs }`) regardless of storage mode. `state_handler` source has zero references to `pg_pool`, `state_machine`, `seq`, or `broadcast_watermark`. Failure case: a synthetic test that mounts an `InMemoryStore` and a `PgStore` against the same router asserts the wire output matches byte-for-byte (modulo seq).
- **AC2** `GET /readyz` returns `{status: "ok"}` (200), `{status: "shutting_down"}` (503 when shutdown is cancelled), `{status: "db_unreachable"}` (503 when `SELECT 1` errors), or `{status: "drain_stale"}` (503 when heartbeat age > 30 s). `readyz` source has zero references to `pg_pool` or `last_drain_pass_at`. Failure case: tests injecting each failure mode through PgStore and asserting the status string.
- **AC3** `AppState` definition in `state.rs` lists exactly five public fields: `persist`, `webhook_tx`, `webhook_secret`, `shutdown`, `ws_tracker`. Two greps across **both** `backend/crates/atc-server/src/` **and** `backend/crates/atc-server/tests/` return zero hits:
  - **Dotted access**: `git grep -nE '\.(state_machine|seq|pg_pool|broadcast_watermark|min_pending_seq|last_drain_pass_at)\b' -- 'backend/crates/atc-server/src' 'backend/crates/atc-server/tests'` (catches `state.seq`, `app_state.broadcast_watermark`, etc.).
  - **Struct-literal positions**: `git grep -nE '^\s*(state_machine|seq|pg_pool|broadcast_watermark|min_pending_seq|last_drain_pass_at):' -- 'backend/crates/atc-server/src' 'backend/crates/atc-server/tests'` (catches `AppState { state_machine: ..., seq: ..., ... }` initializers — investigator found 22 of these in test fixtures alone).
  - Matches inside `InMemoryStore` or `PgStore` impl bodies that happen to name a private field of the same name (e.g., `self.seq`) are allowed — those are internal. Restrict both greps with `--not -e '/persist/'` if needed, or audit the matches manually. Failure case: any test or src module still reaches the removed fields via either pattern.
- **AC4** `backend/crates/atc-core/Cargo.toml` does not list `tokio` or `tokio-util` under `[dependencies]` or `[dev-dependencies]`. `cargo tree -p atc-core --depth 1` shows neither. Failure case: either crate appears.
- **AC5** `atc-core::state_machine` exports exactly: `StateMachineError`, `apply_run_event`, `apply_job_event`, `is_evictable`. `RunStateMachine`, `QueryResult`, `RunnerPoolStats`, `start_eviction_task`, and the `query_*`/`get_*`/`jobs_for_*`/`snapshot`/`evict_expired` methods are not present. `RunnerPoolStats` is re-located to `atc-core/src/types.rs` (no state-machine coupling). `git grep -nE 'RunStateMachine|QueryResult|start_eviction_task' -- 'backend/**/*.rs'` returns zero hits.
- **AC6** `InMemoryStore` in `atc-server::persist::in_memory` owns `StateData` (the HashMap + indexes), `seq: Mutex<u64>`, and uses `atc_core::apply_*_event` for transition logic. `git grep "RunStateMachine" -- 'backend/**/*.rs'` returns zero hits. (Design plans under `docs/design-plans/` retain historical references; that's expected and out of scope per `feedback_phases_not_in_user_facing_strings.md`.) Failure case: any production-source reference to the removed type.
- **AC7** `PgStore` exposes `drain_handles() -> DrainHandles` returning the broadcast watermark and drain heartbeat. `main.rs` consumes those at spawn time. The drain task continues to advance the watermark after successful drain passes and refresh the heartbeat at the top of every loop iteration and after every successful pass (heartbeat-only wakes refresh the heartbeat but do not advance the watermark — current semantics preserved). Memory ordering: drain writes via `Release`; `PgStore::read_snapshot` reads watermark via `Acquire` before opening the REPEATABLE READ transaction. Failure case: drain task can't reach the atomics, AppState still has them, or the ordering relaxes from Release/Acquire on the watermark path.
- **AC8** `spawn_eviction_task` in `atc-server::persist::eviction` is a free function returning `JoinHandle<()>`. `main.rs` spawns it **conditionally** on in-memory mode. PG-mode startup produces no eviction-task handle. Failure case: eviction task spawned in PG mode (visible via tokio-console or test scaffolding), or `start_eviction_task` survives anywhere.
- **AC9** OTel spans `persist.apply.run_event`, `persist.apply.job_event`, `persist.notify.emit` still fire from `PersistentStore` impl methods. In-memory exporter tests assert their presence after a `apply_run_event` round-trip in each mode.
- **AC10** End-to-end smoke: dev server (`just dev`) in in-memory mode accepts a webhook, broadcasts via WS, returns the event on `/v1/state`. PG mode (against the test container) does the same. Both paths are exercised in CI.
- **AC11** Behavioral test coverage migrated cleanly. After the move: (a) `cargo nextest run -p atc-core` is green and contains test functions covering every pure-transition behavior (forward-only, idempotent same-status, first-sight, struct-update merge, snapshot-step replacement) plus domain invariants (status/conclusion consistency); (b) `cargo nextest run -p atc-server` is green and contains test functions covering every store/index/eviction behavior previously exercised in atc-core (jobs_by_run index consistency, jobs_by_repo index consistency, eviction TTL boundary, eviction preserves active jobs, eviction removes orphaned runs, snapshot atomicity); (c) `cargo clippy -- -D warnings` is green for both crates; (d) `just test` (full verification per `feedback_use_just_test_or_nextest.md`) is green; (e) no test exists in `atc-core` that depends on HashMap/index state, and no test exists in `atc-server` that depends on transition-rule purity (clean separation). Test counts are incidental and not load-bearing.
- **AC12** Documentation: both `CLAUDE.md` files refreshed (date stamp + module table + AppState shape), backend-server.md State snapshot section updated, ADR cross-references intact, `scripts/check-docs-lefthook.sh` passes pre-push.

## Documents to Update

- **Root `CLAUDE.md`** (`/CLAUDE.md`) — "Tech Stack" section currently says `atc-core` owns the `RunStateMachine` and TTL eviction. Update to reflect: atc-core owns domain types, pure transition functions, and the eviction predicate; atc-server owns `InMemoryStore` and the eviction task. Refresh "Last verified" date.
- `backend/crates/atc-core/CLAUDE.md` — module table (state_machine row → pure functions only; no more eviction/RunStateMachine row), "Last verified" date, contracts section (drop RunStateMachine references, keep transition rules). Update the `cargo test -p atc-core` count to reflect the new (smaller) suite.
- `backend/crates/atc-core/AGENTS.md` — symlinked; auto-tracks.
- `backend/crates/atc-server/CLAUDE.md` — module table (persist/ now a module with `in_memory.rs`, `pg.rs`, `reads.rs`, `eviction.rs`), AppState field list (5 fields), Storage modes section (eviction now conditional on in-memory mode), Contracts list (`liveness_check` semantics), Testing section, "Last verified" date.
- `backend/crates/atc-server/AGENTS.md` — symlinked; auto-tracks.
- `docs/architecture/backend-server.md` — Domain Model section (pure functions in atc-core), State snapshot section (unified read path through trait), AppState shape, Storage modes operator guidance (eviction spawn conditional + behavior of `liveness_check`), the contract list for `/readyz`.
- `docs/architecture/metrics.md` — verify the OTel boundary-span inventory still cites the right module paths (`persist::PgStore::apply_*_event`, `persist::InMemoryStore::apply_*_event`); after the persist module split, the path becomes `persist::pg::PgStore::apply_*_event` etc. Update inline doc references.
- `docs/architecture-decisions/0005-persistentstore-trait-relocation.md` — append a "Subsequent work" note linking to this PR (read-path unify + read/liveness methods + state-machine purification).
- `scripts/doc-mapping.sh` — today `backend/crates/atc-server/src/persist.rs` has a special dual-map to both `backend-server.md` and `metrics.md` (see `doc-mapping.sh:39` and `doc-mapping.sh:57`). After splitting into `persist/in_memory.rs`, `persist/pg.rs`, `persist/reads.rs`, `persist/eviction.rs`, `persist/mod.rs`: update the existing mapping so each new file routes to the same dual destination (or replace with a directory-level mapping `backend/crates/atc-server/src/persist/*.rs` if the script supports glob). Also: verify `state_machine.rs` mapping still aligned with the new pure-function scope.

## Implementation Guidance

These project rules apply to this plan (cited from `docs/implementation-guidance.md` and `MEMORY.md`):

- **Rule 1 (feature branch)** — implementation continues on the design plan's branch. PR title = full deliverable (e.g., `refactor(backend): pure state machine in atc-core, all persistence in atc-server`). Test plan in PR's first comment, not the body.
- **Rule 2 (TDD)** — Phase 1 starts with failing tests for the pure functions. Each subsequent phase writes failing tests before the migration step.
- **Rule 4 (doc-mapping)** — no new architecture docs in this plan, but the `state_machine.rs` mapping description may need refresh.
- **Rule 7 (Rust test file split)** — atc-core's `state_machine/tests/` directory is already split by concern (event_ingestion, eviction, edge_cases, etc.). The eviction and queries submodules move to atc-server alongside the new InMemoryStore; the remaining submodules stay focused on pure-function tests.
- **Rule 14 (subagents)** — implementation context dispatches subagents per phase. Phase 1 + 2 are atc-core-focused (one investigator/implementer set); Phases 3-5 are atc-server-focused.
- **Rule 17 (planning-artifact labels)** — strip phase/AC numbers from test descriptions, comments, and architecture docs. Behavior description suffices.
- **`feedback_cow_semantics.md`** — the new `InMemoryStore::apply_*_event` uses the existing remove-then-insert (CoW) pattern, matching ADR 0002's StateStore semantics. Frontend RunStore is unaffected (no frontend changes).
- **`feedback_use_just_test_or_nextest.md`** — run `cargo nextest run -p <crate>` for the dev loop; `just test` for full verification before commit.
- **`feedback_codex_review_before_exit.md`** — this plan goes through codex `xhigh` review before ExitPlanMode (next step in this session).
- **`feedback_fix_class_not_instance.md`** — when removing `state.state_machine`, `state.seq`, etc., scrub **all** 44 cited call sites in one pass — don't leave stragglers for the reviewer.
- **`feedback_no_source_grep_tests.md`** — AC3 uses `git grep` for source assertions, not as a runtime test. The behavioral assertions on storage-mode-uniform output (AC1, AC2) are what tests verify.

## Out of Scope

- Frontend changes — `/v1/state` and `SeqEvent` wire shapes do not change. Generated ts-rs files may differ in path comments but not in content.
- New persistence backends.
- Snapshot caching, pagination, or any change to read-path semantics beyond the routing layer.
- Outbox / NOTIFY / drain protocol changes — ADR 0002 / 0003 invariants carry forward unchanged.
- Multi-replica behavior changes.
- `min_pending_seq` redesign — it stays as a `main.rs`-local Arc plumbed into listener and drain.
- `RunnerPoolStats` migration — stays in atc-core as a wire-type struct with no methods.
- **Listener / drain relocation into `persist/`** — `backend/crates/atc-server/src/listener.rs` stays at the top level. The `spawn_listener_task` and `spawn_drain_task` free functions stay where they are. PgStore exposes the watermark/heartbeat Arcs via `drain_handles()` but does NOT own listener/drain lifecycle. The full move (listener+drain become PgStore-internal, lifecycle owned by `PgStore::start` / `PgStore::shutdown`, supervision orchestration rewired through PgStore) is a separate initiative to be filed as its own issue. It's a bigger change than this PR can absorb and diverges from #60's free-function supervisor pattern.

## Glossary

- **DrainHandles** — accessor return type on `PgStore` exposing `watermark: Arc<AtomicI64>` and `heartbeat: Arc<AtomicI64>` so the drain task can advance them. Replaces today's AppState-shared Arcs.
- **LivenessError** — error type returned by `PersistentStore::liveness_check`. Variants: `DbUnreachable(sqlx::Error)` and `DrainStale { age_ms: i64 }`. Distinct from `PersistError` (which is read/write-shaped).
- **First-sight** — domain term for an event whose entity ID is not yet present in the store; an entity is created on the spot. Out-of-order webhook tolerance (jobs may arrive before runs). The pure `apply_*_event(None, env)` path implements this.
