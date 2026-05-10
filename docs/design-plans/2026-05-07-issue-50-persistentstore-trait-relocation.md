# Issue #50 — Relocate `PersistentStore` Trait to atc-server with Two Backends

> Implementation context: read `docs/implementation-guidance.md` before writing code.

## Context

Issue #50 (`design`/`server`, OPEN) asks for a Phase 5 cleanup decision: the `atc_core::PersistentStore` trait was introduced in Phase 2b as a production seam (`Arc<dyn PersistentStore>` mounted on `AppState`); Phase 2c moved the production write path onto `sqlx::Transaction` via `pub(crate)` free functions, and the trait field was dropped from `AppState`. Since then, the trait has lived only in tests, with the production path branching in `routes.rs` between a transactional PG path and an inline in-memory path.

The issue framed three options: keep as test seam, extend the trait for transactions, or delete. All three rest on a load-bearing assumption — that a transaction-mediating trait method must accept `&mut Transaction` as an argument, which conflicts with `Arc<dyn Trait>`'s `&self` access. That assumption is avoidable. **An implementation can own its transaction internally**: the trait method takes `&self` plus a domain envelope and returns the allocated seq; the impl decides whether durability is "open transaction, UPSERT + outbox + notify, commit" or "lock seq, apply to in-memory store, broadcast." With this shape, the trait is genuinely useful again — it unifies the two backends behind one method dispatch.

The remaining question was the trait's location. `atc-core` should not host the trait because the in-memory backend's broadcast emission requires `tokio::broadcast::Sender<SeqEvent>`, a server-level type. So the trait and **both** impls relocate to `atc-server::persist`. Atc-core keeps `StateStore` as the pure domain state machine with inherent `apply_run_event` / `apply_job_event` methods; the server layer wraps `StateStore` in an `InMemoryStore` adapter that holds the broadcast sender and seq mutex. `PgStore` stays as the durable backend, refactored to internalize its own transaction lifecycle. `AppState` carries `Arc<dyn PersistentStore>` again; the route handler stops branching on storage mode and dispatches uniformly through the trait.

A companion research artifact (separate PR) explores **what an additional state backend could look like**, with the explicit framing that the three load-bearing contracts (atomic state+log, push notification, replayable monotonic log) do not have to be satisfied by a single store. Composed architectures — state in store A, log in store B, notify in store C — are valid candidates and the doc should treat them as first-class peers to single-store candidates.

## Definition of Done

1. `PersistentStore` trait relocated to `atc-server::persist` with two impls (`PgStore`, `InMemoryStore`).
2. `PgStore` refactored so its trait methods own their own `sqlx::Transaction` lifecycle (open → UPSERT + outbox INSERT + `pg_notify` → commit → return allocated seq).
3. `InMemoryStore` adapter introduced — holds `Arc<StateStore>` + `Arc<Mutex<u64>>` + `broadcast::Sender<SeqEvent>`; trait methods lock seq, apply to StateStore, broadcast, return seq.
4. `AppState` swapped: `persist: Arc<dyn PersistentStore>` field replaces `pg_pool`-driven branching in the webhook handler. (`pg_pool: Option<PgPool>` stays for non-persist consumers — drain task, listener task, `/readyz` probe, `/v1/state` snapshot.)
5. Route handler webhook ingestion collapses both branches to a single `state.persist.apply_*_event(env).await?` call returning seq.
6. atc-core's `impl PersistentStore for StateStore` deleted; trait deleted from atc-core; atc-core retains `PersistError` + `RunEventEnvelope` + `JobEventEnvelope` as domain types.
7. Tests migrate to drive the trait through `Arc<PgStore>` / `Arc<InMemoryStore>`; outbox-row side-effects in the migrated PG tests asserted ignored or harmlessly co-existent.
8. New ADR records the decision; `docs/architecture/backend-server.md` updated as canonical architecture doc.
9. Issue #50 closed.
10. (Separate PR) Research doc at `docs/architecture/state-externalization-research/additional-backends.md` covering decomposed contract framing, single-store candidates, composed/multi-store candidates, anti-patterns, and "when to switch."

## Locked Decisions

The following are settled inputs to this plan, not open questions:

- **Trait stays, relocated to atc-server.** `atc-core` is the pure domain layer; the trait belongs adjacent to its impls, both of which sit at the server-concern layer (broadcast emission, transaction lifecycle).
- **Single trait, two impls.** `PgStore` (existing struct, refactored) and `InMemoryStore` (new adapter struct). Both implement `async fn apply_run_event(&self, env) -> Result<u64, PersistError>` and `async fn apply_job_event(&self, env) -> Result<u64, PersistError>`.
- **Implementation owns its transaction.** Trait method signatures do not include `&mut Transaction`; PgStore opens, drives, and commits its own transaction internally.
- **`StateStore` is renamed to `RunStateMachine`.** The current name is misleading — the type does not "store" anything in the durability sense; it is an in-memory state machine that applies events and emits domain transitions. Rename ships in this PR. **Module layout (verified):** the type lives in a single file `backend/crates/atc-core/src/store.rs` with sibling test modules in `backend/crates/atc-core/src/store/` (declared as `mod tests;` and `mod property_tests;`). Both rename: `store.rs` → `state_machine.rs` and `store/` → `state_machine/`. `StoreError` renames to `StateMachineError`. **`AppState.store` → `AppState.state_machine` is required, not discretionary** — it's load-bearing in `routes.rs:~179` (`/v1/state` in-memory snapshot read) and `state.rs:15`. Other field renames (`InMemoryStore.state_machine`, locals) follow the same convention.
- **`RunStateMachine` stays in atc-core.** It is the domain state machine. After the trait impl is removed, its `apply_run_event` / `apply_job_event` remain as inherent methods.
- **`pg_pool` stays on `AppState`.** Non-persist consumers (drain task, listener task, `/v1/state` snapshot reader, `/readyz` probe) keep direct pool access. The trait dispatches the persistence write path; the pool serves everything else.
- **Decision recorded as a new ADR.** Per user direction. Neighboring update to `docs/architecture/backend-server.md` reflects the architecture but does not carry the rationale alone.
- **PG tests stay on the trait.** `tests/persist_pg_tests.rs` (16 tests) keeps the `PgStore::new(pool).apply_run_event(env)` interface; assertions are unchanged. The refactored impl now writes outbox rows + emits NOTIFY as side effects, which the tests do not currently inspect — verify no test counts outbox rows. The `pg_store_ping_succeeds` test stays.
- **Wire response unifies on success.** Both modes return `{"status":"accepted","seq":<u64>}` after the refactor (was: `accepted` for PG, `processed` for in-memory). GitHub webhook senders ignore the body; this is a low-risk wire change.
- **Invalid transitions return 200, not 422.** Both modes today return 200 OK on parity rejections (PG mode: `routes.rs:~374`; in-memory mode: `routes.rs:~405`); broadcast is not emitted on rejection. The trait dispatch must preserve this. The route handler matches `PersistError::InvalidTransition` explicitly and returns `(StatusCode::OK, Json(json!({"status":"rejected"})))` — **NOT** mapped through `?`/`map_err` to a 4xx. Only `PersistError::Backend(_)` (or equivalent transient-PG variant) maps to 503.
- **Seq type is `u64` end-to-end.** `SeqEvent.seq` is `u64` today; `AppState.seq: Mutex<u64>` today; `lastSeq` over the wire is a JSON number. The trait method returns `Result<u64, PersistError>`. PG impl converts `BIGSERIAL` (`i64` from sqlx) → `u64` at the impl boundary; the conversion is infallible for non-negative values (BIGSERIAL is always positive).
- **`AppState.seq` becomes `Arc<Mutex<u64>>`.** Today's bare `Mutex<u64>` is not cloneable, but `InMemoryStore` needs to hold a reference. Wrap in `Arc`. The route handler stops touching it directly; only `InMemoryStore` does. (See Architecture § "AppState changes" for the full diff.)

