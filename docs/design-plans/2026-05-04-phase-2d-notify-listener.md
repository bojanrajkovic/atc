# Phase 2d — NOTIFY Emission + Listener Stub

> **Slug:** `phase-2d-notify-listener` (final filename when copied: `2026-05-04-phase-2d-notify-listener.md`)
> **Branch hint:** `feat/phase-2d-notify-listener`
> **PR title (squash):** `feat(server): add LISTEN/NOTIFY end-to-end with listener fetch-and-log stub`
> **Plan author:** Claude (Sonnet 4.6), 2026-05-04 — codex-reviewed and revised
> **Phase reference:** [`docs/architecture/state-externalization-research/rollout-and-implementation.md` § Phase 2d](../../Projects/atc/docs/architecture/state-externalization-research/rollout-and-implementation.md)
> **Implementation guidance:** Per `docs/implementation-guidance.md` (governs all execution-time work for this plan).

---

## Summary

Wire up PostgreSQL `LISTEN/NOTIFY` end-to-end so the next phase (3c) can drop in WebSocket forwarding as a one-line change. The webhook handler emits `pg_notify('atc_outbox', seq::text)` inside the same transaction that wrote the outbox row. A pair of background tasks per replica receive notifications, coalesce wake-ups via `tokio::sync::Notify`, drain unread outbox rows by `seq > last_forwarded_seq ORDER BY seq`, log them, and advance a local watermark — but do not yet forward to WebSocket clients. The watermark initializes to `MAX(seq)` at boot per ADR 0002 Decision 5. A new optional `ATC_DATABASE_LISTENER_URL` env var (with matching Helm chart wiring) lets operators point the listener at a session-mode endpoint when the main pool runs through transaction-mode PgBouncer.

After Phase 2d:
- Every committed webhook produces a NOTIFY visible to listeners.
- Each replica's listener task drains outbox rows by seq order on each NOTIFY and logs them.
- WS clients still receive `SeqEvent`s broadcast from the in-memory path with `Mutex<u64>` seq, exactly as in Phase 2c. Phase 3c retires the in-memory broadcast and substitutes the outbox-driven path.

## Definition of Done

1. **Write side** — webhook handler emits a NOTIFY for every committed outbox row, atomic with the transaction. No NOTIFY on rollback.
2. **Read side** — both background tasks (listener + drain) run on every replica when `pg_pool.is_some()`, observe shutdown cleanly, and survive transient PG connection loss via sqlx's internal reconnect.
3. **Fetch behavior** — drain task fetches outbox rows by `seq > watermark ORDER BY seq` on each wake-up, logs them, and advances the watermark. Watermark initialized to `MAX(seq)` at boot.
4. **Operator surface** — `ATC_DATABASE_LISTENER_URL` is settable both via env var and via the Helm chart (plain value or `existingSecret`). Operator contract documented in `docs/architecture/deployment.md` and `docs/architecture/backend-server.md`.
5. **Acceptance** — every AC below has a passing test. CI green on the feature branch.
6. **Documentation** — `backend-server.md` Webhook Handler / AppState / Lifecycle Wiring / Metrics / Files sections updated in place; `deployment.md` includes the new env var; ADR 0002 implementation status reflects 2d completion; rollout-and-implementation.md marks 2d done.

**Out of Scope** (do NOT pull forward — see § Out of Scope at the end for the full table):
- WebSocket forwarding of fetched outbox rows → Phase 3c
- Retiring the in-memory `StateStore` and `Mutex<u64>` cursor → Phase 3c
- `/readyz` listener-health integration → Phase 3c
- Cursor rename `seq → lastSeq` → Phase 3a
- `pool_stats_after` removal from `SeqEvent` → Phase 3b
- Snapshot read from PG → Phase 3c
- Helm `replicaCount > 1` gating → Phase 4

