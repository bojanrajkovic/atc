# 0006 — Stores own their background-task lifecycle and event emission

**Status:** Accepted (issue #104, 2026-05-13)

Last verified: 2026-05-15

> **Revised by ADR-0008:** Geographic claims in this ADR (the trait lives in `atc-server::persist`; `PgStore::start` spawns listener and drain from inside `atc-server`; the per-task `SHUTDOWN_TIMEOUT_*` constants are atc-server-internal) are superseded by the four-crate split. The lifecycle ownership reasoning (each store owns its background tasks and exposes one `shutdown()` join point) is preserved — only the source locations move: `PersistentStore` to `atc-persist`, `PgStore` machinery to `atc-store-pg`, `InMemoryStore` machinery to `atc-store-mem`, and the per-store shutdown timeouts move into the store crates that consume them. See `docs/architecture-decisions/0008-persistence-crate-split.md` (introduced in issue #169 phase 1).

## Context

ADR-0005 relocated `PersistentStore` to `atc-server::persist` and made each
backend own its write path. Issue #69 followed by purifying `atc-core` and
moving every read path through `state.persist`. Both refactors stopped short
of the same treatment for the *background tasks* that read each store's
internal state and emit broadcast events.

After issue #69, `PgStore` owned its pool and watermark/heartbeat atomics,
but the listener and drain tasks that used those atomics were still spawned
as free functions from `main.rs` via `listener::spawn_listener_task` /
`listener::spawn_drain_task`. The eviction task for `InMemoryStore` was the
same shape: a free `spawn_eviction_task` invoked at startup with an
`Arc<InMemoryStore>` and a cancellation token. The `webhook_tx` broadcast
channel that fanned out `SeqEvent`s lived on `AppState` and was constructed
in `main.rs`.

That layering leaked store-internal mechanics across the abstraction
boundary:

- `main.rs` plumbed 11 `Arc<_>` parameters into `spawn_drain_task`.
- Both stores received the same `broadcast::Sender<SeqEvent>` at
  construction; the channel itself was a `main.rs` concern.
- `AppState` held a `webhook_tx: broadcast::Sender<SeqEvent>` field that
  every WS handler subscribed against.
- `run_shutdown_orchestration` took three separate per-task handles
  (`drain_handle`, `listener_handle`, `eviction_handle`), each `Option`-typed
  to encode the mode dispatch.
- Tests that needed to abort the drain mid-pass held a `JoinHandle<()>`
  directly and called `abort()` on it; the store had no way to know its own
  task had been killed externally.

The result was that the storage-mode dispatch reappeared in `main.rs`
(~110 lines branching on `pg_pool.is_some()`), in `AppState`'s field list
(carrying `webhook_tx` purely for WS subscribe), and in
`run_shutdown_orchestration` (three per-mode handle params).

## Decision

### 1. Each store owns its background tasks

`PgStore::start(pool, listener_conn, shutdown)` constructs the store *and*
spawns both the listener and the drain tasks. The `JoinHandle`s are stored
on the returned `Arc<PgStore>` inside an internal
`std::sync::Mutex<Option<PgStoreHandles>>`. The startup path runs `SELECT
MAX(seq) FROM outbox` before spawning so the seed query is the last
fallible operation in the function; a guardrail comment documents that any
fallible step added after the spawns MUST cancel and join the
already-spawned tasks before returning `Err`.

`InMemoryStore::start(clock, ttl, eviction_period, shutdown)` similarly
constructs the store and spawns the eviction task. Synchronous — no
`.await` at the call site, because the store has no fallible startup work.

### 2. The broadcast sender is store-internal

Each store constructs its own `broadcast::channel(256)` in `start()` and
holds the sender on the struct. Production capacity is fixed at 256 to
match the previous behavior and `PgStore` / `InMemoryStore` semantics. The
sentinel receiver returned by `channel(_)` is dropped immediately;
production subscribers come from `subscribe()`.

### 3. The `PersistentStore` trait gains `subscribe()` and `shutdown()`

```rust
#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    // ... existing four methods unchanged ...
    fn subscribe(&self) -> broadcast::Receiver<SeqEvent>;
    async fn shutdown(&self);
}
```

`subscribe()` delegates to the store's internal `broadcast_tx`. `shutdown()`
takes the handles out of the mutex via `take()` and joins them with
`shutdown::join_with_timeout`. Calling `shutdown` more than once is safe:
the second and later calls observe `None` and return immediately.

`shutdown()` returns unit. Shutdown failures (timeout, task panic) are
logged internally and not propagated, mirroring `join_with_timeout`'s
contract: the process is exiting and there is no actionable recovery for
the caller. A `Result` return would force every call site to handle or
discard an error with no meaningful use.

### 4. `JoinError::is_cancelled()` is a clean exit

`join_with_timeout` is extended so a task that returned `JoinError`
satisfying `is_cancelled()` (typically from an external `AbortHandle`) is
logged at `warn` rather than `error`. The join itself is the intended
outcome — the test or operator explicitly aborted the task. A panic still
logs at `error`.

### 5. Test fixtures use `start_with_test_hooks` and abort via `AbortHandle`

`PgStore::start_with_test_hooks(pool, listener, shutdown, hooks)` returns
`(Arc<PgStore>, PgStoreTestHandles)`. The handle struct carries:

- `drain_abort: AbortHandle` and `listener_abort: AbortHandle`, extracted
  via `JoinHandle::abort_handle()` before the `JoinHandle`s were stored on
  the store. The caller gets abort capability; the store retains join
  capability. `JoinHandle` is not `Clone`, so this split is the correct
  pattern.
- `last_drain_pass_at: Arc<AtomicI64>` and `broadcast_watermark:
  Arc<AtomicI64>`, cloned from the store's owned atomics for direct test
  inspection.

No cfg-gated accessor methods on `PgStore` itself — tests get everything
they need at construction time, and the production type's surface area
stays unchanged.

`InMemoryStore::new_for_test(clock, ttl, broadcast_capacity)` exists solely
because `start()` fixes capacity at 256. The lagging-client WS test in
`ws_tests.rs` requires capacity-2 to trigger `RecvError::Lagged` with three
broadcast events. There is no `broadcast_sender()` accessor — tests inject
events via `persist.apply_run_event(...)` with distinct run IDs.

### 6. `AppState` drops `webhook_tx`; WS handlers subscribe via `persist`

```rust
pub struct AppState {
    pub persist: Arc<dyn PersistentStore>,
    pub webhook_secret: Option<String>,
    pub shutdown: CancellationToken,
    pub ws_tracker: TaskTracker,
}
```

`ws_handler` calls `state.persist.subscribe()` to obtain its
`broadcast::Receiver<SeqEvent>`. The store keeps the broadcast sender alive
for the lifetime of the `Arc<dyn PersistentStore>` clone held in
`AppState`, which `main.rs` keeps in scope across shutdown orchestration —
WS handlers see `shutdown.cancelled()` and send Close(1001) rather than
racing against `RecvError::Closed`.

### 7. `run_shutdown_orchestration` takes `Arc<dyn PersistentStore>`

```rust
pub async fn run_shutdown_orchestration(
    shutdown: CancellationToken,
    ws_tracker: TaskTracker,
    main_serve_task: JoinHandle<io::Result<()>>,
    persist: Arc<dyn PersistentStore>,
    metrics_handle: JoinHandle<()>,
    otel_handles: Option<OtelHandles>,
) -> bool
```

The three per-mode handle params (`drain_handle`, `listener_handle`,
`eviction_handle`, each `Option<JoinHandle<()>>`) collapse to a single
`persist.shutdown().await` call, joining whichever per-mode tasks the
active store actually owns. The aggregate ~13 s shutdown budget is
unchanged; per-task timeout constants
(`SHUTDOWN_TIMEOUT_DRAIN`, `SHUTDOWN_TIMEOUT_LISTENER`,
`SHUTDOWN_TIMEOUT_EVICTION`) move from explicit parameters of
`run_shutdown_orchestration` to internal-use constants of the store
implementations that consume them.

### 8. The "no live emitter when OTel shuts down" invariant still holds

OTel pipeline tear-down runs after `persist.shutdown()` returns and the
process metrics collector joins. The emitter enumeration in the
shutdown.rs comment block is updated: drain + listener (PG mode) and
eviction (in-memory mode) are now joined by `persist.shutdown()`. New
emitter categories must still be joined before `otel::shutdown` and named
in the comment block.

## Consequences

**Positive:**

- `main.rs` storage-mode dispatch shrinks from ~110 lines to ~25. The
  dispatch becomes "construct the right store; everything else is uniform."
- `run_shutdown_orchestration` drops `#[allow(clippy::too_many_arguments)]`
  and three per-mode handle params; the signature aligns with what the
  orchestrator actually does (drive shutdown across the persist, metrics,
  and OTel surfaces).
- `AppState` loses one field. WS handlers and the WS subscription seam
  align with the trait's `subscribe()`.
- Tests that exercise per-task death (`readyz::drain_abort_drives_503`,
  `metrics_min_pending_seq_test`) use `fixture.drain_abort.abort()` against
  the `AbortHandle` returned at construction. The store still joins the
  killed task on `persist.shutdown()`; `is_cancelled()` is a clean exit.
- `is_finished()` polling loops in `notify_listener_tests.rs` collapse to a
  single `persist.shutdown().await` bounded by timeout — a behavioral
  assertion (both tasks exited within budget) rather than a flag check.

**Neutral:**

- `listener.rs` is unchanged. The sole production callers of
  `spawn_listener_task` and `spawn_drain_task` move from `main.rs` /
  `start_full_server` to `PgStore::start_inner`.
- `eviction.rs::spawn_eviction_task` is unchanged. Its sole caller moves
  from `main.rs` to `InMemoryStore::start`.
- `PgStoreStartError` is a fresh error type for the seed-query failure
  path. It hand-implements `Display + Error` (matching `PersistError`'s
  style; `thiserror` is not a workspace dependency).

**Negative:**

- Tests against `PgStore` (the 17 sites in `persist_pg_tests.rs`,
  `transactional_writes_tests.rs::build_app_with_pg`,
  `outbox_tests.rs::build_app_with_pg`, `db_readyz_tests.rs`,
  `graceful_shutdown.rs::start_full_server`) now each connect a
  `PgListener` at construction. The shared helper
  `common::start_pg_store_for_test(pool, db_url)` keeps the boilerplate to
  a single call. Listener / drain tasks running against ephemeral
  per-test databases are reclaimed by nextest's process-per-test model on
  exit.

## Alternatives considered

**Keep the listener / drain spawns in `main.rs` and add a `subscribe()` /
`shutdown()` trait surface anyway.** Rejected because the listener + drain
already had a strong "I belong to PgStore" coupling — they own atomics that
PgStore reads from `liveness_check` and `read_snapshot`. Leaving the spawns
in `main.rs` would have kept the 11-Arc plumbing and the
`run_shutdown_orchestration` signature unchanged; the trait extension alone
addresses neither.

**Make `start()` return `Result<(Arc<Self>, Shutdown)>` where `Shutdown`
carries the handles.** Rejected: the handles are not useful to most callers
(only the shutdown orchestrator joins them), and exposing them clutters the
return type. The `Mutex<Option<Handles>>` on the store keeps the lifecycle
end-to-end inside the type and gives the second-call no-op for free.