## Architecture

### Target shape

```
backend/crates/atc-core/
  src/persist.rs                  — PersistError                                   ✅ kept
                                  — pub trait PersistentStore                     ❌ deleted (relocated)
                                  — impl PersistentStore for StateStore           ❌ deleted
                                  — trait_delegation_* tests                      ❌ deleted
  src/event/                      — RunEventEnvelope, JobEventEnvelope            ✅ kept (location unchanged)
  src/store/  → state_machine/                                                    ↻ directory renamed
              StateStore  → RunStateMachine                                       ↻ type renamed (apply_* methods kept inherent)
              StoreError  → StateMachineError                                     ↻ type renamed
  src/lib.rs:21                   — pub use persist::PersistentStore              ❌ removed
  src/lib.rs                      — pub use store::StateStore                     ↻ replaced with `pub use state_machine::RunStateMachine`

backend/crates/atc-server/
  src/persist.rs       — pub trait PersistentStore                                ➕ new (relocated from core)
                       — pub struct PgStore + impl PersistentStore                ✅ kept, ↻ refactored
                         (now opens own tx; uses upsert_*_in_txn + outbox helpers internally)
                       — pub struct InMemoryStore + impl PersistentStore          ➕ new
                         (holds Arc<RunStateMachine> + Arc<Mutex<u64>> + broadcast::Sender;
                          locks → apply → broadcast)
                       — pub(crate) upsert_run_in_txn, upsert_job_in_txn          ✅ kept (now PgStore-internal)
                       — pub(crate) insert_outbox_*_in_txn,                       ✅ kept
                                    notify_outbox_seq_in_txn
                       — pub(crate) read_all_runs, read_all_jobs                  ✅ kept
                       — _assert_pg_store_impls_trait                             ❌ deleted (trait check inherent in impl)
  src/state.rs         — AppState fields                                          ↻ + persist: Arc<dyn PersistentStore>
                                                                                  (pg_pool: Option<PgPool> stays)
  src/main.rs          — startup wiring                                           ↻ build PgStore or InMemoryStore;
                                                                                  wrap in Arc; mount on AppState
  src/routes.rs        — webhook handler in-memory branch (~30 lines)             ↻ collapsed to one call
                       — webhook handler PG branch                                ↻ collapsed to one call
                                                                                  (both go through state.persist)
  tests/persist_pg_tests.rs                                                       ✅ kept, no migration
                                                                                  (PgStore interface unchanged from caller's view)
```

### Trait + impl signatures

```rust
// backend/crates/atc-server/src/persist.rs

#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    /// Apply a run event durably and return the allocated seq.
    /// PG impl: opens tx → UPSERT + outbox INSERT + NOTIFY → commit. Drain task broadcasts.
    /// In-memory impl: locks seq → applies to RunStateMachine → broadcasts → returns seq.
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError>;
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError>;
}

pub struct PgStore { pool: PgPool }

#[async_trait::async_trait]
impl PersistentStore for PgStore {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut tx = self.pool.begin().await
            .map_err(|e| PersistError::Backend(Box::new(e)))?;
        upsert_run_in_txn(&mut tx, env.clone()).await?;
        let seq_i64 = insert_outbox_run_in_txn(&mut tx, &env).await?;
        notify_outbox_seq_in_txn(&mut tx, seq_i64).await?;
        tx.commit().await
            .map_err(|e| PersistError::Backend(Box::new(e)))?;
        // BIGSERIAL is always positive; conversion is infallible.
        Ok(u64::try_from(seq_i64).expect("BIGSERIAL is non-negative"))
    }
    // apply_job_event analogous
}

pub struct InMemoryStore {
    state_machine: Arc<RunStateMachine>,       // renamed from StateStore (see Locked Decisions)
    seq: Arc<Mutex<u64>>,                      // shared with AppState; wrapped in Arc for adapter ownership
    broadcast_tx: broadcast::Sender<SeqEvent>,
}

#[async_trait::async_trait]
impl PersistentStore for InMemoryStore {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError> {
        let mut guard = self.seq.lock().await;
        self.state_machine.apply_run_event(env.clone()).await?;
        // Auto-conversion via `impl From<StateMachineError> for PersistError`
        // (atc-core/src/persist.rs:19, post-Phase-2 rename of StoreError).
        *guard += 1;
        let allocated = *guard;
        let _ = self.broadcast_tx.send(SeqEvent { seq: allocated, event: WebhookEvent::Run(env) });
        Ok(allocated)
    }
    // apply_job_event analogous
}
```

