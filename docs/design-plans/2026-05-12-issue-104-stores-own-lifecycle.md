# Design plan — Stores own background-task lifecycle and event emission (issue #104)

**Status:** Implemented (PR feat/stores-own-background-task-lifecycle, 2026-05-13)

## Context

After issue #69 (PR #103) merged, `atc-core` became a pure domain library and all stateful persistence concerns moved into `atc-server::persist`. `PgStore` owned the PG pool and its watermark/heartbeat Arcs, but the listener and drain tasks that *used* those Arcs were still spawned as free functions from `main.rs` (`spawn_listener_task`, `spawn_drain_task` in `listener.rs`). The eviction task for `InMemoryStore` was the same shape. The `webhook_tx` broadcast channel was constructed in `main.rs` and passed into both stores and `AppState`.

This meant store-internal mechanics (connection management, watermark Arcs, broadcast sender) leaked across the abstraction boundary: `main.rs` plumbed 11 Arc parameters into `spawn_drain_task` and threaded `webhook_tx` through stores, drain task, and `AppState`. `run_shutdown_orchestration` took three separate store-task handles.

This plan moves each store's full background-task lifecycle inside the store, exposes event emission via `subscribe()` on the trait, and collapses `main.rs` / `shutdown.rs` plumbing significantly.

## Before / After Ownership

```
BEFORE                               AFTER
──────────────────────────────────   ──────────────────────────────────
main.rs                              main.rs
  ├── broadcast::channel(256)          ├── PgStore::start(pool, listener, shutdown)
  ├── min_pending_seq Arc                    └── owns: pool, watermark, heartbeat,
  ├── drain_in_flight Arc                         broadcast_tx, listener+drain handles
  ├── PgStore::new(pool, arcs)        ├── InMemoryStore::start(clock, ttl, period, shutdown)
  ├── spawn_listener_task(...)              └── owns: state, seq, clock, ttl,
  ├── spawn_drain_task(...)                       broadcast_tx, eviction handle
  ├── spawn_eviction_task(...)         └── persist: Arc<dyn PersistentStore>
  └── AppState { persist, webhook_tx }       (no webhook_tx field)

shutdown.rs::run_shutdown_orchestration
  drain_handle: Option<JoinHandle>,    persist: Arc<dyn PersistentStore>
  listener_handle: Option<JoinHandle>,     └── persist.shutdown().await
  eviction_handle: Option<JoinHandle>       (joins all per-mode tasks internally)
```

## Trait extension

```rust
#[async_trait::async_trait]
pub trait PersistentStore: Send + Sync {
    // existing four methods unchanged ...
    fn subscribe(&self) -> broadcast::Receiver<SeqEvent>;
    async fn shutdown(&self);
}
```

`shutdown()` returns unit. Failures are logged internally and not propagated — the process is exiting and there is no actionable recovery for the caller.

Both stores hold `std::sync::Mutex<Option<Handles>>` internally. On first call, `take()` consumes the handles and joins them via `join_with_timeout`. On second call, `take()` returns `None` and `shutdown()` returns immediately.

## PgStore::start

```rust
pub async fn start(
    pool: PgPool,
    listener_conn: PgListener,
    shutdown: CancellationToken,
) -> Result<Arc<Self>, PgStoreStartError>

#[cfg(any(test, feature = "test-support"))]
pub async fn start_with_test_hooks(
    pool: PgPool,
    listener_conn: PgListener,
    shutdown: CancellationToken,
    hooks: PgStoreTestHooks,
) -> Result<(Arc<Self>, PgStoreTestHandles), PgStoreStartError>
```

`PgStoreTestHooks` mirrors the four optional injection params on the existing spawn functions (`received_counter`, `observed_passes`, `drain_started`, `drain_delay`). Production `start()` passes `None` for all four.

