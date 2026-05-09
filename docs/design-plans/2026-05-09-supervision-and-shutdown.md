# Background Task Supervision and Graceful Shutdown

## Context

Issue #60 asks for an audit and alignment of three background-task supervision patterns in the Rust backend:

- **Listener task** (`backend/crates/atc-server/src/listener.rs:70-126`) — `CancellationToken` + `JoinHandle` + `select!`. The most principled pattern.
- **Drain task** (same file, `:150-300`) — same pattern; both arms watch the token.
- **Eviction task** (`backend/crates/atc-core/src/state_machine.rs:489-503`) — `JoinHandle` only, no token; bare `loop { ticker.tick().await }`.
- **Process metrics collector** (`backend/crates/atc-server/src/metrics.rs:228-238`) — fire-and-forget, no handle, no cancellation.

Investigation surfaced a fourth surface the issue's frame does not name: **WS handlers** at `backend/crates/atc-server/src/ws.rs:21-90`. axum's `WebSocketUpgrade::on_upgrade` spawns the handler in a detached task that axum does not track in its connection accounting (hyper's HTTP/1 upgrade returns `Ready(Ok(()))` once the protocol switch completes, so the per-connection task ends at upgrade time and the WS callback runs independently). Today, when shutdown happens, `axum::serve` resolves cleanly on cancellation, `main` proceeds, and the runtime drops the still-running WS tasks at process exit. Clients see a TCP reset rather than a `Close` frame. This is a UX papercut, not a deadlock — graceful shutdown completes today; it just does so abruptly for any connected WS client.

The existing `.abort()`-then-500ms-timeout block at `main.rs:314-322` likewise runs to completion: it kills the listener, drain, and eviction tasks at their next await point. Cooperative shutdown of those tasks is plausible (the listener and drain already accept tokens; eviction does not) and is the clean answer to #60's audit ask.

This plan addresses both: (1) align the four background-task supervision patterns around `CancellationToken` + cooperative exit, and (2) extend cancellation to the WS handler so shutdown emits a clean `Close(1001 "going away")` to connected clients before process exit. To make (2) testable rather than best-effort, the spawned WS tasks are tracked via `tokio_util::task::TaskTracker`; main awaits the tracker before exiting.

## Definition of Done

- The five cancellation surfaces in scope — eviction, listener, drain, process metrics collector, and WS handlers — observe a cancellation signal and exit cooperatively. (The Prometheus exporter upkeep loop spawned by `metrics::build()` at `metrics.rs:61` is library-managed by `metrics_exporter_prometheus` and therefore not a surface this plan can supervise; out of scope.)
- Spawned WS handler tasks are tracked via `tokio_util::task::TaskTracker`, so `main` can wait for them to flush their final events and emit `Close(1001 "going away")` before process exit.
- `main.rs` orchestrates a two-phase shutdown: phase 1 (signal → background tasks drain → drain handle awaited) and phase 2 (`ws_close` cancellation → tracked WS handlers send Close → tracker drains → axum serves resolve → remaining handles joined).
- Drain task finishes its current pass before exiting; the cancellation token is checked between passes only, never inside `drain_pass()`.
- The two `axum::serve(...).with_graceful_shutdown(...)` futures (which implement `IntoFuture`, not `Future` directly, and yield `io::Result<()>`) are spawned as separate tokio tasks via `IntoFuture::into_future` so they are polled independently of `main`'s shutdown choreography.
- Tests verify each cancellation surface exits within its bounded time on token cancel, including a full-server integration test that asserts a connected WS client receives a `Close` frame with status 1001 within budget.
- `docs/architecture/backend-server.md` gains a "Supervision and Shutdown" section; `docs/architecture/metrics.md` is updated to reflect the supervised process collector; touched `Last verified:` lines on architecture docs and any updated `CLAUDE.md` files are bumped.

## Locked Decisions

From clarification in this planning session:

- **Two-phase shutdown.** Phase 1 cancels axum's `with_graceful_shutdown`, listener, drain, eviction, metrics. After `main` awaits the drain handle, phase 2 cancels WS handlers. WS handlers do **not** observe phase 1.
- **Drain mid-pagination cancellation — out.** The token is checked between drain passes only. `drain_pass()` runs to completion or to a Postgres error. A missed batch persists in the outbox and is drained on next startup.
- **Process metrics collector — wired.** `spawn_process_collector` returns a `JoinHandle<()>` and accepts a `CancellationToken`, the same shape as the other tasks. Not fire-and-forget.
- **Going-away signal — Close frame 1001.** No new tagged-message envelope is introduced. `Close(1001 "going away")` is the going-away primitive. Future non-event messages (e.g., `ServerHello` from issue #47) will introduce a tagged-message envelope when that work is designed; until then, the broadcast channel carries only `SeqEvent`.
- **WS handler supervision — `tokio_util::task::TaskTracker`.** Spawned WS tasks are tracked; `main` awaits the tracker's drain before exiting. The user's intent is explicit supervision rather than best-effort fire-and-forget.

From the codebase (verified against axum 0.8.x and tokio-util 0.7.x source):

- `axum::serve(...).with_graceful_shutdown(...)` returns a future whose `Output` is `()` (not `Result`) — the documented "will never error" contract. Today's `if let Err(e) = res { ... }` branches in `main.rs:295-310` are effectively dead code with respect to the serve futures.
- `tokio_util::sync::CancellationToken` lives in the `sync` module and is unconditional — no feature flag required. `tokio_util::task::TaskTracker` is in the `task` module, gated behind feature `rt` (verified in tokio-util 0.7.18 source).
- `atc-server` currently uses bare `tokio-util = "0.7.18"` (no features) at `Cargo.toml:23`. This work adds the `rt` feature so `TaskTracker` is available.
- `atc-core` does not depend on `tokio-util` today. This work adds it (no features needed — only `CancellationToken` is used).
- `AppState` (`backend/crates/atc-server/src/state.rs`) is the right place to hand the WS-close token and the task tracker to per-request handlers — it already holds the broadcast `Sender`.

## Architecture

### Two cancellation tokens

Two `CancellationToken`s are constructed in `main.rs`:

- `shutdown` — phase 1. Cancelled by the signal handler. Watched by axum × 2 (`with_graceful_shutdown`), listener, drain, eviction, metrics.
- `ws_close` — phase 2. Cancelled by `main` after the drain handle has been awaited. Watched by WS handlers (via `AppState.ws_close`).

Two tokens, not one, because the WS handlers must see the drain task finish broadcasting before they themselves close. A single token visited by everyone simultaneously truncates the WS event stream; chaining via `tokio::sync::Notify` or a `watch` channel would express the same dependency, but with more state than two named tokens. The two-token design is the simplest expression of the phase boundary; the implementation may revisit if a third phase emerges.

### Phase 1: drain the queue

```
signal → shutdown.cancel()
  ├── axum (main + metrics): graceful_shutdown future resolves → stops accepting new connections
  ├── listener: cancel arm fires → exits
  ├── eviction: cancel arm fires → exits
  ├── metrics: cancel arm fires → exits
  └── drain: cancel arm fires between passes → finishes current pass → exits
```

`axum::serve` does NOT wait for upgraded WS connections to complete (per the upgrade-task lifecycle described in Context). The serves resolve as soon as their non-upgraded HTTP traffic drains.

### Phase 2: close client connections

```
main awaits drain_handle (bounded by SHUTDOWN_TIMEOUT_DRAIN = 5 s)
  ↓
ws_close.cancel()
  ↓
each WS handler's biased select arm fires
  ↓
each sends Message::Close(CloseFrame { code: 1001, reason: "going away" })
  ↓
each returns; ws_tracker counts down
  ↓
main awaits ws_tracker.wait() (bounded by SHUTDOWN_TIMEOUT_WS = 2 s)
  ↓
main awaits the spawned main_serve and metrics_serve task handles
  ↓
main joins listener / eviction / metrics task handles
```

### `main.rs` shutdown sequence (full pseudocode)

The serves must be polled continuously while `main` is choreographing shutdown. The cleanest expression is to spawn each serve as a tokio task; the runtime then drives them. `main` awaits a clone of `shutdown` for the cue to begin orchestration, and joins the serve tasks at the end. `axum::serve(...).with_graceful_shutdown(...)` is `IntoFuture`, not `Future`, so the spawn site converts via `.into_future()` (or wraps in `async move { x.await }`). The output type is `io::Result<()>`; in practice with `with_graceful_shutdown` it always resolves to `Ok(())` (per axum's "will never error" contract for that wrapper).

```rust
use std::future::IntoFuture;

// 1. spawn serves so they are polled independently of main's shutdown choreography.
//    JoinHandle<io::Result<()>> for each.
let main_serve_task = tokio::spawn(main_serve.into_future());
let metrics_serve_task = tokio::spawn(metrics_serve.into_future());

// 2. wait for the shutdown signal (signal handler in its own task fires shutdown.cancel()).
shutdown.cancelled().await;
// at this instant: with_graceful_shutdown futures resolve; listener/eviction/metrics/drain all
// observe the same token and begin exiting; new connections rejected by axum.

// 3. wait for drain to finish its current pass (PG mode only — drain_handle is an Option).
if let Some(drain_handle) = drain_handle {
    let drain_abort = drain_handle.abort_handle();
    match tokio::time::timeout(SHUTDOWN_TIMEOUT_DRAIN, drain_handle).await {
        Ok(Ok(())) => tracing::info!("drain task exited cleanly"),
        Ok(Err(e)) => tracing::error!(error = %e, "drain task ended with error"),
        Err(_) => {
            tracing::error!(task = "drain", "shutdown timeout exceeded; aborting (best-effort)");
            drain_abort.abort();
        }
    }
}

// 4. fire phase 2: WS handlers close.
ws_close.cancel();

// 5. wait for tracked WS tasks to drain (Close frames sent, handlers returned).
//    TaskTracker has no abort handle, so on timeout we log and continue; in-flight WS tasks
//    will be killed when the runtime drops at process exit. This is best-effort and acceptable.
ws_tracker.close();
match tokio::time::timeout(SHUTDOWN_TIMEOUT_WS, ws_tracker.wait()).await {
    Ok(()) => tracing::info!("ws tracker drained cleanly"),
    Err(_) => tracing::error!(task = "ws_tracker", "shutdown timeout exceeded; runtime drop will kill remaining handlers"),
}

// 6. wait for the spawned serve tasks to complete.
//    Both should already be complete (their graceful_shutdown futures resolved at step 2);
//    this is a clean-up await with a backstop timeout in case a connection stalls.
let serves_join = async { tokio::join!(main_serve_task, metrics_serve_task) };
match tokio::time::timeout(SHUTDOWN_TIMEOUT_SERVES, serves_join).await {
    Ok((main_res, metrics_res)) => {
        if let Ok(Err(e)) = main_res { tracing::error!(error = %e, "main serve ended with error"); }
        if let Ok(Err(e)) = metrics_res { tracing::error!(error = %e, "metrics serve ended with error"); }
    }
    Err(_) => tracing::error!("axum serves did not resolve within timeout; runtime drop will reap"),
}

// 7. join the remaining background-task handles.
for (name, handle, timeout) in [
    ("listener", listener_handle, SHUTDOWN_TIMEOUT_LISTENER),
    ("eviction", eviction_handle, SHUTDOWN_TIMEOUT_EVICTION),
    ("metrics", metrics_handle, SHUTDOWN_TIMEOUT_METRICS),
] {
    // listener_handle is also Option<...> in non-PG mode; guard accordingly.
    if let Some(handle) = handle {
        let abort = handle.abort_handle();
        match tokio::time::timeout(timeout, handle).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::error!(task = name, error = %e, "task ended with error"),
            Err(_) => {
                tracing::error!(task = name, "shutdown timeout exceeded; aborting (best-effort)");
                abort.abort();
            }
        }
    }
}
```

The illustrative `for` loop has heterogeneous `JoinHandle<T>` types and one Option-typed entry (`listener_handle` is `None` outside PG mode), so the implementation expresses it as three explicit blocks or a small generic helper. Either way, every `error!` log carries the task name.

Two distinct timeout outcomes:

- **Per-handle JoinHandle timeouts** (drain, listener, eviction, metrics) — log `error!` with task name and call `AbortHandle::abort()`. Abort is asynchronous; the task may continue past the abort to its next await point. The plan does not claim deterministic termination on the timeout path; the clean-exit path is the one with hard ordering.
- **`ws_tracker.wait()` and serves-join timeouts** — `TaskTracker` and `tokio::join!`-of-spawn-handles have no per-task abort surface. On timeout, log `error!` and proceed; the runtime drops surviving tasks at process exit.

### WS handler third select arm — biased

Today (`ws.rs:30-83`):

```rust
loop {
    tokio::select! {
        result = rx.recv() => { /* ... */ }
        msg = socket.recv() => { /* ... */ }
    }
}
```

After:

```rust
loop {
    tokio::select! {
        biased;
        result = rx.recv() => { /* unchanged */ }
        msg = socket.recv() => { /* unchanged */ }
        () = ws_close.cancelled() => {
            let _ = socket.send(Message::Close(Some(CloseFrame {
                code: 1001,
                reason: "going away".into(),
            }))).await;
            break "shutdown";
        }
    }
}
```

`biased;` evaluates arms top-down rather than randomly. While `rx.recv()` returns `Ready(Ok(_))`, the handler keeps draining buffered events. Only when `rx.recv()` returns `Pending` does the macro fall through to `socket.recv()` (also `Pending` for an idle client) and finally `ws_close.cancelled()` — at which point the Close frame is sent and the handler exits. This is the design's mechanism for "events before close" on the clean, idle-client path. The send is `let _ = ...`: if the client has already disconnected, the Close attempt no-ops.

If a client sends a Close frame or other inbound frame *during* shutdown, `socket.recv()` (above `ws_close.cancelled()` in the biased order) wins and the handler exits via the existing client-initiated branch — the server does not send its own Close 1001 in that case, because the client has already closed. This is the correct outcome but means the "server emits Close 1001" guarantee is conditional on the client being idle at shutdown time. The integration test (AC7) asserts this for an idle client; the unit test (AC8) asserts the buffered-events-before-cancel path under a controlled sender.

### WS task tracking — `TaskTracker`

`AppState` gains a `pub ws_tracker: TaskTracker` field, populated in `main.rs` from a `TaskTracker::new()`. `ws_handler` wraps the handler future via `state.ws_tracker.track_future(handle_socket(socket, rx, ws_close))` before passing it into `ws.on_upgrade(...)` — axum spawns the wrapped future, and the tracker counts it. On shutdown, `main` calls `ws_tracker.close()` (which makes `wait()` return when the in-flight count reaches zero) and then `ws_tracker.wait()` (bounded by `SHUTDOWN_TIMEOUT_WS`).

Note: `TaskTracker::close()` does **not** prevent new tasks from being tracked — it only enables `wait()` to terminate. So if a late WS upgrade arrives between `close()` and `wait()` returning (because axum's graceful_shutdown stops accepting new TCP connections but already-accepted-but-not-yet-handled HTTP requests can still upgrade), the new task is tracked and `wait()` waits for it. Since `ws_close` is already cancelled by that point, the late handler enters its cancellation arm immediately and exits fast. Net effect: late upgrades extend `wait()` by milliseconds, not seconds, and the timeout backstop catches anything pathological.

### Per-task timeout budgets

| Constant | Value | Applies to |
|---|---|---|
| `SHUTDOWN_TIMEOUT_DRAIN` | 5 s | drain handle (worst case: one in-flight 500-row pass with PG round-trips) |
| `SHUTDOWN_TIMEOUT_WS` | 2 s | `ws_tracker.wait()` — time for connected WS clients to receive their Close frames |
| `SHUTDOWN_TIMEOUT_SERVES` | 3 s | spawned axum serve tasks |
| `SHUTDOWN_TIMEOUT_LISTENER` | 1 s | listener handle |
| `SHUTDOWN_TIMEOUT_EVICTION` | 1 s | eviction handle |
| `SHUTDOWN_TIMEOUT_METRICS` | 1 s | metrics handle |

Aggregate worst-case shutdown: ~13 seconds. K8s `terminationGracePeriodSeconds` defaults to 30; we have headroom.

### Why not extract a `SupervisedTask` helper?

Three sites of `(token, handle, timeout, name)` quadruples is below the rule of three for premature abstraction. Each site has different shutdown semantics (drain has the "finish current pass" rule and is awaited *before* phase 2; eviction and metrics are simple ticker selects awaited *after* phase 2; listener has its own select shape with error retry). Extracting a uniform helper would force-fit them. Reject for this scope; revisit if a fifth or sixth task lands.

### Why is `CancellationToken` constructed in `main` and not derived from `AppState`?

`main.rs` constructs both tokens before `AppState` exists. The `shutdown` token wires into the listener, drain, and eviction tasks, which spawn before `axum::serve` starts. `ws_close` and `ws_tracker` are cloned into `AppState` so per-request WS handlers can observe them.

### Test strategy

Unit tests for each cancellation surface use explicit `CancellationToken::new()` + `.cancel()` calls — not real OS signals. Each asserts bounded exit time using `tokio::time::timeout` around the handle's `await`.

The integration test (a new file `backend/crates/atc-server/tests/graceful_shutdown.rs`) spins up the full PG-mode server using the existing testcontainers harness, connects a WS client via `tokio-tungstenite`, fires `shutdown.cancel()` from the test, and asserts:

1. The WS client receives `Message::Close(_)` with status code 1001 within `SHUTDOWN_TIMEOUT_WS + epsilon`.
2. The connection then drops.
3. All four background-task handles plus both spawned `serve` tasks complete within the aggregate budget.

The "events before close" property is a clean-path *design* guarantee from the biased select, not an integration-test invariant. Asserting it deterministically would require a synchronization barrier ("wait until drain has processed N rows") whose fragility outweighs its value. The unit tests on the WS handler verify the biased ordering using a controlled broadcast sender.

For the abort-on-timeout path: a unit test injects a deliberately-stuck future as a `JoinHandle` and verifies the timeout-then-error-log behavior fires within bounded time.

## Implementation Phases

TDD-shaped, but pragmatic about Rust's whole-crate compile model: tests that reference new types must land alongside the minimum implementation that makes them compile. Each phase below pairs failing tests with their minimum implementation slice.

### Phase 1 — Eviction task supervision (atc-core)

- Failing test in `atc-core::state_machine::tests`: eviction task accepts a token; on cancel, the handle resolves within 200 ms.
- `cargo add tokio-util` in `backend/crates/atc-core/` (no feature flags — `CancellationToken` is in the unconditional `sync` module per the verified upstream source). If `cargo build` complains, add features as the compiler indicates.
- Change `start_eviction_task` signature to accept `cancel: CancellationToken`.
- Loop body: `tokio::select!` on `cancel.cancelled()` (break) vs `ticker.tick().await` (run sweep).
- Update the call site in `main.rs` to pass a cloned token.

### Phase 2 — Metrics collector supervision (atc-server)

- Failing test in `atc-server::metrics` (new file or extension): `spawn_process_collector` returns a handle; on cancel, the handle resolves within 200 ms.
- Investigate `metrics_process::Collector::collect()` — confirm its blocking profile. If it's IO-blocking on `/proc`, document that the cancel arm only fires *between* ticks, not mid-collect (acceptable for shutdown latency).
- `spawn_process_collector(cancel: CancellationToken) -> JoinHandle<()>` — return handle, accept token, `select!` on cancel vs ticker.
- Update the call site in `main.rs`.

### Phase 3 — WS handler supervision (atc-server)

- Failing tests in `atc-server::ws` tests (new): (a) idle connected client receives `Message::Close(_)` with code 1001 on token cancel; (b) under a controlled broadcast sender (kept alive past cancellation), biased select drains all buffered events before observing cancellation.
- Update `atc-server/Cargo.toml` to add the `rt` feature to `tokio-util` (verified by codex against tokio-util 0.7.18 source: `tokio_util::task::TaskTracker` is gated behind `feature = "rt"`). Resulting line: `tokio-util = { version = "0.7.18", features = ["rt"] }`.
- Address the existing inconsistency in `backend/crates/atc-server/tests/ws_tests.rs:282` while the WS surface is being touched — the test expects continued delivery on `Lagged`, but `ws.rs:54` closes on `Lagged`. Fix the test to match current behavior, or update the production code if the test reflects the intended contract (resolve in implementation; default is to make the test match the code).
- Add `pub ws_close: CancellationToken` and `pub ws_tracker: TaskTracker` fields to `AppState`; populate in `main.rs` from freshly-constructed instances.
- `handle_socket` accepts a `CancellationToken` argument; `ws_handler` clones it from `AppState.ws_close` and threads it through `ws.on_upgrade(...)`. Add the third `biased`-prefixed `select!` arm that sends `Message::Close(CloseFrame { code: 1001, reason: "going away" })` and exits.
- `ws_handler` wraps the upgrade future in `state.ws_tracker.track_future(...)` so the spawned handler is tracked.

### Phase 4 — main.rs shutdown sequence rewrite

- Failing integration test: `backend/crates/atc-server/tests/graceful_shutdown.rs` — PG-mode server in testcontainers, idle WS client connected, `shutdown.cancel()` fires from the test, asserts (a) Close 1001 received within budget, (b) all handles complete within aggregate budget.
- Failing unit test for the abort-on-timeout path with a deliberately-stuck future.
- Construct two tokens (`shutdown`, `ws_close`) and a `TaskTracker` in main. Plumb into AppState and task spawn sites.
- Spawn both `main_serve` and `metrics_serve` as tokio tasks via `tokio::spawn(serve.into_future())` (since `WithGracefulShutdown` is `IntoFuture`, not `Future`) so they are polled independently. Output type is `JoinHandle<io::Result<()>>`.
- Replace the existing `tokio::select!` orchestration at `main.rs:295-322` with the seven-step sequence in the pseudocode above. The current code's `if let Err(e) = res` branches are dead under graceful shutdown but harmless; the rewrite makes them go away by structure.
- Handle the in-memory mode path: `drain_handle` and `listener_handle` are `None` outside PG mode (per the existing `main.rs:195` conditional). Pseudocode's step 3 and step 7 guard with `if let Some(...)`.
- Define per-task timeout constants (`SHUTDOWN_TIMEOUT_DRAIN`, etc.) in a single block at the top of `main.rs` for ergonomic tuning.

### Phase 5 — Architecture docs

- Add a "Supervision and Shutdown" section to `docs/architecture/backend-server.md`. Update existing prose at L363-374 (which says "On graceful shutdown, abort the eviction task, listener task, and drain task") to reflect the cooperative two-phase design. Document the five cancellation surfaces, the WS handler's role, the two cancellation phases, the `TaskTracker` for WS supervision, the per-task timeout budgets, and the operator-facing shutdown contract (next bullet). Bump `Last verified:`.
- **Operator-facing shutdown contract subsection** — what SIGTERM does, the aggregate timeout budget (~13 s worst case), the durability of webhooks committed during the shutdown window (committed rows persist in outbox; drained by next replica on its next startup pass), and recommended K8s settings (`terminationGracePeriodSeconds: 30` is sufficient; `preStop` hook for LB de-registration is a separate operational improvement tracked outside this PR). Operators should be able to predict ATC's shutdown behavior from this prose alone, without reading source.
- Update `docs/architecture/metrics.md` (`:48`) to reflect that `spawn_process_collector` now returns a `JoinHandle<()>` and accepts a `CancellationToken`. Bump `Last verified:`.
- Verify `scripts/doc-mapping.sh` mappings remain accurate. Existing entries for `main.rs`, `listener.rs`, `metrics.rs`, `state_machine.rs` already point to the right docs; no new mapping expected.
- `backend/crates/atc-core/CLAUDE.md` and `backend/crates/atc-server/CLAUDE.md` — verify they don't make stale claims about supervision patterns. The known stale spot at `backend/crates/atc-server/CLAUDE.md:46` (claims lagging clients are not disconnected, but `ws.rs:54` closes on `Lagged`) is in scope to fix in this phase since the file is being touched anyway. Bump `Last verified:` on any updated `CLAUDE.md`.
- The root `/Users/brajkovic/Projects/atc/CLAUDE.md` does not carry shutdown-specific claims; no forced bump.

### Phase 6 — Final verification

- `just test` passes (including the new tests).
- `just lint` clean.
- Manual SIGTERM verification: `just dev`, connect a WS client (`websocat ws://localhost:8080/v1/ws`), `kill -TERM` the backend process, observe a clean exit within ~13 s with the WS client receiving a Close 1001 frame.

## Acceptance Criteria

- AC1: `start_eviction_task` accepts a `CancellationToken`; on cancel, the returned handle resolves within 200 ms.
- AC2: `spawn_process_collector` returns a `JoinHandle<()>` and accepts a `CancellationToken`; on cancel, the handle resolves within 200 ms (between ticks).
- AC3: `handle_socket` observes a `CancellationToken`; on cancel, an idle connected client (i.e., not concurrently sending its own Close or other frames) receives a `Message::Close` frame with status code 1001 and reason "going away", and the handler returns within 200 ms. A client that initiates its own close concurrently exits via the existing client-initiated path; the server does not send a server-side Close in that case.
- AC4: Existing listener and drain tasks exit cooperatively on phase-1 token cancel — `main.rs`'s happy path no longer calls `.abort()` on them.
- AC5: `drain_pass()` contains no cancellation-token checks. The token is observed only between passes (verified by code review of the diff).
- AC6: `main.rs` orchestrates two phases: phase 1 cancels `shutdown`; phase 2 (`ws_close.cancel()`) is fired only after the drain handle has been awaited or its timeout has fired.
- AC7: With an idle connected WS client (one that is not sending its own frames during shutdown), `shutdown.cancel()` causes the client to receive a `Close(1001 "going away")` frame within `SHUTDOWN_TIMEOUT_WS + epsilon` (~2.5 s), and all background-task handles plus both spawned serve tasks complete within the aggregate budget (~13 s).
- AC8: A WS-handler unit test, with the broadcast sender held alive past cancellation (so `RecvError::Closed` does not preempt the cancel arm), shows that biased select drains all buffered events before observing cancellation on the clean path.
- AC9: When a per-handle JoinHandle await exceeds its timeout (drain, listener, eviction, metrics), `main.rs` logs `error!` naming the task, calls `AbortHandle::abort()` (best-effort and asynchronous), and continues. When `ws_tracker.wait()` or the spawned-serves join exceeds its timeout, `main.rs` logs `error!` naming the surface and continues; no per-surface abort is available at that layer, so surviving tasks are reaped by runtime drop at process exit.
- AC10: `docs/architecture/backend-server.md` has a "Supervision and Shutdown" section documenting the supervision model, timeouts, and an operator-facing shutdown contract subsection (SIGTERM behavior, aggregate timeout, webhook durability across replicas, K8s settings guidance); `docs/architecture/metrics.md` reflects the supervised process collector. `Last verified:` lines on touched architecture docs and `CLAUDE.md` files are updated.

## Documents to Update

- `docs/architecture/backend-server.md` — new "Supervision and Shutdown" section; revise L363-374 prose; bump `Last verified:`.
- `docs/architecture/metrics.md` — update L48 to reflect the new `spawn_process_collector` shape; bump `Last verified:`.
- `backend/crates/atc-server/CLAUDE.md` — fix the stale lag-semantics claim at L46 while the file is being touched; bump `Last verified:`.
- `backend/crates/atc-core/CLAUDE.md` — verify; update if it makes stale supervision claims; bump `Last verified:` if touched.
- `backend/crates/atc-core/Cargo.toml` — add `tokio-util` (no features needed).
- `backend/crates/atc-server/Cargo.toml` — add the `rt` feature to `tokio-util` (currently no features). `TaskTracker` is gated behind `rt`.
- `backend/crates/atc-server/tests/ws_tests.rs` — fix the lag-test inconsistency at `:282` (test expects continued delivery; production code closes on `Lagged`).
- `scripts/doc-mapping.sh` — verify existing mappings still cover the changed files; no new entry expected.

## Implementation Guidance

Rules from `docs/implementation-guidance.md` that apply to this scope:

- Rule 1 — feature branch; squash-merge PR title for the full deliverable; test plan in the PR's first comment.
- Rule 2 — TDD with pragmatic pairing: each phase above pairs failing tests with the minimum implementation that makes them compile and run. This is not a relaxation of TDD; it acknowledges Rust's whole-crate compile model.
- Rule 3 — never pin library versions; `cargo add tokio-util` (no `--features` unless the compiler asks).
- Rule 4 — `scripts/doc-mapping.sh` check (no new mapping expected).
- Rule 7 — Rust test-file split rule applies if any new test file exceeds ~500 lines. The new `graceful_shutdown.rs` integration test is unlikely to exceed that.
- Rule 14 — orchestrating context delegates implementation to subagents; main context dispatches and reviews.
- Rule 16 — `ed3d-research-agents:*` for any further investigation during implementation.
- Rule 17 — strip planning-artifact labels: no `phase_NX_*`, `ac<N>_*`, `t<N>_*` test names; no `// AC6: …` comments. Behavioral test names only.

Project-memory items that bite:

- `feedback_dont_skip_runtime_verification.md` — Phase 6's manual SIGTERM test is mandatory, not optional.
- `feedback_no_source_grep_tests.md` — AC5 ("no cancellation-token checks inside `drain_pass`") is verified by code review of the diff, not by a grep-based test.
- `feedback_dont_assume_dep_minimalism.md` — adding `tokio-util` to `atc-core` is justified by the supervision pattern.
- `feedback_pr_title_convention.md`, `feedback_pr_body_convention.md`, `feedback_test_plans.md` — squash-merge title for the full deliverable; PR body as "what will be implemented" at design time, updated at PR time; test plan as the first PR comment.
- `feedback_verify_lefthook_installed.md` — run `just setup` at the start of the implementation session.
- `feedback_verify_invariant_layer.md` — this plan was rewritten after a primary-source audit corrected an earlier misreading of axum's WS upgrade lifecycle. The corrected mental model (WS spawned tasks are not in axum's connection accounting) governs the design; verify any further claims against the same primary sources rather than against generalized expectations.
- `feedback_codex_review_before_exit.md` — codex `xhigh` review was run on a prior draft of this plan and produced four blockers; this revision folds them in.
- `feedback_run_e2e_tests_for_frontend_changes.md` — does not apply; this scope is backend-only. The frontend's `connection.ts` is unchanged in this work.

## Out of Scope

- A tagged `ServerMessage` envelope with `Event(SeqEvent) | GoingAway | …` variants. Deferred to issue #47, which has its own design questions. Until #47 lands, the broadcast channel carries only `SeqEvent`, and the going-away signal rides on RFC 6455's Close 1001.
- Replacing `metrics-process` with a different metric source.
- A `SupervisedTask` abstraction. See Architecture § "Why not extract a `SupervisedTask` helper?".
- Mid-pagination cancellation inside `drain_pass()`. Locked decision.
- Frontend reconnection behavior changes. Existing `connection.ts` flow handles disconnect/reconnect; an optional `event.code === 1001` branch in `onclose` for diagnostics is a follow-up, not part of this PR.
- Signal-injection test infrastructure. Tests use explicit `CancellationToken::cancel()`.
- Per-task graceful-shutdown metrics (`atc_shutdown_task_duration_seconds` histogram). Could be a follow-up if observability needs it; not required for this issue.
- K8s deployment-side polish: a `preStop` lifecycle hook to delay SIGTERM by ~5 s for load-balancer de-registration, and a "shutting-down" readiness-probe state to fail readiness immediately for faster LB drain. These touch `deploy/helm/atc/templates/deployment.yaml` and live one layer above pod-internal supervision; tracked as a separate (small) follow-up issue.
- Supervising the `metrics_exporter_prometheus` upkeep loop spawned at `metrics.rs:61`. That loop is owned by the library, with no public API to inject a `CancellationToken`. The runtime drops it at process exit; no operational concern at ATC's scale.
- An integration-test assertion that "every drain-broadcast event reaches the client before Close 1001." The biased select gives this property at the WS handler level on the clean path; deterministic end-to-end assertion would need a synchronization barrier outside the design's scope.

## Glossary

- **Cooperative shutdown** — a task observes a cancellation signal and chooses an exit point of its own. Opposite of `.abort()`, which terminates a task at its next await point regardless of work in progress, and is asynchronous (the task may continue past the abort call until that next await point).
- **Drain pass** — one execution of `drain_pass()` (`backend/crates/atc-server/src/listener.rs:314-437`), which paginates through outbox rows in 500-row batches until a partial page indicates the backlog is exhausted.
- **Cancellation surface** — a place in the codebase where a long-lived task or connection observes (or fails to observe) a `CancellationToken`.
- **Phase 1 / phase 2** — the two cancellation tokens. Phase 1 (`shutdown`) signals tasks and axum to stop accepting and finish in-flight work; phase 2 (`ws_close`) signals WS handlers to send Close 1001 and exit. Phase 2 fires only after phase 1's drain handle has been awaited.
- **`TaskTracker`** — `tokio_util::task::TaskTracker`, a cheaply-cloneable handle that counts tracked futures. Used here so `main` can wait for the spawned WS handler tasks to flush their Close frames before process exit.
- **Biased select** — `tokio::select! { biased; … }`. Evaluates arms top-down rather than randomly when multiple are ready, used here so buffered broadcast events are delivered to the WS client before the cancellation arm fires.