**Note on `PersistError`.** The existing `PersistError` enum in atc-core has `InvalidTransition` and a backend-error variant (verify exact name during implementation — likely `Backend(Box<dyn Error>)` or similar). Do **not** add an `sqlx`-aware constructor on `PersistError` in atc-core (that would drag `sqlx` into the domain layer). The atc-server-side `PgStore` impl wraps sqlx errors via `Box::new` into the existing variant.

### AppState changes

```rust
// backend/crates/atc-server/src/state.rs — diff
 pub struct AppState {
-    pub store: Arc<StateStore>,
+    pub state_machine: Arc<RunStateMachine>,   // renamed (see Locked Decisions); held for /v1/state, eviction, etc.
     pub webhook_tx: broadcast::Sender<SeqEvent>,
     pub webhook_secret: Option<String>,
-    pub seq: Mutex<u64>,                       // route handler increments directly
+    pub seq: Arc<Mutex<u64>>,                  // shared with InMemoryStore adapter
     pub pg_pool: Option<PgPool>,
     pub min_pending_seq: Arc<AtomicI64>,
     pub last_drain_pass_at: Arc<AtomicI64>,
+    pub persist: Arc<dyn PersistentStore>,     // new — write-path dispatch
 }
```

Builder sweep: every call site that constructs `AppState` (production `main.rs`, test fixtures in `tests/common/mod.rs::build_app_*`) must wrap the seq in `Arc::new(Mutex::new(0))` and construct the right `persist` impl. The implementing context enumerates these via `rg "AppState \{|AppState::new" backend/`.

### Route handler simplification

Before (today, ~140 lines):

```rust
if let Some(pool) = &state.pg_pool {
    // ~50 lines: begin tx, upsert, outbox insert, notify, commit, return accepted+seq
} else {
    // ~30 lines: lock seq, apply, increment, broadcast, return processed
}
```

After:

```rust
match state.persist.apply_run_event(env).await {
    Ok(seq) => (StatusCode::OK, Json(json!({"status":"accepted","seq": seq}))).into_response(),
    Err(PersistError::InvalidTransition) => {
        // Preserve current behavior: parity rejection is observable as 200,
        // not a 4xx. No broadcast is emitted. (Same as today's PG and in-memory paths.)
        // PersistError::InvalidTransition is a unit variant (atc-core/src/persist.rs:14).
        (StatusCode::OK, Json(json!({"status":"rejected"}))).into_response()
    }
    Err(PersistError::Backend(e)) => {
        // Transient PG (or backend) failure → 503. Drain heartbeat + retry handle recovery.
        tracing::error!(error = %e, "persistence write failed");
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":"error"}))).into_response()
    }
}
```

The dispatch logic moves from "ifs in the route handler" to "polymorphism in the trait impl"; **error mapping is explicit, not `?`-propagated**, so invalid transitions stay on the 200 contract today's clients depend on. The job branch is structurally identical.

### Why `pg_pool` stays on AppState

The trait covers the **persistence write path**. Other consumers of the pool are not write-path:

| Consumer | How it accesses the pool today |
|---|---|
| Drain task (`listener::spawn_drain_task`) | Receives a startup-time clone of `PgPool` at task spawn, **not** via `AppState.pg_pool` at runtime. |
| Listener task (`listener::spawn_listener_task`) | Same — startup-time clone; holds its own `PgListener` for `LISTEN atc_outbox`. |
| `/v1/state` handler | Reads `state.pg_pool` from `AppState` directly; opens REPEATABLE READ transaction. **Active runtime user of the field.** |
| `/readyz` probe | Reads `last_drain_pass_at` atomic; does not use the pool. |
| Metrics health-checkers | Some Phase 5 metrics emit pool stats; verify call sites. |

The runtime AppState consumer is `/v1/state`. Listener and drain tasks need the field to exist at startup (cloning happens in `main.rs`) but do not re-read it from AppState at runtime. **Conclusion: `pg_pool: Option<PgPool>` stays on AppState** — only the webhook write path moves to `persist: Arc<dyn PersistentStore>`.

### Atc-core dependency check after relocation