`PgStoreTestHandles` gives the test fixture what it needs at construction time — no cfg-gated accessor methods on the type itself. It carries the listener / drain `AbortHandle`s plus cloned `last_drain_pass_at` / `broadcast_watermark` Arcs.

`AbortHandle`s are extracted via `JoinHandle::abort_handle()` before the `JoinHandle`s are stored on the store. The caller gets abort capability; the store retains join capability.

`start()` / `start_with_test_hooks()` order:
1. Construct `broadcast::channel(256)` → `broadcast_tx` + sentinel receiver (dropped).
2. Construct Arcs: `broadcast_watermark`, `last_drain_pass_at`, `min_pending_seq`, `drain_in_flight`, `drain_notify`.
3. Capture `startup_at = Instant::now()` **before** the watermark query.
4. Run `SELECT COALESCE(MAX(seq), 0) FROM outbox`; seed gauge. **Last fallible step.**
5. Spawn `spawn_listener_task` and `spawn_drain_task` (unchanged signatures in `listener.rs`).
6. Extract `AbortHandle`s via `.abort_handle()` before storing `JoinHandle`s.
7. Store `JoinHandle`s in `Mutex::new(Some(PgStoreHandles { ... }))`.
8. Return `Arc<Self>` (with `PgStoreTestHandles` in the test variant).

**Partial-failure invariant.** After step 5, no fallible operations remain. A guardrail comment states this; a future contributor adding a fallible step after the spawns must cancel and join the already-spawned tasks before returning `Err`.

