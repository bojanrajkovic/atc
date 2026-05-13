# 0005 — Relocate `PersistentStore` trait to atc-server with two backends

**Status:** Accepted (issue #50, 2026-05-07)

Last verified: 2026-05-07

## Context

Phase 2b introduced a `PersistentStore` trait in `atc-core` with a single production impl:
`impl PersistentStore for RunStateMachine`. `AppState` held `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` to enable durable shadow writes alongside the in-memory path.

Phase 2c changed the webhook write path to own and drive its own `sqlx::Transaction`. The Phase 2c plan (D2, `docs/design-plans/2026-05-04-phase-2c-outbox.md:67-80`) concluded that `Arc<dyn PersistentStore>` was incompatible with the new path: `&self` from a trait object cannot yield the `&mut Transaction<'_, Postgres>` that sqlx requires for executor binding. The field was dropped from `AppState`, and the trait survived only in `tests/persist_pg_tests.rs` as a test seam.

This left the webhook handler with two diverging code paths: ~50 lines branching on `pg_pool.is_some()` for the PG path (open tx → upsert → outbox → notify → commit) versus ~30 lines for the in-memory path (lock seq → apply to RunStateMachine → broadcast → return "processed"). The two paths shared no code, had diverging response bodies (`"accepted"` vs `"processed"`), and were increasingly difficult to test uniformly.

Issue #50 revisited the Phase 2c assumption. The load-bearing claim — that the trait method must accept `&mut Transaction` — is false if the implementation owns its transaction internally. `PgStore::apply_run_event(&self, env)` can call `self.pool.begin()`, drive the transaction, and commit entirely within the method body. `Arc<dyn PersistentStore>` with `&self` access is fully compatible with this shape. The trait is useful again.

The trait's location also needed to change. `atc-core` is the pure domain layer and has no business holding a trait whose in-memory impl requires `tokio::sync::broadcast::Sender<SeqEvent>` — a server-level type. Both impls belong adjacent to the server concerns they embed.

## Decision

### 1. Relocate `PersistentStore` trait to `atc-server::persist`

The trait moves from `atc-core::persist` to `atc-server::persist` and is removed from `atc-core`'s public API. The move co-locates the trait with both its impls and the private helpers they call.

Trait signature:

```rust
#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    async fn apply_run_event(&self, env: RunEventEnvelope) -> Result<u64, PersistError>;
    async fn apply_job_event(&self, env: JobEventEnvelope) -> Result<u64, PersistError>;
}
```

`#[async_trait]` is required for `Arc<dyn PersistentStore>` dyn dispatch; native async-fn-in-traits does not yet support object safety in stable Rust.

`PersistError` stays in `atc-core` as a domain error type shared by the `RunStateMachine` inherent methods and the trait impls.

### 2. Two impls, each owning its own transaction lifecycle

**`PgStore`** (existing struct, refactored): each trait method calls `self.pool.begin()`, runs `upsert_*_in_txn` + `insert_outbox_*_in_txn` + `notify_outbox_seq_in_txn` in the open transaction, commits, and returns the allocated `BIGSERIAL` seq converted to `u64`. The existing `pub(crate)` helpers are consumed by `PgStore` rather than directly by the route handler.

**`InMemoryStore`** (new adapter): holds `Arc<RunStateMachine>` + `Arc<Mutex<u64>>` (shared with `AppState`) + `broadcast::Sender<SeqEvent>`. Each trait method acquires the seq mutex, applies to the state machine, increments the counter, broadcasts a `SeqEvent`, and returns the allocated seq. The mutex is held across the full pipeline to preserve the ordering invariant (seq values are strictly monotonically increasing and their assignment order matches the in-memory apply order).

Both impls emit `PersistError::InvalidTransition` on rejected transitions and `PersistError::Backend(_)` on infrastructure failure.

### 3. `AppState` carries `Arc<dyn PersistentStore>` for the write path

`AppState` gains `pub persist: Arc<dyn PersistentStore>` as the webhook write-path dispatch point. `pg_pool: Option<PgPool>` is retained for non-persist consumers — drain task, listener task, `/v1/state` snapshot reader, `/readyz` probe. `seq: Mutex<u64>` becomes `Arc<Mutex<u64>>` so `InMemoryStore` can hold a shared reference.

> **Revised by ADR-0006:** Each store now owns the background tasks that read its state and emit broadcast events (listener + drain for `PgStore`; eviction for `InMemoryStore`). The trait gains `subscribe()` and `shutdown()`, `AppState` drops `webhook_tx`, and WS handlers obtain their receiver via `state.persist.subscribe()`. `main.rs` no longer plumbs Arcs or spawns the per-store tasks; it constructs the store via `PgStore::start` / `InMemoryStore::start` and hands the resulting `Arc<dyn PersistentStore>` to the shutdown orchestrator, which calls `persist.shutdown()` once.

`main.rs` constructs the right impl at startup:

```rust
let persist: Arc<dyn PersistentStore> = match pg_pool.clone() {
    Some(pool) => Arc::new(PgStore::new(pool)),
    None => Arc::new(InMemoryStore::new(state_machine.clone(), seq.clone(), webhook_tx.clone())),
};
```

### 4. Route handler dispatches uniformly through the trait

The webhook handler's two-branch body collapses to a single trait dispatch:

```rust
let persist_result = match &event {
    WebhookEvent::Run(env) => state.persist.apply_run_event(env.clone()).await,
    WebhookEvent::Job(env) => state.persist.apply_job_event(env.clone()).await,
};
match persist_result {
    Ok(seq) => (StatusCode::OK, Json(json!({"status":"accepted","seq": seq}))),
    Err(PersistError::InvalidTransition) => (StatusCode::OK, Json(json!({"status":"rejected"}))),
    Err(PersistError::Backend(e)) => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status":"error"}))),
}
```

`PersistError::InvalidTransition` preserves the existing 200 OK contract for parity rejections — it is not propagated through `?` to a 4xx.

### 5. Response body unifies on `"accepted"` for success

Both modes now return `{"status":"accepted","seq":<u64>}` on success. The in-memory mode previously returned `{"status":"processed"}` — a divergence with no semantic justification once the write path is unified. GitHub webhook senders ignore the response body; the change is low-risk. Invalid transitions return `{"status":"rejected"}` (unchanged from the PG path). HTTP status stays 200 for both accepted and rejected.

### 6. Metric emit points move into `PgStore`

`atc_pg_write_failures_total` and `atc_pg_notify_emitted_total` previously emitted from the route handler. They now emit from within `PgStore::apply_*_event`, co-located with the code paths they measure. `atc_pg_notify_emitted_total` emits after `tx.commit()` because PG only delivers queued NOTIFYs on commit; aborted transactions silently drop NOTIFYs, so emitting before commit would overcount.

## Consequences

**Positive:**

- Route handler is ~80 lines shorter. The storage-mode branch is gone; error handling is uniform.
- `AppState` no longer branches on `pg_pool.is_some()` for the write path. (Follow-up issue #69 removed the read-path branching too and dropped `pg_pool` from `AppState` entirely; `PgStore` now owns the pool internally.)
- `atc-core`'s public API shrinks: the trait and its `RunStateMachine` impl are removed. `atc-core` returns to being a pure domain model crate.
- Both backends are exercised through the same test surface; `webhook_ingestion_tests.rs` and `webhook_hmac_tests.rs` now cover both modes without mode-specific assertions.
- The `"accepted"` + seq response provides useful observability for webhook senders and tests without new infrastructure.

**Neutral:**

- `async-trait` stays in `atc-server`; it moves out of `atc-core` (no longer needed there).
- `tests/persist_pg_tests.rs` continues to test `PgStore` directly through the trait, unchanged in behavior (outbox rows and NOTIFYs are now side effects of `apply_*_event`, verified in separate `transactional_writes_tests.rs` tests).
- ~~The `/v1/state` read path still branches on `pg_pool.is_some()`~~ — resolved by issue #69 (PR #103): `read_snapshot` and `liveness_check` joined the `PersistentStore` trait, route handlers became storage-mode-uniform, and atc-core was reduced to pure transition functions.

**Negative (accepted trade-offs):**

- Wire response change (`"processed"` → `"accepted"`) is observable to any consumer that inspects the response body. GitHub's webhook delivery mechanism does not inspect the body; the change is low-risk for ATC's actual consumers.
- The `Arc<Mutex<u64>>` wrapping adds one indirection level for the seq counter. This is negligible in the context of network I/O.

## Supersedes

- Phase 2c plan D2 (`docs/design-plans/2026-05-04-phase-2c-outbox.md:67-80`): "the webhook handler cannot use `Arc<dyn PersistentStore>` — `&self` cannot yield `&mut Transaction`." The impl can own its transaction, making this constraint moot.
- Phase 2c plan D2 (`docs/design-plans/2026-05-04-phase-2c-outbox.md:234-235`): issue #50 as open question. Issue closed by this PR.

## See Also

- Issue [#50](https://github.com/bojanrajkovic/atc/issues/50) — Reconcile `PersistentStore` trait with transactional outbox
- Issue [#69](https://github.com/bojanrajkovic/atc/issues/69) — Resolved by PR #103: unified `/v1/state` and `/readyz` read paths through `read_snapshot` / `liveness_check` trait methods; atc-core reduced to pure transition functions; all persistence concerns moved to `atc-server::persist`.
- `docs/design-plans/2026-05-07-issue-50-persistentstore-trait-relocation.md` — Full implementation plan with all acceptance criteria
- Forward-looking research on decomposed and composed alternative backends — Phase 7 of the implementation plan; deferred to a separate PR.