- `async-trait` is no longer needed in atc-core after the trait moves. **Verify before deletion** — if `RunStateMachine`'s inherent methods don't need the macro (they shouldn't; native `async fn` is fine for non-`dyn` inherent methods), drop the dep from `backend/crates/atc-core/Cargo.toml`.
- `async-trait` **stays in atc-server**. Native async-fn-in-traits does not yet support `dyn Trait` dispatch, so `Arc<dyn PersistentStore>` requires the `#[async_trait]` macro on both the trait and impls. Do not remove from `backend/crates/atc-server/Cargo.toml`.
- `PersistError` stays in atc-core as the domain error returned by `RunStateMachine` inherent methods. The atc-server trait method propagates it. **Do not** add an `sqlx`-aware constructor on `PersistError` (would drag `sqlx` into the domain layer); wrap sqlx errors via the existing `Backend` variant inside the atc-server `PgStore` impl.
- `RunEventEnvelope` / `JobEventEnvelope` stay in atc-core as domain types (in the event module — `backend/crates/atc-core/src/event/`, **not** `persist.rs`).

## Implementation Phases

Phases are TDD-ordered. Each phase ends with `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` green. (Phase 2 in particular requires workspace-wide tests because the rename touches both crates.)

### Phase 1 — Test the InMemoryStore contract AND the new wire-response shape

Write failing tests for two surfaces simultaneously, since the wire change and the trait dispatch are interlocking:

**(a) `InMemoryStore` direct unit tests** — at `backend/crates/atc-server/src/persist.rs` under `#[cfg(test)] mod inmem_tests`. Coverage:

- Seq monotonicity: first call returns 1, second returns 2, after 100 mixed calls returns 100.
- Broadcast emission on success: subscriber on `webhook_tx` receives one `SeqEvent` per successful call; `seq` field matches return value.
- **Invalid-transition behavior**: `Completed → InProgress` returns `Err(PersistError::InvalidTransition)` (unit variant — `atc-core/src/persist.rs:14`); broadcast is **not** emitted; seq is **not** incremented.
- Concurrency: two `apply_run_event` calls from different tasks produce sequential, non-interleaved seqs.

**(b) Route-handler wire-response tests** — added to `webhook_ingestion_tests.rs` (or a new test file if cleaner). Coverage:

- Successful in-memory ingestion → `200 OK` + `{"status":"accepted","seq":<u64>}`.
- Successful PG ingestion → same shape.
- Invalid-transition rejection (both modes) → `200 OK` + `{"status":"rejected"}`. **No** 4xx.
- Backend error (force a transient PG fault, e.g., closed pool) → `503 SERVICE_UNAVAILABLE`.

Both test surfaces fail at this phase (types/handlers don't exist yet). They lock the contract before implementation.

### Phase 2 — Rename `StateStore` → `RunStateMachine` (mechanical, repo-wide)

Pure mechanical rename, isolated to its own phase so the surface area is auditable and the trait-relocation phase that follows reads against the renamed type. No behavior change; `cargo test --workspace` stays green throughout.

1. **Rename type**: `StateStore` → `RunStateMachine` everywhere.
2. **Rename file + sibling directory**: `backend/crates/atc-core/src/store.rs` → `backend/crates/atc-core/src/state_machine.rs` (the type definition lives in the file at root, not in a `mod.rs`); and the sibling test directory `backend/crates/atc-core/src/store/` (containing `tests.rs`/`tests/` and `property_tests.rs`, declared via `mod tests;` and `mod property_tests;` from inside `store.rs:~596`) → `backend/crates/atc-core/src/state_machine/`. All `mod store` / `use crate::store::*` references update accordingly. `lib.rs` `pub use store::StateStore` becomes `pub use state_machine::RunStateMachine`.
3. **Rename error type**: `StoreError` → `StateMachineError`. The existing `impl From<StoreError> for PersistError` (`atc-core/src/persist.rs:19`) becomes `impl From<StateMachineError> for PersistError`. The auto-conversion via `?` is the production mapping mechanism — no separate named mapper exists today.
4. **Rename instance fields** holding the type. Recommended (implementing context can refine):
   - `AppState.store` → `AppState.state_machine`
   - Local `let store = ...` → `let state_machine = ...`
   - Test fixtures' `store: ...` field → `state_machine: ...`
5. **Update `implementation-guidance.md` rule 7 reference**: the path `backend/crates/atc-core/src/store/tests/` becomes `backend/crates/atc-core/src/state_machine/tests/`. The rule still names this as the reference pattern for split test files; update the path.
6. **Verify**: `cargo test --workspace` green after each grep+replace pass. `cargo clippy --workspace -- -D warnings` clean. No frontend impact (ts-rs-generated types use struct names like `WorkflowRun`, not `StateStore`).

Enumeration command (live source only — exempts preserved-history docs): `rg -w 'StateStore|StoreError' backend/ docs/architecture/backend-server.md docs/architecture-decisions/ CLAUDE.md backend/crates/*/CLAUDE.md`. **Do NOT** sweep `docs/architecture/state-externalization-research/`, `docs/implementation-plans/`, or `docs/design-plans/` — those are historical artifacts that document the pre-rename world and must be preserved as-is. Live-doc rename targets enumerated in the Documents to Update table.

### Phase 3 — Implement trait, PgStore refactor, InMemoryStore

Single phase because the changes are interlocking:

1. **Move trait declaration** from `atc-core::persist` to `atc-server::persist`. Refactor signatures to return `Result<u64, PersistError>`.
2. **Refactor `impl PersistentStore for PgStore`**: each method opens its own `sqlx::Transaction`, calls the existing `upsert_*_in_txn` + `insert_outbox_*_in_txn` + `notify_outbox_seq_in_txn` helpers, commits, returns seq (converted from BIGSERIAL `i64` to `u64` at this boundary). The existing helpers stay `pub(crate)` (now consumed by `PgStore` rather than directly by the route handler).
3. **Implement `InMemoryStore`** struct + `impl PersistentStore for InMemoryStore` per the signatures above. The impl uses the existing `impl From<StateMachineError> for PersistError` (post-rename of `StoreError`; see `atc-core/src/persist.rs:19`) — typically just `?` after `apply_run_event(...)` since `From` auto-conversion is the idiomatic mechanism. The match for invalid-transition uses the **unit variant** `PersistError::InvalidTransition` (no `{ .. }` pattern — `persist.rs:14`). Phase 1 tests pass.
4. **Delete** `_assert_pg_store_impls_trait` (the impl block itself proves the trait is satisfied).
5. **Delete** `impl PersistentStore for RunStateMachine` from atc-core (renamed in Phase 2).
6. **Delete** the trait declaration from atc-core; remove the `pub use` at `atc-core/src/lib.rs:21`.
7. **Delete or migrate trait-dependent tests in atc-core** — `trait_delegation_apply_run_event_ok`, `trait_delegation_apply_job_event_ok`, and `from_store_error_maps_to_invalid_transition` (verify at `persist.rs:133`; if the test depends on `dyn PersistentStore` it must be deleted, since the trait no longer exists in atc-core; if it tests the `From` impl directly, migrate to a non-trait test). The `From<StateMachineError> for PersistError` impl itself stays (it's the production mapping mechanism).

### Phase 4 — Wire AppState, route handler, AND migrate response-body assertions

This phase changes the response shape; all tests asserting the old shape must update in the **same** phase to keep `cargo test` green throughout.

1. **AppState**: add `pub persist: Arc<dyn PersistentStore>`; change `seq: Mutex<u64>` → `seq: Arc<Mutex<u64>>`. Keep `pg_pool: Option<PgPool>`.
2. **Builder sweep**: every site that constructs `AppState` (`main.rs`, `tests/common/mod.rs::build_app_*`) wraps the seq in `Arc::new(Mutex::new(0))` and constructs the right `persist` impl. Enumerate via `rg "AppState \{|AppState::new" backend/`.
3. **Startup wiring** in `main.rs`: after pool init (or skip), construct the right impl:
   ```rust
   let persist: Arc<dyn PersistentStore> = match pg_pool.clone() {
       Some(pool) => Arc::new(PgStore::new(pool)),
       None => Arc::new(InMemoryStore::new(store.clone(), seq.clone(), webhook_tx.clone())),
   };
   ```
4. **Route handler** in `routes.rs`: replace both webhook-mode branches with the explicit-match dispatch from Architecture § "Route handler simplification" (NOT a `?`-propagation). The match preserves 200 for `InvalidTransition` and maps `Backend(_)` → 503.
5. **Move metric emit points into `PgStore`**: today `atc_pg_write_failures_total` (`routes.rs:304, 357`) and `atc_pg_notify_emitted_total` (`metrics.rs:127`) are emitted from the route handler. After the refactor, both move into `PgStore::apply_*_event` (the success/failure branches around the `tx.commit()` and `notify_outbox_seq_in_txn` calls). Add a unit test in the `inmem_tests` / `pg_tests` modules that verifies counter increment on the matching code path.
6. **Migrate response-body assertions** — the **explicit file list is authoritative**; the grep is a sanity check (broad to catch both literal-string and structured assertions):
   - `backend/crates/atc-server/tests/webhook_hmac_tests.rs:11` (and `:38`)
   - `backend/crates/atc-server/tests/webhook_ingestion_tests.rs:21` (and `:45`)
   - `backend/crates/atc-server/tests/phase_3c_state_pg_read.rs:248`
   - Sanity grep: `rg -n '"processed"|status.*processed|processed.*status' backend/crates/atc-server/tests/` — review each hit and update.

   Updated assertions: success → `{"status":"accepted","seq":<u64>}`; invalid-transition → `{"status":"rejected"}` with `200 OK`.
7. **Test pass**: full `cargo test -p atc-server` green. Observable behavior changes ONLY in:
   - Response body literal: `"processed"` → `"accepted"` for in-memory success; new `seq` field included.
   - Invalid-transition body literal: any prior `"rejected"`-style body → unified `{"status":"rejected"}`. **HTTP status stays 200.**
   - Other observable behavior (broadcast emission, state mutation, seq monotonicity, drain pipeline) is unchanged.

### Phase 5 — Test reconciliation (PG path side effects + import surfaces)

Phase 4 changes the response contract; this phase reconciles deeper test surfaces.

1. **`persist_pg_tests.rs` import-surface update** — the file imports `PersistentStore` from `atc_core` today (`tests/persist_pg_tests.rs:12`). Switch to `use atc_server::persist::PersistentStore;` (or trait-method names without trait import — verify with the implementing context). **Assertion bodies do not change**, but the import line must.
2. **`persist_pg_tests.rs` side-effect verification** (16 tests) — `PgStore::apply_*_event` now writes an outbox row + emits NOTIFY in addition to the UPSERT. Verify that **no test counts outbox rows, asserts an empty outbox table, or asserts NOTIFY silence on a listener**. If any do, fix them or migrate them. The grep is `rg "outbox|pg_notify|FROM outbox" backend/crates/atc-server/tests/persist_pg_tests.rs`.
3. **In-memory test surface** — most existing in-memory tests dispatch through the webhook route, which now calls `state.persist.apply_run_event`. Observable behavior other than the response body literal (covered in Phase 4) is unchanged. Verify all green.
4. **Mock or stub trait** — none introduced. Tests use real `PgStore` or real `InMemoryStore`. No `mockall` adoption.

### Phase 6 — Documentation, ADR, and CLAUDE.md updates

Create new ADR (next sequential number — verify against `docs/architecture-decisions/` listing):
- File: `docs/architecture-decisions/{N}-persistence-trait-relocation.md`
- Status: Accepted
- Sections: **Context** (Phase 2b introduction → Phase 2c divergence → trait became vestigial → re-evaluation surfaced that internal-transaction-ownership unblocks the trait again) | **Decision** (relocate trait to atc-server, two impls, route handler dispatches uniformly, response contract unifies on `accepted`/`rejected` while preserving 200 status for invalid transitions) | **Consequences** (atc-core public API shrinks; both backends behind one interface; future backend research framed in the separate doc — link).

**ADR annotation sweep** (per `docs/planning-workflow.md` § ADR Annotation Sweep): annotate every doc, test, and code comment that argues the **opposite** position the new ADR supersedes. Inline annotation format:

```markdown
> **Revised by ADR-{N}:** Trait relocated to atc-server with both backends behind one interface; original "test-only seam vs. extend vs. delete" framing superseded. See `docs/architecture-decisions/{N}-persistence-trait-relocation.md`.
```

Specific surfaces to annotate (codex-cited; expand if grep finds more):
- `docs/architecture/state-externalization-research/README.md:21` — still lists #50 as open. Update to closed and link the new ADR.
- `docs/design-plans/2026-05-04-phase-2c-outbox.md:67-80, 234-235` — Phase 2c plan D2 section argues a different trait fate. Annotate inline.
- Any code comments referencing the old shape — `rg "Phase 2b|Phase 2c trait|trait fate|PersistentStore (test|seam)" backend/ docs/`.

Update `docs/architecture/backend-server.md`:
- § "Storage modes — operator guidance": no change to the dev-only framing.
- § "Modules" `persist.rs` row: rewrite to describe trait + two impls + private helpers.
- § "Files" `persist.rs` entry: same.
- § "Contracts": update "Webhook ingestion" entries — both bullets now say "dispatch through `state.persist`."
- **`StateStore` → `RunStateMachine` rename sweep** in this doc — codex cited live references at `docs/architecture/backend-server.md:65` and `:386`. Update both, plus any `store` field mention. Run `rg -n 'StateStore|store_error|StoreError' docs/architecture/backend-server.md` and update every match.
- Search the doc for any other `PersistentStore` / `PgStore` mention; update.

Update `CLAUDE.md` (project root):
- Line 2 "Last verified" header — update date; remove `#50 still open for PersistentStore trait cleanup`.
- Line ~46 "Remaining Phase 5 follow-ups" — drop #50 from the list.
- **`StateStore` → `RunStateMachine` rename sweep** — codex cited references at `CLAUDE.md:17` and `:47`. Update both. Run `rg -n 'StateStore|StoreError' CLAUDE.md` and update every match.

Update `backend/crates/atc-core/CLAUDE.md`:
- Drop "Phase 2b adds PersistentStore trait" sentence.
- Persist module table row: rewrite. `PersistentStore` no longer lives here. `apply_run_event` / `apply_job_event` are inherent methods on `RunStateMachine` returning `PersistError`.
- Drop "used by `PgStore`" note from the `predecessors_of()` description (now used internally by the in-txn helpers, which are atc-server private).
- **`StateStore` → `RunStateMachine` rename sweep** in this doc — run `rg -n 'StateStore|StoreError' backend/crates/atc-core/CLAUDE.md` and update every match. Module table row description shifts from "store" to "state_machine".

Update `backend/crates/atc-server/CLAUDE.md`:
- Persist module row: rewrite to describe trait declaration + `PgStore` + `InMemoryStore` + private helpers.
- "PG access" subsection: drop `PgStore is no longer mounted in AppState` line. AppState now mounts `Arc<dyn PersistentStore>`.
- "Webhook ingestion" contracts: rewrite both bullets to dispatch through trait.
- **`StateStore` → `RunStateMachine` rename sweep** — `state` module description, `AppState.store` field reference, contracts referencing the in-memory store. Run `rg -n 'StateStore|StoreError|\bstore\b' backend/crates/atc-server/CLAUDE.md` (review `\bstore\b` matches manually — many will be field-name references that rename).
- Verify both `CLAUDE.md` and `AGENTS.md` symlinks present.

Run `scripts/check-docs-lefthook.sh` locally — verify staleness gate satisfied. Existing mappings already point `backend/crates/atc-*/src/*` at `backend-server.md`; no new mapping needed.

Update `docs/architecture/state-externalization-research/rollout-and-implementation.md`:
- Phase 5 section: mark #50 closure complete.

Close issue #50 in the merge commit (`Closes #50` in PR body).

### Phase 7 (Separate PR) — Research doc: alternative state backends

Independent PR. Title: `docs(architecture): research alternative state backends`.

Create `docs/architecture/state-externalization-research/additional-backends.md`. Required framing per user feedback: **the three load-bearing contracts do not have to be satisfied by a single store.** Decomposition is a first-class architectural option.

Sections:

1. **Three load-bearing contracts** — atomic state-update + log-append; push notification of new log entries; replayable monotonic log. Explicit framing that any future architecture must satisfy all three but they may decompose across multiple stores.

2. **Decomposition framing** — what does it mean to satisfy each contract independently? Cross-store atomicity options:
   - Outbox pattern across distributed stores.
   - CDC tools (Debezium, Materialize) bridging state-store → log-store.
   - Event-sourcing-first (log-as-primary, state-as-derivation) shapes.
   - Saga / 2PC with idempotency for cross-store transactional contracts.

3. **Single-store candidates** —
   - **CockroachDB**: PG-wire-compat; sqlx mostly drop-in; CHANGEFEEDS replace LISTEN/NOTIFY; lowest migration cost.
   - **NATS JetStream + KV bucket**: stream as durable log, KV as state, push consumers as drain. Inverts the layering (log-as-primary). Single binary.
   - **DynamoDB + Streams**: table as state, Stream as log, Lambda/ECS task as drain. AWS lock-in.
   - **FoundationDB**: versionstamps + watches give native ordered-log + push semantics. Operational complexity is real.

4. **Composed / multi-store candidates** —
   - **PG state + Kafka log + NATS notify** — three-way decomposition. Cross-store atomicity via outbox-from-PG-to-Kafka (Debezium or hand-rolled) + saga compensation.
   - **CockroachDB state+log + NATS notify** — Cockroach's CHANGEFEED feeds NATS; NATS is the only push primitive consumers see.
   - **DynamoDB + Streams + EventBridge** — AWS-native composition, less operational lift inside AWS.
   - **EventStoreDB primary + projection-derived state** — flips the architecture: log is canonical, state is derived projection.

5. **Anti-patterns / poor fit** — plain Redis pub/sub (no durable transactional outbox); plain S3 (no notify, no transactional outbox); Cassandra/Scylla CDC (latency-unfriendly for live broadcast). Cite reasons.

6. **CockroachDB drop-in case study** — what changes and what doesn't if PG → Cockroach: schema (mostly identical), sqlx (mostly works), LISTEN/NOTIFY → CHANGEFEED (drain task rewrites), `pg_notify` emit (replaced with row-level CHANGEFEED).

7. **NATS JetStream inverted-layering case study** — log as primary, state as KV derivation, drain-equivalent is push consumer.

8. **PG-state + Kafka-log composed case study** — what cross-store atomicity looks like; Debezium considerations; the tax on operational lift.

9. **Diagrams** — current PG architecture (Mermaid sequence: webhook → outbox+notify → drain → broadcast); CockroachDB variant; NATS variant (inverted); PG+Kafka+NATS variant (decomposed).

10. **"When to switch" framing** — operational conditions that would motivate moving off PG (scale ceiling, multi-region, cost, self-host friction). Modern guidance: stay on PG until you hit a specific limit, name the limit, then revisit.

11. **Recommendation** — PG fits ATC's current scale and operational footprint. Doc is a forward-looking artifact for the day a switch is contemplated, not a roadmap item.

Reference this doc from the new ADR (Phase 6) and from `docs/architecture/state-externalization-research/rollout-and-implementation.md`.

**Research material**: an internet-researcher run is in flight (dispatched 2026-05-07) with current backend-capability information. Incorporate findings as primary sources for the doc.

## Acceptance Criteria

| AC | Verification |
|---|---|
| **AC1** Trait relocated | `rg "trait PersistentStore" backend/crates/atc-server/src/` returns matches; `rg "trait PersistentStore" backend/crates/atc-core/src/` returns zero. |
| **AC2** PgStore refactored to internalize tx | `PgStore::apply_run_event` body opens `pool.begin()`, calls upsert+outbox+notify helpers, commits, returns seq. |
| **AC3** InMemoryStore exists and impls trait | `pub struct InMemoryStore` exists in atc-server::persist; `impl PersistentStore for InMemoryStore` exists; struct fields match the signature in this plan. |
| **AC4** AppState carries trait | `state.rs::AppState` has `pub persist: Arc<dyn PersistentStore>`. |
| **AC5** Route handler dispatches via trait | Webhook handler in `routes.rs` calls `state.persist.apply_*_event(env)` once per event kind; no `if let Some(pool) = &state.pg_pool` branching for the apply path. |
| **AC6** atc-core no longer exports the trait | `rg "PersistentStore" backend/crates/atc-core/` returns zero matches in `src/`. |
| **AC7** atc-core trait_delegation tests deleted | `rg "trait_delegation" backend/crates/atc-core/` returns zero. |
| **AC8** Migrated tests pass | `cargo test -p atc-server` reports all green, including the 16 `persist_pg_tests` (now exercising the full transactional path). |
| **AC9** Workspace clean | `cargo check --workspace`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace` all pass. |
| **AC10** ADR exists | `docs/architecture-decisions/{N}-persistence-trait-relocation.md` exists with Status: Accepted. |
| **AC11** Architecture doc updated | `docs/architecture/backend-server.md` describes the trait, two impls, and the unified route-handler dispatch; no claims of `PgStore is not mounted in AppState`. |
| **AC12** CLAUDE.md updated | Root, atc-core, atc-server CLAUDE.md files all reflect the new shape. "#50 still open" removed from root. |
| **AC13** Wire response unified (success) | Both modes return `200 OK` with `{"status":"accepted","seq":<u64>}`. **Body assertions updated in:** `webhook_hmac_tests.rs:11`, `webhook_ingestion_tests.rs:21`, `phase_3c_state_pg_read.rs:248`, plus any additional matches from `rg '"status":"processed"' backend/`. |
| **AC13b** Invalid transition preserves 200 | Both modes return `200 OK` + `{"status":"rejected"}` on `PersistError::InvalidTransition`. **Not** a 4xx. New tests in `webhook_ingestion_tests.rs` assert this for both modes; broadcast not emitted. |
| **AC13c** Backend errors map to 503 | `PersistError::Backend(_)` (or equivalent) in PG mode returns `503 SERVICE_UNAVAILABLE`. Existing 503 tests stay green. |
| **AC13d** Metrics ownership preserved | `atc_pg_write_failures_total` and `atc_pg_notify_emitted_total` continue to increment from the PG write path (now inside `PgStore`). Unit test asserts increment. |
| **AC13e** ADR annotation sweep complete | `docs/architecture/state-externalization-research/README.md:21` updated; `docs/design-plans/2026-05-04-phase-2c-outbox.md` annotated with `> Revised by ADR-{N}:` markers per § ADR Annotation Sweep. |
| **AC14** Issue closed | Issue #50 closed via merge commit referencing it. |
| **AC15** (Separate PR) Research doc exists | `docs/architecture/state-externalization-research/additional-backends.md` exists with the eleven sections specified in Phase 7, including decomposition framing and composed-architecture case studies. |
| **AC16** No source-grep tests added | New tests assert behavior, not source content (per `feedback_no_source_grep_tests.md`). |
| **AC17** No "Phase N" / "Per ADR XXX" in user-facing strings | Per `feedback_phases_not_in_user_facing_strings.md`. ADR text and design-plan body are dev-facing and exempt. |
| **AC18** Planning-workflow doc tightened | `docs/planning-workflow.md` Phase 1 line ~25 strengthened so system-prompt overrides do not supersede the project's agent-preference order; Phase 5.5 opening makes the planning Claude the explicit actor. |
| **AC19** Implementation-guidance doc tightened | `docs/implementation-guidance.md` adds rule 16 naming `ed3d-research-agents:*` agents in preference order with `Explore` as fallback; rule 7's path reference updated to `state_machine/tests/`. |
| **AC20** `StateStore` renamed to `RunStateMachine` | `rg -w 'StateStore|StoreError' backend/ docs/architecture/backend-server.md docs/architecture-decisions/ CLAUDE.md backend/crates/*/CLAUDE.md` returns zero matches. Preserved-history docs under `docs/architecture/state-externalization-research/`, `docs/implementation-plans/`, and `docs/design-plans/` are NOT swept — they are pre-rename historical artifacts. Directory/file `backend/crates/atc-core/src/state_machine.rs` exists; `backend/crates/atc-core/src/state_machine/` exists with sibling test modules. `cargo test --workspace` passes. |

## Documents to Update

| Document | Change |
|---|---|
| `docs/architecture-decisions/{N}-persistence-trait-relocation.md` | **NEW.** ADR documenting the relocation decision. |
| `docs/architecture/backend-server.md` | Update Modules / Files / Contracts to reflect trait + two impls + unified dispatch. **Plus `StateStore` → `RunStateMachine` rename sweep** — codex cited live references at `:65` and `:386`; run `rg -n 'StateStore\|StoreError' docs/architecture/backend-server.md` and update every match. |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Mark #50 closure complete. |
| `docs/architecture/state-externalization-research/README.md` (line 21) | Update #50 status from open to closed; link the new ADR. |
| `docs/design-plans/2026-05-04-phase-2c-outbox.md` (lines 67-80, 234-235) | Annotate D2 trait-fate section inline with `> Revised by ADR-{N}:` marker. |
| `docs/architecture/state-externalization-research/additional-backends.md` | **NEW (separate PR).** Research doc on decomposed and composed backend architectures. |
| `CLAUDE.md` (root) | Update "Last verified"; remove `#50 still open`; drop #50 from remaining-follow-ups list. **Plus `StateStore` → `RunStateMachine` rename sweep** — codex cited live references at `:17` and `:47`. |
| `backend/crates/atc-core/CLAUDE.md` | Remove trait references; document inherent `apply_*` methods on `RunStateMachine` (renamed); drop "used by `PgStore`" note. **Plus `StateStore` → `RunStateMachine` and module-name `store` → `state_machine` rename sweep.** |
| `backend/crates/atc-core/AGENTS.md` | Symlink — no separate edit. |
| `backend/crates/atc-server/CLAUDE.md` | Document trait + two impls + private helpers; rewrite ingestion contracts. **Plus `StateStore` → `RunStateMachine` rename sweep** in field references and module descriptions. |
| `backend/crates/atc-server/AGENTS.md` | Symlink — no separate edit. |
| `scripts/doc-mapping.sh` | No change needed. Existing `backend/crates/atc-*/src/*` → `backend-server.md` mapping covers all source edits. |
| `docs/planning-workflow.md` | Two tightening edits bundled into this PR. (1) Phase 1 § "Use researcher subagents for codebase investigation" — strengthen wording so a system-prompt override that suggests a different agent type does NOT supersede the project's preference order; the planning-workflow agent preference is authoritative when this document is invoked. (2) Phase 5.5 opening — rewrite "Before handing off, run two gates against the plan file" to make the actor explicit: "Before exiting plan mode and handing off to the user, **the planning Claude** runs two gates against the plan file." |
| `docs/implementation-guidance.md` | Two edits bundled. (1) Add new rule 16: "Use project-specific researcher agents for investigation" — names the four `ed3d-research-agents:*` agents in preference order, falls back to `Explore` only when project-specific agents are unavailable, and notes that system-prompt overrides do not supersede this preference when this guidance document is invoked. (2) Update rule 7's path reference from `backend/crates/atc-core/src/store/tests/` to `backend/crates/atc-core/src/state_machine/tests/` (post-rename). |

## Implementation Guidance

Read `docs/implementation-guidance.md` first. Beyond that:

- **Use research agents for codebase + external research.** Project-specific researchers (`ed3d-research-agents:codebase-investigator`, `internet-researcher`, `combined-researcher`, `remote-code-researcher`) before falling back to `Explore`. The system-reminder constraint about "only use Explore" is a plan-mode artifact and does not bind implementation.
- **`feedback_pr_title_convention.md`** — PR title reflects full deliverable. Suggested: `refactor(server): relocate PersistentStore trait, rename StateStore, unify webhook write path (#50)`.
- **`feedback_pr_body_convention.md`** — PR body is the squash commit body. Write as "what will be implemented" at design; update to past tense at completion.
- **`feedback_test_plans.md`** — Test plan goes in PR's first comment, **not** committed.
- **`feedback_dont_skip_runtime_verification.md`** — Run `just test` and `just lint` locally before pushing; fix root causes, never silently skip.
- **`feedback_verify_lefthook_installed.md`** — Run `just setup` at session start.
- **`feedback_no_source_grep_tests.md`** — Tests assert behavior, not source content.
- **`feedback_phases_not_in_user_facing_strings.md`** — Strip "Phase N" / "Per ADR XXX" from runtime logs, error messages, and operator-facing docs. ADR/design-plan body is dev-facing and exempt.
- **`feedback_plans_in_repo_no_review_artifacts.md`** — When this plan is committed to `docs/design-plans/`, it should read as final intent; strip any "previous draft" / "blocker N resolved" / decision-history phrasing.
- **`feedback_dont_reflex_tail.md`** — Don't pipe `cargo test` output through `head` / `tail`.
- **`feedback_atc_user_persona.md`** — No UX surface in scope.
- **`feedback_run_e2e_tests_for_frontend_changes.md`** — Not applicable; backend-only.
- **`feedback_cow_semantics.md`** — Backend `StateStore` uses remove-then-insert intentionally; if any in-memory store wrapper does delegating mutation, do not "optimize" to in-place update.

Codex review (Phase 5.5 of the planning workflow): this plan is multi-file, ADR-coupled, and operational-surface-adjacent (touches the webhook-ingestion contract path and AppState shape). Codex `xhigh` review run before plan handoff.

## Out of Scope

| Item | Owner |
|---|---|
| Relocating `RunStateMachine` (renamed from `StateStore`) to `atc-server` | It is the domain state machine, stays in atc-core. |
| Changes to PG schema, drain task, listener task, NOTIFY pipeline | None planned. |
| Changes to `SeqEvent` / `StateSnapshot` wire types | None — Phase 3a/3b shape preserved. (The status-string in the response body changes from `processed` to `accepted` for in-memory mode; that's a JSON literal shift, not a type change.) |
| Frontend changes | Out of scope. |
| Performance tuning, metric additions | Out of scope. |
| Mock-trait adoption (`mockall` etc.) | Out of scope. Tests use real impls. |
| Outbox retention design | Issue #67. |
| Raw webhook audit table | Issue #65. |
| Dashboard ConfigMap | Issue #64. |
| Legacy-metric doc backfill | Issue #66. |
| Switching to a non-PG state backend | Research doc only (Phase 7 separate PR). No implementation. |
| Unifying the `/v1/state` read path through the trait | **Issue #69** (filed during this planning session). The route handler still branches on `pg_pool.is_some()` for the read path; surfaced as a layer-violation smell during round-2 codex review. Tracked as a follow-up because (a) it's a meaningful scope expansion (~50-100 lines + AppState restructuring + test fixture diff) and (b) the proposed shape (third trait method `read_snapshot`) needs exploration — see open question 6 on #69 (is the right answer even one trait with multiple concerns, or split into `PersistentWrite` + `SnapshotProvider`, or something else?). Cannot land before #50 — relies on the trait being in atc-server first. |

## Glossary

- **Outbox** — append-only `outbox` table written transactionally with state UPSERTs; consumed by drain task.
- **Drain task** — background task in PG mode that reads outbox rows in seq order, broadcasts `SeqEvent`, and advances `broadcast_watermark`.
- **`broadcast_watermark`** — drain's commit-order cursor; read by `/v1/state` as `lastSeq`.
- **In-memory mode** — `ATC_DATABASE_URL` unset; dev-only; webhook handler dispatches to `InMemoryStore` which applies to `RunStateMachine` and broadcasts directly under the seq mutex.
- **`RunStateMachine`** — atc-core's domain state machine (renamed from `StateStore` in this PR; see Locked Decisions). In-memory map of runs and jobs that applies events to derive state transitions; not a persistence store.
- **Predicated UPSERT** — `INSERT ... ON CONFLICT DO UPDATE ... WHERE runs.status = ANY($preds)`; zero rows affected maps to `PersistError::InvalidTransition`.
- **Trait dispatch** — `Arc<dyn PersistentStore>` on `AppState`; route handler calls trait method without knowing which backend is mounted.