`PgStore::shutdown`:
1. Take handles: `let h = self.handles.lock().unwrap().take(); // drop guard before .await`
2. If `None` return (second-call no-op).
3. `join_with_timeout(h.drain, SHUTDOWN_TIMEOUT_DRAIN, "drain")` then `join_with_timeout(h.listener, SHUTDOWN_TIMEOUT_LISTENER, "listener")`.
4. `JoinError::is_cancelled()` (from a test's `drain_abort.abort()`) is treated as clean exit — `join_with_timeout` logs at `warn` rather than `error` for cancelled tasks.

The existing `PgStore::new`, `PgStore::new_for_test`, `DrainHandles`, and `drain_handles()` are **removed** — they existed solely to bridge `main.rs` → tasks.

## InMemoryStore::start

```rust
pub fn start(
    clock: Arc<dyn Clock>,
    completed_ttl: Duration,
    eviction_period: Duration,
    shutdown: CancellationToken,
) -> Arc<Self>    // synchronous — no .await at call site
```

The store constructs its own `broadcast::channel(256)` and spawns the eviction task internally. `InMemoryStore::subscribe` returns `self.broadcast_tx.subscribe()`. `InMemoryStore::shutdown` takes the handle, drops the guard before `.await`, and calls `join_with_timeout`.

**One test constructor — custom capacity, no eviction task:**

```rust
#[cfg(any(test, feature = "test-support"))]
pub fn new_for_test(
    clock: Arc<dyn Clock>,
    completed_ttl: Duration,
    broadcast_capacity: usize,
) -> Arc<Self>
```

This exists solely because `start()` fixes capacity at 256 and the lagging-client test in `ws_tests.rs` requires capacity-2 to trigger `RecvError::Lagged` with 3 events. There is no `broadcast_sender()` accessor — tests drive events through `persist.apply_run_event(...)` with distinct run IDs.

The existing `InMemoryStore::new(clock, completed_ttl, broadcast_tx)` is **removed** — the broadcast sender is now internal.

## main.rs collapse

The ~110-line PG/InMem dispatch (lines 147–256) collapses to ~25 lines: a `match cfg.database_url` on whether to run `PgStore::start(pool, listener_conn, shutdown.clone())` or `InMemoryStore::start(clock, ttl, period, shutdown.clone())`.

`AppState` loses `webhook_tx`. WS handler: `state.persist.subscribe()` replaces `state.webhook_tx.subscribe()`. Imports removed from `main.rs`: `AtomicBool`, `AtomicI64`, `Notify`, `spawn_eviction_task`, `broadcast`, `Instant`, `SystemTime`, `UNIX_EPOCH`, the standalone `now_millis()` helper.

## shutdown.rs collapse

The signature drops from 8 params (`#[allow(clippy::too_many_arguments)]`) to 6 — three per-mode `Option<JoinHandle<()>>` params collapse to `persist: Arc<dyn PersistentStore>`. The three `join_with_timeout` calls collapse to `persist.shutdown().await`. The OTel comment block is rewritten to reference `persist.shutdown()` joining per-mode emitters.

Inline shutdown.rs unit tests call `run_shutdown_orchestration` with `persist: Arc<dyn PersistentStore>` — a fresh `InMemoryStore::start(SystemClock, 1h, 1m, child_token)` whose `shutdown()` is safe to call.

## Test fixture migration

`AppFixture` carries (in addition to its existing fields) `drain_abort: AbortHandle`, `listener_abort: AbortHandle`, and cloned `last_drain_pass_at` / `broadcast_watermark` Arcs. `build_app_inner` calls `PgStore::start_with_test_hooks(...)` and populates `AppFixture` from the returned `PgStoreTestHandles`.

`common::start_pg_store_for_test(pool, db_url)` is a shared helper that returns `Arc<PgStore>` — used by `persist_pg_tests.rs` (17 sites), `transactional_writes_tests.rs::build_app_with_pg`, `outbox_tests.rs::build_app_with_pg`, `db_readyz_tests.rs::build_app_with_pool`, and `graceful_shutdown.rs::start_full_server`. Each call connects a `PgListener` and a fresh `CancellationToken`.

`ws_tests.rs::test_setup` uses `InMemoryStore::new_for_test(clock, ttl, broadcast_capacity)`. The lagging-client test replaces the `state.webhook_tx.send(SeqEvent {...})` block with three `state.persist.apply_run_event(env)` calls against distinct `RunId`s.

`is_finished()` polling loops on `drain_handle` / `listener_handle` are replaced with `tokio::time::timeout(budget, fixture.state.persist.shutdown()).await.expect(...)` — a behavioral assertion (both tasks exited within budget) rather than a flag check.

`fixture.drain_handle.abort()` becomes `fixture.drain_abort.abort()` (call sites: `readyz.rs`, `metrics_min_pending_seq_test.rs`).

## Locked decisions

- Full lifecycle symmetry: each store owns *all* of its background tasks, not just some.
- `shutdown(&self)` returns unit — failures logged internally, not propagated.
- `Mutex<Option<Handles>>::take()` provides second-call no-op.
- `JoinError::is_cancelled()` from a test abort is treated as clean exit (log `warn` not `error`).
- Span names `listener.task`, `drain.task`, `drain.pass`, `drain.broadcast`, `listener.recv` unchanged.
- `PersistentStore::shutdown()` is single-caller (only called by `run_shutdown_orchestration`).
- OTel shutdown still runs after `persist.shutdown()` returns.
- Aggregate ~13 s shutdown budget unchanged; per-task constants move inside store implementations.
- `broadcast::channel(256)` is the production capacity for both stores.
- `listener.rs` stays top-level; no file moves.
- `now_millis()` stays as a free helper in `pg.rs` (mirrors the existing copy in `listener.rs`); a `Clock`-injection refactor for testable heartbeats is deferred.

## Verification

```bash
just setup
cargo nextest run -p atc-server        # Docker required (216 tests pass)
cargo clippy -p atc-server -- -D warnings
just lint && just check && just build
just test-e2e                          # WS subscription seam changed
```

Spot-checks: `git grep -n 'webhook_tx' src/state.rs` → zero hits; `git grep -n 'spawn_listener_task\|spawn_drain_task\|spawn_eviction_task' src/main.rs` → zero hits.