ADR refs: [ADR 0002 Decision 3](../../Projects/atc/docs/architecture-decisions/0002-state-externalization-postgres-outbox.md) (NOTIFY + session-mode connection), [ADR 0002 Decision 5](../../Projects/atc/docs/architecture-decisions/0002-state-externalization-postgres-outbox.md) (forwarder design + startup watermark + level-triggered drain), [ADR 0003 Decision 2](../../Projects/atc/docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (strictly monotonic, not gapless seq cursor).

---

## Locked Decisions

These were settled during planning and codex review. Do **not** reopen during implementation; if a constraint emerges that contradicts one of these, stop and revise the plan.

### D1 — NOTIFY placement: inside the transaction

Emit `SELECT pg_notify($1::text, $2::text)` inside the same transaction as the outbox INSERT, immediately after the `insert_outbox_*_in_txn` call and before `tx.commit()`. The seq returned by the outbox insert (currently discarded via `.map(|_| ())` in `routes.rs:213,223`) is bound as the payload.

PG queues NOTIFYs from inside a txn and delivers them only on COMMIT. Aborted txns silently drop the NOTIFY (correct: no outbox row → no notification). Eliminates the post-commit-but-pre-NOTIFY crash window.

> **Note for doc reconciliation.** `rollout-and-implementation.md:100` says "Webhook handler emits NOTIFY after commit." This is functionally equivalent for the success path because PG queues NOTIFYs until COMMIT — wording refers to delivery semantics, not the API call site. Phase E of this plan clarifies the wording.

### D2 — Listener connection: own connection, configurable DSN, fully wired through Helm

Use `sqlx::postgres::PgListener::connect(&listener_url).await` (NOT `connect_with(&pool)` which permanently consumes a slot from the main pool).

**Config struct change:**

```rust
// backend/crates/atc-server/src/config.rs
pub struct Config {
    pub database_url: Option<String>,
    pub database_listener_url: Option<String>, // NEW; falls back to database_url
    // ...existing fields
}
```

**Helm chart wiring (Files to Modify includes the chart):**

- `deploy/helm/atc/values.yaml` — under `config:` add `databaseListenerUrl: ""` (empty string default → unset env). Under `existingSecret:` add `databaseListenerUrlKey: ""` so secret-based deployments can supply a distinct key.
- `deploy/helm/atc/values.schema.json` — extend the `config` and `existingSecret` schemas with the new fields (string type; optional).
- `deploy/helm/atc/templates/deployment.yaml` — add a parallel env block for `ATC_DATABASE_LISTENER_URL` mirroring the existing `ATC_DATABASE_URL` pattern (lines 69–78). Both `existingSecret` (with `databaseListenerUrlKey`) and plain `config.databaseListenerUrl` paths supported. Skip the env entry entirely when neither is set, so the Rust `Option<String>` fallback to `database_url` works.
- `deploy/helm/atc/tests/values-*.yaml` — at least one fixture exercises `databaseListenerUrl`; one exercises `existingSecret.databaseListenerUrlKey`.
- `docs/architecture/deployment.md` — document the new env var, when to set it (transaction-mode PgBouncer for the main pool), and the existingSecret path.

**Operator contract** (documented in both `backend-server.md` and `deployment.md`): the listener DSN MUST be session-mode compatible (direct PG or session-mode PgBouncer). Transaction-mode PgBouncer breaks `LISTEN`. Default behavior: listener uses the same DSN as the main pool. Operators with transaction-mode pooling for the main pool override `ATC_DATABASE_LISTENER_URL` to point at a session-mode endpoint.

### D3 — Coalescing: two tasks + `Arc<tokio::sync::Notify>`

Two background tasks. Importantly, **the `PgListener` is fully initialized in main.rs before the HTTP server starts accepting webhooks** — this prevents the early-notification race where a webhook fires between server bind and `LISTEN` registration.

**main.rs sequence:**

```text
(1) Build pg_pool.
(2) If pg_pool.is_some():
    (a) Compute listener_url = cfg.database_listener_url.or(database_url).unwrap();
    (b) Connect: let mut listener = PgListener::connect(&listener_url).await
        .unwrap_or_else(|e| { tracing::error!(...); std::process::exit(1); });
    (c) Subscribe: listener.listen(NOTIFY_CHANNEL).await
        .unwrap_or_else(|e| { tracing::error!(...); std::process::exit(1); });
    (d) Init watermark: let watermark = sqlx::query_scalar!("SELECT COALESCE(MAX(seq), 0) FROM outbox")
        .fetch_one(&pool).await
        .unwrap_or_else(|e| { ...; exit(1); });
    (e) Spawn listener task and drain task with the now-ready listener and watermark.
(3) Build router, bind server.
```

Once both tasks are spawned and the listener is registered, the HTTP server binds. There is no window where a webhook commit can produce a NOTIFY that is not delivered to the listener.

**Listener task** (in `listener.rs`):
```text
loop {
  tokio::select! {
    _ = shutdown.cancelled() => break,
    res = listener.recv() => match res {
      Ok(_notification) => {
        atc_pg_notify_received_total.increment(1);
        if let Some(c) = observed_recv.as_ref() { c.fetch_add(1, Relaxed); }
        notify.notify_one();
      }
      Err(e) => {
        atc_pg_listener_recv_errors_total.increment(1);
        tracing::warn!(error = %e, "pg listener recv error");
        tokio::time::sleep(Duration::from_secs(1)).await;
      }
    }
  }
}
```

**Drain task** (in `listener.rs`): owns the watermark as a local variable; tests inject the optional `drain_started: Option<Arc<Notify>>` baseline-signal hook (see AC refinements below).

```text
let mut watermark: i64 = initial_watermark;
loop {
  if let Some(s) = drain_started.as_ref() { s.notify_one(); }   // test hook: signal pass entry
  drain_pass(&pool, &mut watermark, observed_passes.as_ref()).await;
  tokio::select! {
    _ = shutdown.cancelled() => break,
    _ = notify.notified() => {}
  }
}
```

`drain_pass()` body:
```text
let rows = sqlx::query!("SELECT seq, kind, run_id, job_id, payload \
                        FROM outbox WHERE seq > $1 ORDER BY seq", *watermark)
    .fetch_all(pool).await?;
for row in &rows {
    tracing::info!(seq = row.seq, kind = %row.kind, run_id = row.run_id,
                   job_id = ?row.job_id, "outbox drain (stub: not forwarding)");
}
if let Some(last) = rows.last() { *watermark = last.seq; }
if let Some(c) = observed_passes { c.fetch_add(1, Relaxed); }
atc_pg_drain_rows_total.increment(rows.len() as u64);
atc_pg_drain_passes_total.increment(1);
```

`Notify` holds at most one permit. N notifications during a slow drain collapse to a single permit; the drainer wakes once afterwards and re-drains. Phase 3c adds `forward_to_ws_clients(&row).await` inside the for-loop and retires the in-memory broadcast — that's the only diff for the WS pivot.

### D4 — Watermark init: in 2d (per ADR 0002 D5)

The watermark initializes to `COALESCE(MAX(seq), 0)` at boot — directly per ADR 0002 Decision 5 ("Replica startup watermark"). Owned as a local `i64` in the drain task. No AppState change.

> **Codex disposition note.** An earlier draft of this plan deferred watermark init to Phase 3c. That is wrong: the rollout doc and ADR D5 both place watermark init in 2d as part of the level-triggered drain stub. Phase 3c only adds the `forward_to_ws_clients` call, not the watermark.

### D5 — Channel name: hardcoded constant

```rust
// backend/crates/atc-server/src/listener.rs
pub(crate) const NOTIFY_CHANNEL: &str = "atc_outbox";
```

Per-database scope (PG default). Referenced by both writer (`persist::notify_outbox_seq_in_txn`) and listener task. A unit test asserts `NOTIFY_CHANNEL == "atc_outbox"` to catch accidental rename in PR review.

### D6 — Reconnection: rely on sqlx, log + sleep on `recv()` Err

`PgListener::recv()` auto-reconnects internally and re-LISTENs on every previously subscribed channel. **Successful transient reconnects do NOT surface as `Err`** — they happen silently inside `recv()`. Only irrecoverable errors leak through. On `Err`, the listener task logs, increments `atc_pg_listener_recv_errors_total`, sleeps 1s, and continues the loop.

> **Documented in module comment.** Notifications received during the brief disconnect→re-LISTEN window are silently dropped at the DB level (sqlx-postgres-0.8.6/src/listener.rs:202). The watermark + `seq > last_forwarded_seq` drain heals the gap on the next NOTIFY because the listener will then catch up by SELECTing every row newer than the watermark.

No exponential backoff in 2d. Phase 5 hardening can revisit if a real outage produces a tight loop.

### D7 — Listener startup failure: fail-fast

If `pg_pool.is_some()` and any of (`PgListener::connect`, `listen()`, watermark `SELECT MAX(seq)`) fails at boot, log error and `std::process::exit(1)`. Symmetric with main pool init at `main.rs:69-76`.

Runtime failures (post-startup) are handled by D6 (auto-reconnect via sqlx). Phase 3c will add `/readyz` listener-health integration when the listener becomes load-bearing for forwarding — that's where degraded vs. healthy starts to matter for cluster routing. **Updated in `rollout-and-implementation.md` Phase 3c section** so the deferral isn't lost between phases.

### D8 — `notify_outbox_seq_in_txn` helper in `persist.rs`

Add a third helper next to the existing `insert_outbox_*_in_txn` functions:

```rust
pub(crate) async fn notify_outbox_seq_in_txn(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    seq: i64,
) -> Result<(), PersistError> {
    sqlx::query!("SELECT pg_notify($1::text, $2::text)",
                 crate::listener::NOTIFY_CHANNEL,
                 seq.to_string())
        .execute(&mut **tx).await
        .map_err(|e| PersistError::Backend(Box::new(e)))?;
    Ok(())
}
```

Rationale: matches the established `*_in_txn` pattern; sqlx offline cache entry generated alongside the other helpers in `backend/.sqlx/`; route handler delegates rather than open-coding SQL.

### D9 — Metrics: `register_listener_metrics()`

In `backend/crates/atc-server/src/metrics.rs`:

```rust
pub fn register_listener_metrics() {
    metrics::describe_counter!(
        "atc_pg_notify_emitted_total",
        "Notifications emitted from the webhook handler, by event kind"
    );
    metrics::describe_counter!(
        "atc_pg_notify_received_total",
        "Notifications received by the listener task"
    );
    metrics::describe_counter!(
        "atc_pg_listener_recv_errors_total",
        "Listener task recv() error events (sqlx reconnects internally; this counts irrecoverable surfacings)"
    );
    metrics::describe_counter!(
        "atc_pg_drain_passes_total",
        "Drain task pass count (one per wake-up)"
    );
    metrics::describe_counter!(
        "atc_pg_drain_rows_total",
        "Total outbox rows fetched by the drain task across all passes"
    );
}
```

Called from `main.rs` after `register_pg_write_counters()`.

`atc_pg_notify_emitted_total{kind="run"|"job"}` is incremented in the route handler **after `tx.commit()` succeeds**. `atc_pg_notify_received_total` and `atc_pg_listener_recv_errors_total` are incremented in the listener task. `atc_pg_drain_passes_total` and `atc_pg_drain_rows_total` are incremented in the drain task.

> **Codex disposition note.** Counter renamed from `atc_pg_listener_reconnects_total` because sqlx hides successful reconnects from `recv()`; the counter actually measures `recv()` error escapes, not reconnect events.

### D10 — Test fixture refactor: lift `start_pg` to `tests/common/mod.rs`

`start_pg` is currently duplicated in **3 test files** (`outbox_tests.rs`, `persist_pg_tests.rs`, `transactional_writes_tests.rs`). `db_readyz_tests.rs` has different inline container setup and does not define `start_pg`. Lift to `tests/common/mod.rs` with the signature:

```rust
pub async fn start_pg() -> (sqlx::PgPool, impl Drop, String /* db_url */) { ... }
```

Update the 3 test files to call `common::start_pg()`. Add a sibling fixture for Phase 2d:

```rust
pub struct AppFixture {
    pub router: Router,
    pub state: Arc<AppState>,
    pub broadcast_rx: broadcast::Receiver<SeqEvent>,
    pub listener_handle: JoinHandle<()>,
    pub drain_handle: JoinHandle<()>,
    pub observed_recv: Arc<AtomicU64>,
    pub observed_passes: Arc<AtomicU64>,
    pub drain_started: Arc<Notify>,
    pub shutdown: CancellationToken,
    pub db_url: String,
}
pub async fn build_app_with_pg_and_listener(...) -> AppFixture { ... }
```

The `drain_started` `Arc<Notify>` is the test-only baseline-signal hook used by AC4–AC7 for delta-from-baseline assertions (see AC refinements below).

### D11 — `start_pg` is the only test fixture lifted in this PR

`db_readyz_tests.rs` has different inline setup (it tests pool init failure modes by passing bad URLs). Don't refactor it in this PR; that's a separate cleanup. Leaving the inline setup keeps the 2d diff narrow.

---

## Files to Modify

### Production code
- `backend/crates/atc-server/src/listener.rs` — **NEW.** `NOTIFY_CHANNEL` const, `spawn_listener_task`, `spawn_drain_task` (both take a fully-initialized `PgListener` / pool, both return `JoinHandle<()>`), module doc with reconnect-loss-window note (D6) and drain-loop-ordering note.
- `backend/crates/atc-server/src/lib.rs` — add `pub mod listener;`.
- `backend/crates/atc-server/src/persist.rs` — add `notify_outbox_seq_in_txn` helper (D8).
- `backend/crates/atc-server/src/routes.rs` — bind seq from outbox helpers (lines 213, 223 → `let seq = ...?;`); call `notify_outbox_seq_in_txn` inside the txn before `tx.commit()`; increment `atc_pg_notify_emitted_total{kind}` after commit succeeds.
- `backend/crates/atc-server/src/state.rs` — no field changes (listener handles and watermark live in main.rs / drain task scope, not AppState).
- `backend/crates/atc-server/src/config.rs` — add `database_listener_url: Option<String>` reading `ATC_DATABASE_LISTENER_URL`.
- `backend/crates/atc-server/src/metrics.rs` — `register_listener_metrics()` (D9).
- `backend/crates/atc-server/src/main.rs` — call `register_listener_metrics()`; when `pg_pool.is_some()`, derive `listener_url`, connect+listen+init-watermark synchronously (fail-fast on Err), spawn both tasks with `shutdown.clone()`, retain handles, abort on shutdown. **Sequence per D3:** all listener init completes before `axum::serve` binds.
- `backend/.sqlx/` — regenerated via `cargo sqlx prepare --workspace -- --tests` after adding the `pg_notify`, `MAX(seq)`, and drain `SELECT` queries.

### Helm chart (per B2 fix)
- `deploy/helm/atc/values.yaml` — add `config.databaseListenerUrl: ""` and `existingSecret.databaseListenerUrlKey: ""`.
- `deploy/helm/atc/values.schema.json` — extend schemas for the new fields.
- `deploy/helm/atc/templates/deployment.yaml` — parallel env block for `ATC_DATABASE_LISTENER_URL` (mirroring the `ATC_DATABASE_URL` block at lines 69–78), supporting both existingSecret and plain-value paths; emits the env entry only when one of the two paths is configured.
- `deploy/helm/atc/tests/values-*.yaml` — at least one fixture exercises `databaseListenerUrl`; one exercises `existingSecret.databaseListenerUrlKey` (in a new fixture or an additive change to existing ones).

### Tests
- `backend/crates/atc-server/tests/common/mod.rs` — lift `start_pg` returning URL; add `AppFixture` + `build_app_with_pg_and_listener` (D10).
- `backend/crates/atc-server/tests/outbox_tests.rs`, `tests/persist_pg_tests.rs`, `tests/transactional_writes_tests.rs` — switch to `common::start_pg()`; remove local copies. (`tests/db_readyz_tests.rs` left as-is per D11.)
- `backend/crates/atc-server/tests/notify_listener_tests.rs` — **NEW.** All ACs.
- `backend/crates/atc-server/tests/listener_unit_tests.rs` (or a `#[cfg(test)] mod tests` inside `listener.rs`) — unit test asserting `NOTIFY_CHANNEL == "atc_outbox"`.

---

## Implementation Phases

Five phases. Each is a meaningful checkpoint, runnable, and reviewable in isolation.

### Phase A — Module scaffolding + config + metrics

- Create `listener.rs` with `NOTIFY_CHANNEL` const, function signatures stubbed `unimplemented!()`, module doc.
- Wire `pub mod listener;` in `lib.rs`.
- Add `database_listener_url` field to `Config` reading `ATC_DATABASE_LISTENER_URL`.
- Add `register_listener_metrics()` in `metrics.rs`; call from `main.rs`.

**Verification:** `cargo build -p atc-server` clean; `cargo clippy -p atc-server -- -D warnings` clean; existing test suite passes unchanged.

### Phase B — Webhook NOTIFY emission (write side)

- Add `notify_outbox_seq_in_txn` helper in `persist.rs`.
- Modify `routes.rs:206-228`: bind seq from outbox helper, call `notify_outbox_seq_in_txn` inside the txn, increment `atc_pg_notify_emitted_total{kind}` after `tx.commit()` succeeds.
- Regenerate `backend/.sqlx/` cache (per `CONTRIBUTING.md:249-261`).

**Verification:** All existing `outbox_tests` and `transactional_writes_tests` continue to pass (txn structure unchanged for them — they don't observe NOTIFYs). `SQLX_OFFLINE=true cargo build -p atc-server` clean. Manual smoke: out-of-band `psql -d $DSN` (interactive session, kept open) running `LISTEN atc_outbox;`; fire webhook in another terminal; observe NOTIFY arrives with seq payload.

### Phase C — Listener + drain task implementation (read side)

- Implement `spawn_listener_task(listener: PgListener, notify, shutdown, observed_recv) -> JoinHandle<()>`. Pre-condition: caller has already called `connect()` and `listen()` on the listener — see D3. Body is the recv loop.
- Implement `spawn_drain_task(pool, initial_watermark, notify, shutdown, observed_passes, drain_started) -> JoinHandle<()>`. Body is the drain-then-wait loop with the SQL fetch + log + watermark advance.
- Wire into `main.rs`:
  - `let listener_url = cfg.database_listener_url.clone().or_else(|| cfg.database_url.clone()).expect(...);`
  - `let mut listener = PgListener::connect(&listener_url).await.unwrap_or_else(|e| { exit(1); });`
  - `listener.listen(NOTIFY_CHANNEL).await.unwrap_or_else(|e| { exit(1); });`
  - `let initial_watermark = sqlx::query_scalar!("SELECT COALESCE(MAX(seq), 0) FROM outbox").fetch_one(&pool).await.unwrap_or_else(|e| { exit(1); });`
  - `let notify = Arc::new(Notify::new());`
  - Spawn both tasks. Retain handles.
  - Then build router and bind `axum::serve`.
- After `tokio::select!` on the servers exits, abort/await both handles within a budget. Listener task uses `tokio::select!` against shutdown so `PgListener::Drop` runs UNLISTEN cleanly.

**Verification:** Manual smoke against PG: server boots clean, post a webhook, `curl :9090/metrics` shows `atc_pg_notify_emitted_total{kind="run"} 1`, `atc_pg_notify_received_total 1`, `atc_pg_drain_passes_total ≥ 1`, `atc_pg_drain_rows_total ≥ 1`. Server logs include the row from the drain task. SIGTERM exits within ~1s. In-memory mode (`ATC_DATABASE_URL` unset): server starts, no listener spawned, no `atc_pg_*` counters appear in `/metrics`.

### Phase D — Test infrastructure + integration tests

- Lift `start_pg` to `tests/common/mod.rs` returning URL (D10). Update 3 test files.
- Add `AppFixture` + `build_app_with_pg_and_listener` helper.
- Write `tests/notify_listener_tests.rs` covering all ACs (see below).

**Verification:** `cargo test -p atc-server` passes. `cargo test -p atc-server --test notify_listener_tests` passes in isolation. `SQLX_OFFLINE=true cargo build -p atc-server` clean.

### Phase E — Helm chart wiring + documentation

- Helm chart changes per D2 (values.yaml, values.schema.json, deployment.yaml template, test fixtures).
- `helm lint deploy/helm/atc` and `helm-unittest` clean. `helm template` exercises both env-set and env-unset paths.
- Update `docs/architecture/backend-server.md` **in place** in the relevant existing sections (per codex I5):
  - **Webhook Handler** section: add "and emits `pg_notify('atc_outbox', seq)` inside the txn before commit" to the flow description.
  - **AppState** section: add a sentence noting the listener task receives a clone of the existing `CancellationToken` for shutdown; AppState gets no new fields.
  - **Lifecycle Wiring** section: add the listener init sequence (connect → listen → MAX(seq) → spawn) before `axum::serve`. Document fail-fast on listener init errors.
  - **Metrics** section: add the new counters with descriptions.
  - **Files** section: add `src/listener.rs`.
  - New small subsection "NOTIFY emission and listener fetch-stub" capturing the two-task coalescing structure, the DSN session-mode contract, and the reconnect-loss-window note.
  - Bump `Last verified`.
- Update `backend/crates/atc-server/CLAUDE.md`: add `listener` to Module table; add NOTIFY emission and listener-task rows to Contracts. Bump `Last verified`.
- Update `docs/architecture/deployment.md`: document `ATC_DATABASE_LISTENER_URL` (env var name, when to set, default fallback to `ATC_DATABASE_URL`, plain-value vs. existingSecret paths). Bump `Last verified`.
- Update `docs/architecture/state-externalization-research/rollout-and-implementation.md`:
  - Clarify line 100 wording (delivery semantics).
  - Mark `ATC_DATABASE_LISTENER_URL` (line 102) IMPLEMENTED in 2d.
  - Mark Phase 2d items DONE (mirror Phase 2c structure at lines 60–72).
  - Add Phase 3c section note: "When the drain task gains WebSocket forwarding, it becomes load-bearing for cluster routing; at that point extend `/readyz` to reflect listener health (mechanism is a 3c design decision)."
  - Bump `Last verified`.
- Update `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md`: Implementation Status — Decision 3 (NOTIFY + session-mode connection) implemented; Decision 5 (forwarder structure + watermark init) implemented; **only the `forward_to_ws_clients` step of Decision 5 remains for Phase 3c**. Cross-link PR.

**Verification:** `scripts/check-docs-lefthook.sh` passes. `helm lint deploy/helm/atc` + `helm-unittest` clean. Cross-reference scan: ADR ↔ rollout doc ↔ backend-server.md ↔ deployment.md ↔ CLAUDE.md all consistent.

---

## Acceptance Criteria

Each criterion is named `phase_2d_notify_listener_ac<N>_<short_description>`. AC4–AC7 capture a baseline immediately after the test fixture finishes startup (drain task has signaled `drain_started` for the first pass and the test waits for that signal before measuring), then assert deltas from that baseline — see codex I2 fix.

### AC1: NOTIFY fires on commit
Out-of-band `PgListener` (constructed by the test, not the in-process listener task) subscribed to `atc_outbox` BEFORE the webhook fire. Fire one `workflow_run` and one `workflow_job` webhook. Receive exactly 2 notifications, payloads parse as `i64` and match the seq returned by the outbox INSERT for each.

### AC2: NOTIFY does NOT fire on rollback
Out-of-band `PgListener` subscribed BEFORE a parity-rejecting webhook (Completed→Requested setup mirroring `outbox_tests::phase_2c_outbox_ac2_1_*`). Assert `tokio::time::timeout(Duration::from_secs(1), listener.recv())` returns `Err(_)` — no notification received because the txn rolled back.

### AC3: NOTIFY does NOT fire when `pg_pool: None`
Extend `outbox_tests::phase_2c_outbox_ac3_5_no_pg_pool_uses_in_memory_path` to assert `atc_pg_notify_emitted_total` Prometheus counter is absent / zero after the webhook fires in in-memory-only mode. (Out-of-band listener not required since there's no DB to listen on.)

### AC4: Listener task receives all N notifications
After fixture startup, capture `baseline_recv = observed_recv.load(Ordering::Relaxed)`. Fire N=10 sequential `workflow_run` webhooks. Assert `observed_recv.load() == baseline_recv + 10` within a 2s budget.

### AC5: Listener task observes shutdown
After firing the cancellation token, `listener_handle.is_finished()` returns true within a 500ms budget. Verifies the `tokio::select!` against `shutdown.cancelled()` is wired correctly.

### AC6: Drain task fetches and advances watermark
After fixture startup, capture `baseline_passes = observed_passes.load()` and `baseline_rows = read_metric("atc_pg_drain_rows_total")`. Fire N=5 sequential webhooks (one row each). Assert `observed_passes.load() >= baseline_passes + 5` (one pass per wake-up) AND `read_metric("atc_pg_drain_rows_total") >= baseline_rows + 5` (5 rows fetched total). Query `SELECT MAX(seq) FROM outbox` directly and assert the in-process watermark is at most lagging by one wake-up cycle (drain task exposes a test-only watermark accessor, or the assertion polls metrics until the row count matches).

### AC7: Coalescing works (multi-notification during in-flight pass)
The drain task is configured to sleep for 50ms inside `drain_pass` for this test only (via a test-only `Duration` parameter exposed on `spawn_drain_task` or set through `AppFixture`). Capture baseline. Fire 4 NOTIFYs in rapid succession during the in-flight pass. Assert `observed_passes.load() - baseline_passes ∈ [2, 3]` within a 5s budget (in-flight pass + at least one trailing coalesced pass; depending on scheduling, may include one more if the trailing pass was already in progress when later NOTIFYs fired). Strict equality is racy; the bounded interval is the deterministic property.

### AC8: NOTIFY payload is the seq token (not the event body)
For each notification received in AC1, parse the payload string back to `i64` and match against the seq column in the corresponding outbox row queried directly via `SELECT seq FROM outbox WHERE run_id = $1 ORDER BY seq DESC LIMIT 1`.

### AC9: Channel name is `atc_outbox`
- Out-of-band listener LISTENs on `"atc_outbox"` and receives notifications, validating the writer emits on the same channel.
- Unit test in `listener.rs`: `assert_eq!(NOTIFY_CHANNEL, "atc_outbox");` (catches accidental rename in PR review).

### AC10: sqlx offline cache up to date
`SQLX_OFFLINE=true cargo build -p atc-server` succeeds. `git diff --stat backend/.sqlx/` shows new query files for the `pg_notify`, `MAX(seq)`, and drain `SELECT` queries.

### AC11: Listener fail-fast at startup (direct seam)
Test calls `PgListener::connect("postgres://nope:nope@127.0.0.1:1/x").await` directly and asserts `Err(_)`. Then assert that the test version of the lifecycle init helper (extracted from main.rs as a small library function for testability) propagates the error to the caller (which would then `exit(1)` in production main). Pure library-seam test; no binary spawning.

### AC12: Shutdown completeness
After cancellation token fires, both `listener_handle.is_finished()` and `drain_handle.is_finished()` return true within 2s.

### AC13: Watermark initialized to MAX(seq) at startup
Pre-seed the outbox with N=3 rows (via direct `INSERT INTO outbox`). Build the fixture (which initializes the watermark). Assert the drain task's first pass after startup fetches **0 rows** — because the watermark has caught up to `MAX(seq) = 3` and there are no newer rows. Then fire one webhook; assert the next drain pass fetches exactly 1 row.

### AC14: Helm chart exposes the new env var
Helm-unittest assertions:
- `config.databaseListenerUrl: "postgres://..."` produces `ATC_DATABASE_LISTENER_URL` env entry on the deployment.
- `existingSecret.name + existingSecret.databaseListenerUrlKey: "..."` produces a `valueFrom.secretKeyRef` entry.
- Neither set: no `ATC_DATABASE_LISTENER_URL` env entry is rendered (Rust falls back to `ATC_DATABASE_URL`).
- Both `config.databaseListenerUrl` and `existingSecret.databaseListenerUrlKey` set: the existingSecret path wins (mirroring the existing `ATC_DATABASE_URL` precedence).

---

## Verification

Canonical local-PG invocation per `CONTRIBUTING.md:249-261`. The justfile does NOT expose a `db-up` recipe. Bring up PG via `docker run` directly, and apply migrations explicitly.

```bash
# 1. Bring up local PG, apply migrations, regenerate offline sqlx cache.
docker run -d --rm --name atc-pg -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:17-alpine
DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
  cargo sqlx migrate run --source backend/crates/atc-server/migrations
(cd backend && DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
  cargo sqlx prepare --workspace -- --tests)
git status backend/.sqlx/                                          # expect new query files

# 2. Full test sweep (Docker required — testcontainers boots ephemeral PG per test).
just test

# 3. Targeted backend tests.
cargo test -p atc-server --test notify_listener_tests
cargo test -p atc-server --test outbox_tests
cargo test -p atc-server --test transactional_writes_tests
cargo test -p atc-server --test persist_pg_tests
cargo test -p atc-server --test db_readyz_tests

# 4. Lint.
cargo clippy -p atc-server -- -D warnings
cargo clippy -p atc-core   -- -D warnings

# 5. Offline build (no DB available, exercises sqlx cache).
SQLX_OFFLINE=true cargo build -p atc-server

# 6. Doc-staleness gate.
scripts/check-docs-lefthook.sh

# 7. Helm gates.
helm lint deploy/helm/atc
just helm-unittest
just helm-check

# 8. In-memory-only mode smoke (verify listener NOT spawned).
env -u ATC_DATABASE_URL cargo run -p atc-server &
SERVER_PID=$!
sleep 2
curl -X POST http://127.0.0.1:8080/v1/webhooks/github -H "X-GitHub-Event: workflow_run" -d '...'
curl http://127.0.0.1:9090/metrics | grep -E 'atc_pg_(notify|listener|drain)'   # expect no matches
kill $SERVER_PID

# 9. Connected-mode smoke. Open psql in interactive mode (do NOT use -c, which exits immediately).
ATC_DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" cargo run -p atc-server &
SERVER_PID=$!
# In another terminal, START AN INTERACTIVE PSQL SESSION:
#   psql "postgres://postgres:postgres@127.0.0.1:5432/postgres"
#   atc=> LISTEN atc_outbox;
#   (leave the session open — it will print "Asynchronous notification" on each NOTIFY)
# Back in the original terminal:
curl -X POST http://127.0.0.1:8080/v1/webhooks/github -H "X-GitHub-Event: workflow_run" -d '...'
# psql session prints: Asynchronous notification 'atc_outbox' with payload '<seq>' received from server process with PID ...
kill $SERVER_PID

# 10. Cleanup.
docker stop atc-pg
```

---

## Rollout

Single PR; no shadow / dual-mode for 2d. Squash-merge:

- `feat(server): add LISTEN/NOTIFY end-to-end with listener fetch-and-log stub`

No feature flag. NOTIFY emission is gated by the existing `pg_pool.is_some()` branch in the webhook handler, and listener tasks are only spawned when `pg_pool.is_some()`. In-memory mode (no DB configured) is unaffected.

**Pre-merge checklist:**
- [ ] All ACs have a passing test, named `phase_2d_notify_listener_ac<N>_<short>`.
- [ ] Helm chart changes pass `helm lint`, `helm-unittest`, and `helm-check` matrix.
- [ ] Documents to Update table satisfied; `Last verified` dates bumped.
- [ ] Phase 3c readyz-deferral note added to `rollout-and-implementation.md`.
- [ ] `pg_notify`, `MAX(seq)`, and drain `SELECT` queries each have entries in `backend/.sqlx/`.
- [ ] `start_pg` lifted to `tests/common/mod.rs`; no remaining local copies in the 3 affected test files.
- [ ] Test plan posted as first comment on PR (per repo convention; see `~/.claude/.../memory/feedback_test_plans.md`).

---

## Documents to Update

| Document | Change |
|---|---|
| `docs/architecture/backend-server.md` | **Edit existing sections in place** (per codex I5): Webhook Handler (add NOTIFY emission inside-txn), AppState (note no field changes; listener handles in main scope), Lifecycle Wiring (listener init sequence before `axum::serve`), Metrics (new counters), Files (add `src/listener.rs`). Add a small new subsection "NOTIFY emission and listener fetch-stub." Bump `Last verified`. |
| `docs/architecture/deployment.md` | New env var `ATC_DATABASE_LISTENER_URL` documented, including when to set it, default fallback, and plain-value vs. existingSecret paths via Helm. Bump `Last verified`. |
| `backend/crates/atc-server/CLAUDE.md` | Add `listener` to Module table; add NOTIFY emission and listener fetch-stub rows to Contracts. Bump `Last verified`. |
| `deploy/helm/atc/CLAUDE.md` | If chart-level invariants change (operator MUST set listener URL when main pool runs through transaction-mode pooler), document. Otherwise note no change. Bump `Last verified` if edited. |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | (a) Clarify line 100 "after commit" wording (delivery semantics; emission is inside-txn). (b) Mark `ATC_DATABASE_LISTENER_URL` (line 102) IMPLEMENTED in 2d. (c) Mark Phase 2d DONE checklist mirroring Phase 2c structure. (d) Add Phase 3c readyz-deferral note (contract level — do not prescribe `Arc<AtomicBool>` shape per codex M2). Bump `Last verified`. |
| `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md` | Implementation Status: Decision 3 (NOTIFY + session-mode connection) implemented in 2d; Decision 5 partial (listener structure + coalesce + watermark init implemented; `forward_to_ws_clients` step deferred to 3c). Cross-link PR. |
| `backend/crates/atc-core/CLAUDE.md` | No change (atc-core is unchanged). Verify during implementation; do not skip. |

No new ADR. Phase 2d implements ADR 0002 Decision 3 (NOTIFY) and the structure + watermark halves of Decision 5; only the WS forwarding half of D5 remains for Phase 3c.

---

## Implementation Notes

**NOTIFY queue overflow.** PG's async notification queue defaults to 8 GB; if it fills, `tx.commit()` itself fails. At ATC's scale this is unrealistic — listeners drain notifications eagerly via `recv()`. Document the failure mode in `backend-server.md` for completeness; no defensive code in 2d.

**`PgListener::connect` connection count.** Opens its own internal single-connection pool. One additional PG connection per replica beyond the main pool. Operators should size PG `max_connections` accordingly (`main_pool_max + 1 per replica`). sqlx's internal pool reserves a small headroom (1–2 connections) for reconnect handoff; the practical operator sizing is `main_pool_max + 2 per replica` to give the listener room to reconnect without thrashing.

**Drain pass cost.** The drain task wakes per notification and runs a `SELECT * FROM outbox WHERE seq > $1 ORDER BY seq` for whatever has accumulated since the last pass. Index on `outbox.seq` (BIGSERIAL PK) makes this an indexed range scan; cheap. CPU/memory cost negligible.

**Test timing budgets.** AC4–AC7 use `Duration::from_secs(2)` budgets for counter advancement. Local PG via testcontainers is typically <100ms per webhook round-trip. The 2s budget accommodates CI variance without flakiness.

**`observed_*` counters in production.** `Option<Arc<AtomicU64>>` parameters on `spawn_listener_task` and `spawn_drain_task` default to `None` in `main.rs`. They exist solely for test observability.

**Test-only `drain_started` baseline hook.** A `Option<Arc<Notify>>` parameter on `spawn_drain_task` that signals at the top of each drain pass. Tests await the first signal post-startup, then capture baseline counters; subsequent test signals can be ignored. Used by AC4–AC7 to make assertions delta-from-baseline rather than absolute, removing flakiness from the unconditional startup pass.

---

## Out of Scope (Phase Boundary Reminders)

These items WILL look tempting during implementation. They are NOT part of Phase 2d.

| Tempting change | Phase that owns it |
|---|---|
| Forward fetched outbox rows to local WS clients | **3c** |
| Drop in-memory `StateStore` from broadcast path | **3c** |
| Drop `Mutex<u64>` for cursor | **3c** |
| `/readyz` reports listener-degraded | **3c** (shape is 3c design work — do not prescribe `Arc<AtomicBool>` here) |
| Rename `StateSnapshot.seq` → `lastSeq` | **3a** |
| Remove `pool_stats_after` from `SeqEvent` | **3b** |
| Snapshot read from PG | **3c** |
| Helm `replicaCount > 1` gate | **4** |
| Outbox retention / eviction policy | **5** |
| Exponential backoff on listener `recv()` errors | **5** |
| Refactor `db_readyz_tests.rs` inline setup | **separate cleanup PR** |

If any of these feel necessary to make 2d work, the plan has a hole — stop and surface it before implementing.

---

## Glossary

- **NOTIFY / LISTEN** — PG primitives for cross-session signaling. NOTIFYs are queued during a txn and delivered on COMMIT. Listeners receive them via `recv()` on a session-mode-compatible connection.
- **`atc_outbox`** — The PG channel name used by ATC for outbox-row notifications. Hardcoded constant in `listener.rs`.
- **Drain** — The act of fetching outbox rows on each NOTIFY and advancing a watermark. In Phase 2d the fetched rows are logged but not forwarded; in Phase 3c the same loop pushes each row to local WS clients.
- **Watermark (`last_forwarded_seq`)** — The highest seq the drain task has fetched and "processed" (logged in 2d; forwarded in 3c). Initialized to `MAX(seq)` at startup. Owned as a local `i64` in the drain task.
- **Coalesce** — Multiple notifications during an in-flight drain pass collapse to a single trailing pass via `tokio::sync::Notify`'s permit semantics. Prevents drain-storms.
- **Session-mode connection** — A PG connection where session state (including `LISTEN` registrations) survives across statements. Direct PG and session-mode PgBouncer qualify; transaction-mode PgBouncer does not.
- **`PgListener`** — sqlx's wrapper around a session-mode connection that issues `LISTEN` and provides `recv()` over an internal reconnecting connection.
- **`Arc<tokio::sync::Notify>`** — Tokio's permit-based async signal primitive. One permit at a time; `notify_one()` on N concurrent calls grants exactly one permit; `notified().await` consumes it.
- **Observation counter** — `Arc<AtomicU64>` injected into the listener and drain tasks at test fixture construction time. Production code passes `None`.
- **`drain_started` hook** — `Arc<Notify>` test-only signal fired at the top of each drain pass. Tests use it to capture a post-startup baseline before measuring deltas.

---

## Codex Review Disposition

The plan was reviewed by `codex exec` (sandbox read-only) on 2026-05-04. Disposition of every finding:

### Blockers — all addressed in this revision

| Finding | Disposition |
|---|---|
| Startup sequencing impossible as written; loses early notifications | **Fixed.** D3 reworked: listener `connect()` + `listen()` + watermark init happen in main BEFORE `axum::serve`. Spawn helpers take a fully-initialized `PgListener` and an `i64` watermark. |
| `ATC_DATABASE_LISTENER_URL` not wired through Helm | **Fixed.** Helm chart files added to Files to Modify (values.yaml, values.schema.json, deployment.yaml template, fixtures). `deployment.md` added to Documents to Update. AC14 covers the chart wiring. |
| Plan diverges from rollout doc's fetch-and-log stub | **Fixed.** Drain task now fetches outbox rows on each wake-up, logs them, and advances watermark. Watermark init at boot per ADR 0002 D5 (no longer deferred to 3c). Phase 3c reduces to a one-line `forward_to_ws_clients` addition inside the drain loop body. |

### Important — addressed

| Finding | Disposition |
|---|---|
| `atc_pg_listener_reconnects_total` misnamed | **Fixed.** Renamed to `atc_pg_listener_recv_errors_total`. Description clarifies sqlx hides successful reconnects. |
| AC6/AC7 flaky without baseline hook | **Fixed.** Added `drain_started: Option<Arc<Notify>>` test-only signal. AC4–AC7 capture baseline post-startup and assert deltas. AC7 uses bounded interval `[2, 3]` rather than strict equality. |
| AC11 should not be conditional | **Fixed.** AC11 calls `PgListener::connect(bad_url)` directly via the library seam; no binary spawning. |
| Verification commands not executable as written | **Fixed.** Subshells with `(cd backend && ...)`; `env -u ATC_DATABASE_URL`; interactive `psql` session described as a separate terminal session, not a one-shot `psql -c`. |
| `backend-server.md` update scope too narrow | **Fixed.** Documents to Update specifies *which existing sections* to update in place (Webhook Handler, AppState, Lifecycle Wiring, Metrics, Files), plus a small new subsection. |

### Minor — addressed

| Finding | Disposition |
|---|---|
| D10 overstates `start_pg` duplication | **Fixed.** Corrected to 3 files; `db_readyz_tests.rs` left as-is per D11. |
| Phase E over-specifies 3c implementation detail | **Fixed.** Phase 3c readyz-deferral note kept at contract level; no `Arc<AtomicBool>` prescription. |
| AC12 untestable "no runtime leak" clause | **Fixed.** Dropped vague clause; AC12 keeps only the handle-finished assertions. |
| Planning-workflow conformance partial | **Fixed.** Added Summary, Definition of Done, and Implementation Guidance pointer. |

### Strengths preserved

- Inside-txn `pg_notify` placement (write side) — preserves Phase 2c mutex-across-txn invariant, closes post-commit/pre-notify crash window.
- `SELECT pg_notify($1::text, $2::text)` is the correct sqlx 0.8.6 form.
- `PgListener::connect(&listener_url)` over `connect_with(&pool)` — avoids burning a main-pool slot.
- Phase boundary discipline: WS forwarding, in-memory store retirement, snapshot cutover, and `/readyz` listener-health all correctly deferred.

### Unresolved questions surfaced and resolved

- **Q1: pure no-op vs. fetch-and-log stub.** Resolved in favor of fetch-and-log per the rollout doc. Drain task fetches rows by `seq > watermark ORDER BY seq` and logs them; watermark init at boot.
- **Q2: distinct existingSecret key for listener URL.** Resolved YES — `existingSecret.databaseListenerUrlKey` mirrors the existing `databaseUrlKey` pattern.
