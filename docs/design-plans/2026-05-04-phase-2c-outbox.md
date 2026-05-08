# Phase 2c — Outbox Table + Transactional Writes

> **Slug:** `phase-2c-outbox`
> **Branch hint:** `feat/phase-2c-outbox` (squash-merged; PR title `feat(server): add transactional outbox and reverse webhook error policy`)
> **Plan author:** Claude (Opus 4.7), 2026-05-04
> **Phase reference:** [`docs/architecture/state-externalization-research/rollout-and-implementation.md` § Phase 2c](../../Projects/atc/docs/architecture/state-externalization-research/rollout-and-implementation.md)

---

## Context

Phase 2b shipped PostgreSQL **shadow writes**: every webhook produces an in-memory mutation AND a durable PG write, with drift observability via the `atc_shadow_pg_write_failures_total{kind}` counter. PG and the in-memory store are coherent in the happy path but they are **independently committed** — a PG transient failure produces drift, and the webhook still returns 200 because the in-memory path is still authoritative.

Phase 2c is the cutover from "shadow + tolerate drift" to "PG is the durable record of truth, atomically per webhook":

1. Add an `outbox` table whose `BIGSERIAL seq` PK is the **durable monotonic-not-gapless cursor** that Phase 3c will forward over WS and Phase 4 will use across replicas.
2. Make the current-state UPSERT and the outbox INSERT happen in **one PostgreSQL transaction** so split-brain is structurally impossible (`runs`/`jobs` rows and outbox rows commit together or roll back together).
3. **Reverse the error policy:** transient PG failures (commit failed) now return **5xx** as an honest operator signal — the webhook delivery shows up as a failure in GitHub's webhook UI and the operator can manually redeliver. Parity rejections (the predicated UPSERT matched 0 rows; transition is permanently invalid) return **200** because retrying would fail identically; this matches the existing `StoreError → 200` contract in `routes.rs:186-188`. Successful webhooks return 200. **GitHub does NOT auto-retry on 5xx** ([Handling failed webhook deliveries](https://docs.github.com/en/webhooks/using-webhooks/handling-failed-webhook-deliveries)) — 4xx and 5xx are equivalent from GitHub's wire perspective; the choice is purely about operator observability and HTTP semantic accuracy.
4. **Rename the Phase 2b metric** from `atc_shadow_pg_write_failures_total` to `atc_pg_write_failures_total` — the writes are no longer "shadow." Broader "shadow" terminology cleanup (function names, comments, test file names) is a separate sweep tracked in D6.

**Out of scope** (these are explicit handoffs to later phases — do NOT pull them forward):
- `NOTIFY` emission and listener stub → **Phase 2d**
- `seq` → `lastSeq` cursor rename + frontend lockstep → **Phase 3a**
- `pool_stats_after` removal from `SeqEvent` and frontend derivation → **Phase 3b**
- `state_handler` reading PG instead of in-memory → **Phase 3c**
- Multi-replica Helm gating + SQLite removal → **Phase 4**

After Phase 2c, the outbox is **written but not yet read by anyone**. WS clients still receive `SeqEvent`s broadcast from the in-memory path with `Mutex<u64>` seq, exactly as today.

ADR refs: [ADR 0002 Decision 2](../../Projects/atc/docs/architecture-decisions/0002-state-externalization-postgres-outbox.md) (atomicity), [ADR 0003 Decision 2](../../Projects/atc/docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (monotonic-not-gapless), [ADR 0004](../../Projects/atc/docs/architecture-decisions/0004-frontend-derived-pool-stats.md) (outbox stores domain events only).

---

## Locked Decisions

These were settled during Phase 2c brainstorming. Do **not** reopen during implementation; if a constraint emerges that contradicts one of these, stop and revise the plan.

### D1 — Outbox schema (Hybrid: projected columns + JSONB payload)

`migrations/0002_outbox.sql`:

```sql
CREATE TABLE outbox (
    seq         BIGSERIAL PRIMARY KEY,
    kind        TEXT      NOT NULL CHECK (kind IN ('run', 'job')),
    run_id      BIGINT    NOT NULL,
    job_id      BIGINT    NULL,
    payload     JSONB     NOT NULL,
    inserted_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX outbox_run_idx ON outbox (run_id);
-- seq PRIMARY KEY already gives us the cursor index for Phase 3c forwarder.
```

**Column naming note.** `inserted_at` (not `committed_at`) — PostgreSQL `now()` returns `transaction_timestamp()` (transaction start), not commit time. The column is for forwarder-lag observability later; transaction-start granularity (~ms) is sufficient. If we ever want true commit-time, we'd use `clock_timestamp()` or set it post-commit, both of which are out of scope here.

**Payload type.** `payload` JSONB stores the full `RunEventEnvelope` or `JobEventEnvelope` (the atc-core envelope structs that the route handler already constructs from a parsed webhook). **NOT `SeqEvent`** — `SeqEvent` carries `pool_stats_after: Option<Vec<RunnerPoolStats>>`, which ADR 0004 forbids in the outbox. The envelopes already exclude `pool_stats_after` by construction.

**Why hybrid (vs. fully normalized or fully JSONB):**
- Projected `kind`, `run_id`, `job_id` give cheap indexable filtering for debugging and the Phase 3c forwarder's row-by-seq drain.
- JSONB `payload` keeps the event body contract identical to what the WS forwarder will need to reconstruct, without reintroducing per-field migrations every time `RunEvent` / `JobEvent` evolves.
- `inserted_at` is explicit (not `created_at`) to disambiguate from `runs.created_at` / `jobs.created_at` — the timestamp is set by PostgreSQL `now()` (transaction-start time, ~ms granularity), useful as a coarse forwarder-lag signal in Phase 3c.

**No FK to `runs(id)`.** The outbox is an **append-only event log**; eviction policy for `runs`/`jobs` (ADR 0003 Decision 4) and outbox retention (ADR 0003 Decision 4 + Phase 5) are decided independently. A FK would couple them and complicate eviction.

### D2 — `PersistentStore` trait fate (Drop field, keep struct)

> **Revised by [ADR 0005](../architecture-decisions/0005-persistentstore-trait-relocation.md):** The Phase 2c constraint ("`&self` cannot yield `&mut Transaction`") is moot when the impl owns its transaction internally. Issue #50 is closed: the trait was relocated to `atc-server::persist` with `PgStore` (internal tx lifecycle) and `InMemoryStore` as impls; `AppState` carries `Arc<dyn PersistentStore>` for the write path. The original "test-only seam vs. extend vs. delete" framing below is superseded.

The webhook handler in Phase 2c **owns and drives the transaction**, which means it cannot use `Arc<dyn PersistentStore + Send + Sync>` — `&self` from a trait object cannot yield the `&mut Transaction<'_, Postgres>` that sqlx requires for executor binding inside a transaction.

**Action:**
1. **Drop** `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` from `AppState` (`backend/crates/atc-server/src/state.rs`). Sweep all field references at the same time: **14 `pg_store: None` test literals + 1 `pg_store: Some(Arc::new(PgStore::new(pool)))` helper site** (codex-verified count).
2. **Keep** the `PgStore` struct and its `impl PersistentStore for PgStore` block in `backend/crates/atc-server/src/persist.rs`. The trait method bodies stay valid for unit tests that drive single statements through `&pool` rather than a transaction.
3. **GitHub issue filed: [#50](https://github.com/bojanrajkovic/atc/issues/50)** ("Reconcile PersistentStore trait with transactional outbox (Phase 5)"). Tracks the question of whether to keep the trait as a test-only seam, extend it with a transaction-scoped variant, or delete it. Resolution required before state externalization is declared shipped; not a blocker for 2c–4.

**Why drop the field rather than keep both:**
- A field that's never read is a code smell that future agents will misuse.
- Tests that pass `pg_store: None` are revealing the field is already optional and unused — the cleanup is "delete," not "wire it up."
- The struct + impl stay for trait-mediated unit testing without violating the "no dead trait dispatch in route handler" invariant.

### D3 — Transaction composition (`pub(crate)` helpers in `persist.rs`)

Add four `pub(crate)` free functions in `backend/crates/atc-server/src/persist.rs`, each taking `&mut sqlx::Transaction<'_, Postgres>` as the executor:

| Helper | Purpose |
|---|---|
| `upsert_run_in_txn(&mut tx, &RunEventEnvelope) -> Result<(), PersistError>` | The current `apply_run_event` body, rewritten to bind against `&mut **tx`. |
| `upsert_job_in_txn(&mut tx, &JobEventEnvelope) -> Result<(), PersistError>` | The current `apply_job_event` body — including the stub-run UPSERT preamble — rewritten against `&mut **tx`. |
| `insert_outbox_run_in_txn(&mut tx, &RunEventEnvelope) -> Result<i64, PersistError>` | `INSERT INTO outbox (kind, run_id, payload) VALUES ('run', $1, $2::jsonb) RETURNING seq` — returns the assigned seq (`i64`). Phase 2c doesn't use it yet; Phase 2d's NOTIFY payload will. |
| `insert_outbox_job_in_txn(&mut tx, &JobEventEnvelope) -> Result<i64, PersistError>` | `INSERT INTO outbox (kind, run_id, job_id, payload) VALUES ('job', $1, $2, $3::jsonb) RETURNING seq` — returns the assigned seq (`i64`). |

The route handler creates `let mut tx = pool.begin().await?;`, calls helpers in order, then `tx.commit().await?`. **The stub-run UPSERT preamble (currently the first statement in `apply_job_event`) moves inside the transaction** — PostgreSQL same-transaction statement visibility ensures the FK check on the subsequent job UPSERT sees the just-inserted stub row.

**Why `pub(crate)` free functions vs. methods on `PgStore`:**
- The route handler already has `&PgPool` (via `AppState.pg_pool`); it doesn't need a `PgStore` instance to begin a transaction.
- Free functions express the constraint "the caller owns the transaction" more clearly than methods on a struct that holds a pool.
- Test seams are unchanged — unit tests that want to exercise a single statement still go through the `PersistentStore` trait against `&pool`.

### D4 — Webhook handler ordering (mutex-across-txn, gated on `pg_pool.is_some()`)

**Critical: `pg_pool: Option<sqlx::PgPool>` semantics are preserved.** Per `main.rs:69-87` and ADR 0003 ("Single-replica deployments may run without PG through Phase 4"), the in-memory-only path is still supported when `ATC_DATABASE_URL` is unset. Phase 2c gates the transactional path on `state.pg_pool.is_some()`:

- `pg_pool = Some(pool)` → transactional path (this section).
- `pg_pool = None` → in-memory-only path (existing Phase 2b behavior with the shadow PG block elided). This is the dev/test default and remains the contract through Phase 4.

```rust
// backend/crates/atc-server/src/routes.rs (webhook handler, post-Phase-2c)
async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<serde_json::Value>) {
    // (sections 1–3: header check, HMAC verify, parse_webhook — UNCHANGED from 2b)

    let event = match result {
        ParseResult::Parsed(event) => event,
        ParseResult::Skipped { .. } => return (StatusCode::OK, Json(/* ... */)),
    };

    // 4. Acquire seq mutex BEFORE pool.begin() — preserves broadcast=durable order.
    let mut seq_guard = state.seq.lock().await;

    // 5. Branch on whether PG is configured.
    let pool_stats_after: Option<Option<Vec<RunnerPoolStats>>> = match &state.pg_pool {
        Some(pool) => {
            // Transactional path (Phase 2c).
            let mut tx = match pool.begin().await {
                Ok(tx) => tx,
                Err(e) => {
                    metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                        .increment(1);
                    tracing::error!(error = %e, "pg begin failed");
                    drop(seq_guard);
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(/* ... */));
                }
            };

            let txn_result: Result<Option<Vec<RunnerPoolStats>>, PersistError> = match &*event {
                atc_github::WebhookEvent::Run(env) => {
                    upsert_run_in_txn(&mut tx, env).await?;
                    insert_outbox_run_in_txn(&mut tx, env).await?;
                    Ok(None)
                }
                atc_github::WebhookEvent::Job(env) => {
                    // upsert_job_in_txn does the stub-run UPSERT internally (statement
                    // ordering inside the txn relies on PG same-txn visibility).
                    upsert_job_in_txn(&mut tx, env).await?;
                    insert_outbox_job_in_txn(&mut tx, env).await?;
                    Ok(/* pool_stats computed below after in-mem apply */ None)
                }
            };

            match txn_result {
                Ok(_) => {
                    if let Err(e) = tx.commit().await {
                        metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                            .increment(1);
                        tracing::error!(error = %e, "pg commit failed");
                        drop(seq_guard);
                        return (StatusCode::SERVICE_UNAVAILABLE, Json(/* ... */));
                    }
                    // PG committed. Now apply to in-memory store (still under the mutex).
                    apply_in_memory_post_commit(&state, &*event).await
                }
                Err(PersistError::InvalidTransition) => {
                    // Parity rejection. Transaction is rolled back automatically when
                    // tx is dropped without commit. No in-memory apply, no broadcast.
                    metrics::counter!("atc_pg_write_failures_total", "kind" => "parity")
                        .increment(1);
                    tracing::warn!("pg parity rejection: transition invalid under predicate");
                    drop(seq_guard);
                    return (StatusCode::OK, Json(serde_json::json!({"status": "rejected"})));
                }
                Err(PersistError::Backend(e)) => {
                    metrics::counter!("atc_pg_write_failures_total", "kind" => "transient")
                        .increment(1);
                    tracing::error!(error = %e, "pg backend failure mid-txn");
                    drop(seq_guard);
                    return (StatusCode::SERVICE_UNAVAILABLE, Json(/* ... */));
                }
            }
        }
        None => {
            // In-memory-only path (no DB configured). Existing 2b in-memory behavior
            // minus the shadow PG block.
            apply_in_memory_post_commit(&state, &*event).await
        }
    };

    // 6. Broadcast under the same mutex so cursor matches durable order.
    if let Some(pool_stats_after) = pool_stats_after {
        let seq = *seq_guard;
        *seq_guard += 1;
        let seq_event = SeqEvent { seq, event: *event, pool_stats_after };
        let _ = state.webhook_tx.send(seq_event);
    }
    drop(seq_guard);

    (StatusCode::OK, Json(serde_json::json!({"status": "processed"})))
}

/// Apply a domain event to the in-memory store. Returns:
/// - `Some(Some(pool_stats))` for Job events that succeeded (broadcast it)
/// - `Some(None)`             for Run events that succeeded
/// - `None`                   if in-memory apply rejected (no broadcast)
async fn apply_in_memory_post_commit(
    state: &AppState,
    event: &atc_github::WebhookEvent,
) -> Option<Option<Vec<RunnerPoolStats>>> {
    match event {
        atc_github::WebhookEvent::Run(env) => {
            match state.store.apply_run_event(env.clone()).await {
                Ok(_) => Some(None),
                Err(e) => {
                    // PG already committed but in-memory rejected. The durable
                    // record is correct; in-memory drift will heal on the next
                    // event for this entity (same predicate). Log and skip
                    // broadcast — clients refresh via /v1/state on the next tick.
                    metrics::counter!("atc_pg_in_memory_drift_total").increment(1);
                    tracing::warn!(error = %e, "post-commit in-memory drift");
                    None
                }
            }
        }
        atc_github::WebhookEvent::Job(env) => {
            match state.store.apply_job_event(env.clone()).await {
                Ok(_) => Some(Some(state.store.pool_stats().await)),
                Err(e) => {
                    metrics::counter!("atc_pg_in_memory_drift_total").increment(1);
                    tracing::warn!(error = %e, "post-commit in-memory drift");
                    None
                }
            }
        }
    }
}
```

**Error policy (revised — no false "GitHub auto-retries" rationale):**

| HTTP status | When | Counter | Operator effect |
|---|---|---|---|
| **200 (status: processed)** | PG commit + in-memory apply both succeed | none | normal |
| **200 (status: rejected)** | Predicated UPSERT matched 0 rows (parity) | `atc_pg_write_failures_total{kind="parity"}` | logged; no operator action |
| **200 (status: drift)** | PG commit OK, in-memory apply failed | `atc_pg_in_memory_drift_total` | logged; in-memory drift heals on next event for this entity |
| **503 Service Unavailable** | `pool.begin()` / `tx.commit()` / mid-txn `sqlx::Error::Backend` | `atc_pg_write_failures_total{kind="transient"}` | webhook delivery shows as failed in GitHub UI; operator can manually redeliver |

**Why 200 for parity rejection (not 4xx):** GitHub does NOT auto-retry on 4xx OR 5xx ([Handling failed webhook deliveries](https://docs.github.com/en/webhooks/using-webhooks/handling-failed-webhook-deliveries)) — both show up as "failed deliveries" in the operator UI. A parity rejection is a permanent rejection (the transition is invalid; retrying with the same payload will fail identically), and surfacing it as a "failed delivery" is misleading: the webhook was correctly received and intentionally not applied. 200 with `status: "rejected"` body is honest. This matches the existing in-memory `StoreError → 200` contract at `routes.rs:186-188` rather than diverging from it.

**Why 503 for transient (not 200):** A transient failure means PG is unavailable AND we have no durable record — returning 200 would lie about the outcome. 503 surfaces the failure in GitHub's webhook UI so the operator notices and can manually redeliver via the "Redeliver" button (or via `gh api repos/:owner/:repo/hooks/:hook_id/deliveries/:id/attempts`). The redelivery is operator-driven, not GitHub-automatic.

**Post-commit in-memory drift policy:** If PG commits but the in-memory apply returns Err, the durable record (PG) is correct and the in-memory store has drifted. Phase 2c logs this (`atc_pg_in_memory_drift_total` counter), skips the broadcast, and returns 200. Drift heals on the next webhook for the same entity because the in-memory `predecessors_of` predicate accepts retries. **Phase 3c eliminates this entire class of failure** by retiring the in-memory store as the broadcast source — the WS forwarder reads from outbox directly. Until then, drift is a P3 observability signal, not a correctness bug for the durable record.

**Why mutex-across-txn (vs. txn first, then mutex):**
The naive ordering — commit transaction → drop wait → acquire mutex → broadcast — opens a race for two concurrent webhooks targeting the same `run_id`:

1. Webhook A and Webhook B both `pool.begin()` and execute UPSERTs.
2. PG serializes their commits (durable order: A < B).
3. **But** the tasks then race for the seq mutex; B may acquire first.
4. Broadcast order: B before A. Durable order: A before B. **Inversion.**

Holding the mutex from before `pool.begin()` through `tx.commit()` and the in-memory apply guarantees broadcast order = durable order. This is the same invariant Phase 2b's "mutex-across-broadcast" enforces — Phase 2c just extends it across the txn so PG commit order is also captured.

**Cost of holding the mutex across PG round-trips:** webhook handlers now serialize on the seq mutex for ~10–50 ms each (PG round-trip + commit), versus ~microseconds in 2b. The `state_handler` path (snapshot reads under the same mutex) experiences correspondingly higher tail latency. **This is acceptable for 2c** — Phase 3c retires the `Mutex<u64>` entirely (cursor comes from `MAX(seq)` over outbox in the snapshot transaction), so 2c is a transient pessimization, not a permanent regression. Document it in the architecture doc; do not optimize away.

### D5 — Seq counter (Mutex<u64> stays through 2c)

Keep `state.seq: Arc<Mutex<u64>>` for broadcast ordering and `StateSnapshot.seq` (still "next seq to assign" semantics until Phase 3a renames + reorients to `lastSeq`).

The outbox's `BIGSERIAL` is the **durable** monotonic-not-gapless cursor for Phase 3c+. In 2c the two cursors coexist:
- **In-memory `Mutex<u64>`:** strict-monotonic, gapless, resets on restart, drives WS broadcast. Visible to clients.
- **Outbox `BIGSERIAL`:** strict-monotonic, NOT gapless (aborted txns consume seq), persists across restarts. **Not yet visible to clients.**

Phase 3c retires the in-memory cursor; Phase 3a renames the WS contract field. **Do not couple them in 2c.**

### D6 — Metric rename + shadow terminology cleanup

**Two-tier sweep.** The metric identifier rename is hard-required for wire-contract reasons. The broader "shadow" terminology cleanup (function names, file names, comments) is a hygiene sweep that lands in the same PR for coherence.

**Tier 1: metric identifier rename — `atc_shadow_pg_write_failures_total{kind}` → `atc_pg_write_failures_total{kind}`** (and registration helper rename — `register_shadow_pg_counters` → `register_pg_write_counters`):

| File | Symbol / line context |
|---|---|
| `backend/crates/atc-server/src/metrics.rs` | counter declaration + describe + `register_shadow_pg_counters` fn name |
| `backend/crates/atc-server/src/main.rs` | call site of `metrics::register_shadow_pg_counters()` |
| `backend/crates/atc-server/src/routes.rs` | counter `metrics::counter!` increments at parity/transient sites |
| `backend/crates/atc-server/tests/shadow_writes_tests.rs` | counter-name assertions (file is also renamed in tier 2) |
| `backend/crates/atc-server/CLAUDE.md` | §Contracts row mentioning the counter |
| `docs/architecture/backend-server.md` | architecture-doc prose mentioning the counter |

Codex confirmed via repo grep: no external dashboard, alerting rule, Helm chart, or Grafana JSON references the old counter name. The rename is fully in-repo.

**Tier 2: shadow terminology cleanup** (cohesion sweep — same PR):

| File | Change |
|---|---|
| `tests/shadow_writes_tests.rs` | rename file to `tests/transactional_writes_tests.rs` |
| `routes.rs` | rename `PgWrite` enum's "shadow" comments; update tracing log strings ("shadow PG write parity failure" → "pg parity rejection"); remove or adapt the `pg_write` variable name and the post-mutex shadow-write block |
| `metrics.rs` | rename `register_shadow_pg_counters` → `register_pg_write_counters` (or similar); update the describe!() string |
| Architecture doc + CLAUDE.md prose | search-and-replace "shadow" → "transactional" / remove where stale; preserve historical references inside `state-externalization-research/*.md` (those describe Phase 2b state and should stay) |

Historical design docs under `docs/architecture/state-externalization-research/` are intentionally **not** swept — they describe the pre-2c state and should remain accurate to that history. The 2c-status update goes in the Implementation Status section of the rollout document, not by editing prior phase descriptions.

---

## Files to Modify

### Production code

- `backend/crates/atc-server/migrations/0002_outbox.sql` — **new file**, schema from D1.
- `backend/crates/atc-server/src/persist.rs` — add 4 `pub(crate)` helpers (D3); existing `impl PersistentStore for PgStore` stays.
- `backend/crates/atc-server/src/routes.rs` — webhook handler rewritten to mutex-across-txn ordering (D4); error policy reversed; counter name updated.
- `backend/crates/atc-server/src/state.rs` — drop `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>` field (D2).
- `backend/crates/atc-server/src/main.rs` — drop `pg_store` construction in `AppState::new()`; only `pg_pool` is stored.
- `backend/crates/atc-server/src/metrics.rs` — rename counter + registration helper (D6).
- `backend/.sqlx/` — **regenerated** after schema/query changes (`cargo sqlx prepare --workspace -- --tests`). Per `CONTRIBUTING.md` (offline sqlx requirement), the cache must be in sync with the source SQL or CI will fail with `set DATABASE_URL` errors at compile time.

### Tests

- `backend/crates/atc-server/tests/shadow_writes_tests.rs` → **rename** to `tests/transactional_writes_tests.rs`. Adapt assertions:
  - Drop drift-tolerant assertions (PG and in-memory now atomically consistent in the happy path).
  - Add txn-abort scenario (see Acceptance Criteria).
  - Update counter name to `atc_pg_write_failures_total`.
- `backend/crates/atc-server/tests/persist_pg_tests.rs` — keep; trait-mediated single-statement tests are still valid.
- `backend/crates/atc-server/tests/outbox_tests.rs` — **new file**, see Acceptance Criteria for cases.
- ~16 test files with `pg_store: None` literals — sweep to remove the field from `AppState` constructions.

---

## Documents to Update

Per `.ed3d/design-plan-guidance.md` Principle 6, every doc that describes superseded behavior must be updated alongside the code in the same PR.

| Document | Change |
|---|---|
| `docs/architecture/backend-server.md` | Multi-section update. (a) **Webhook Handler:** rewrite to mutex-across-txn ordering, `pg_pool: Option` branch, error policy table (200 success / 200 rejected / 200 drift / 503 transient). (b) **SeqEvent Sidecar Contract:** add note that outbox payload is `RunEventEnvelope`/`JobEventEnvelope`, NOT `SeqEvent`. (c) **AppState section:** drop `pg_store` field; document `pg_pool: Option<PgPool>` retained for in-memory-only mode through Phase 4. (d) **Lifecycle Wiring:** update `main.rs` flow to no longer instantiate `PgStore` for AppState. (e) **Startup Behavior:** document the `(Some, None)` branch retention. (f) **Metrics section:** rename counter; add `atc_pg_in_memory_drift_total`. (g) New "Phase 2c: Transactional Outbox" subsection. Bump "Last verified" date. |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Mark Phase 2c **complete**; copy DONE checklist into the in-scope list (mirroring Phase 2b's structure at lines 60–72). Bump "Last verified" date. **Do NOT edit prior phase descriptions** — those are historical. |
| `backend/crates/atc-server/CLAUDE.md` | Update §Modules: `routes` (webhook handler drives txn), `persist` (exports `pub(crate)` txn helpers + retains `PgStore` for tests), `state` (drop `pg_store` field reference), `metrics` (counter renamed; add drift counter), `migrations/` (add 0002_outbox.sql reference). Update §Contracts: replace "Shadow PG writes" + "PG write failures" rows with "Transactional writes" + "Webhook error policy" + "Optional PG mode (pg_pool=None branch)". Bump "Last verified" date. |
| `backend/crates/atc-core/CLAUDE.md` | No change needed — `PersistentStore` trait is unchanged; `predecessors_of()` semantics unchanged. (Verify during implementation; do not skip the verification step.) |
| `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md` | Add "Implementation status" subsection at the end: Decision 2 (atomicity) and Decision 3 (NOTIFY infrastructure groundwork — outbox table only, NOTIFY emission lands in 2d) implemented in Phase 2c. Cross-link to PR. Add retroactive annotation `> **Revised by Phase 2c implementation: ...**` if any wording in the body is contradicted by what shipped. |
| `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` | Add "Implementation status" subsection: Decision 2 (monotonic-not-gapless cursor on outbox) implemented; Decision 4 (TTL eviction as SQL DELETE) deferred to Phase 5 per rollout doc. Cross-link to PR. |
| `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` | Add "Implementation status" subsection: outbox-stores-domain-events-only constraint enforced (envelopes, not SeqEvent); frontend derivation lands in Phase 3b (deferred). Cross-link to PR. |

**No new ADR.** Phase 2c is implementation of decisions already accepted in ADR 0002 — ADR 0003 Decision 2 (monotonic-not-gapless) materializes the moment the outbox is `BIGSERIAL`, which is intentional.

---

## Acceptance Criteria

Each criterion has a slug `phase-2c-outbox.AC<N>.<M>` for traceability into test names.

### AC1: Atomicity (success)

`AC1.1` — A single webhook produces exactly one `runs`/`jobs` row update AND exactly one matching `outbox` row, both visible after `tx.commit()`.

`AC1.2` — Outbox `seq` is strictly monotonic across a sequence of N successful webhooks (`SELECT seq FROM outbox ORDER BY seq` returns N consecutive — though "consecutive" is not required by ADR 0003 Decision 2; only "strictly increasing" is. Test asserts strict-increase.).

`AC1.3` — Outbox `payload` JSONB deserializes back into `RunEventEnvelope` or `JobEventEnvelope` exactly (round-trip equality on a representative envelope).

### AC2: Atomicity (rollback)

`AC2.1` — Parity rejection rolls back the entire transaction. When the predicated UPSERT matches 0 rows (e.g., re-applying `RunEvent::Requested` to a run already `Completed`), the transaction is dropped without commit: no `runs` row update, no stub `runs` row (for the job-arrives-first case), no `outbox` row. The webhook returns 200 with `{"status":"rejected"}` and `atc_pg_write_failures_total{kind="parity"}` increments by 1. **NOTE:** The outbox `seq` does NOT advance in this case because the outbox `INSERT` is statement 2 in the helper sequence; statement 1 (the predicated UPSERT) returns 0 rows and short-circuits before the outbox INSERT runs.

`AC2.2` — Demonstrates the BIGSERIAL monotonic-not-gapless property directly. Test code (bypassing the route handler) opens a transaction, runs `INSERT INTO outbox (kind, run_id, payload) VALUES ('run', 1, '{}'::jsonb) RETURNING seq` to capture `seq_a`, calls `tx.rollback()`, then opens a fresh transaction, runs the same INSERT and commits, capturing `seq_b`. Asserts `seq_b > seq_a + 1` IS POSSIBLE under aborted-txn behavior, OR more weakly that `seq_b > seq_a` and the rolled-back seq is not visible in any committed row. This isolates the BIGSERIAL property from the route handler so the test does not need to inject a commit-time failure.

`AC2.3` — Job webhook rolls back stub-run UPSERT when the job UPSERT rejects. When `upsert_job_in_txn` rejects due to predicate mismatch (e.g., re-applying a Queued event to an InProgress job), the stub `runs` UPSERT performed earlier in the same transaction is also rolled back: no stub run row appears in `runs` table.

### AC3: Error policy

`AC3.1` — Parity rejection returns **200** with body `{"status":"rejected"}` (matching the existing `StoreError → 200` contract). Counter `atc_pg_write_failures_total{kind="parity"}` increments by 1. No SeqEvent broadcast.

`AC3.2` — Transient PG failure (`pool.begin()` error, `tx.commit()` error, or mid-txn `sqlx::Error::Backend`) returns **503 Service Unavailable**. Counter `atc_pg_write_failures_total{kind="transient"}` increments by 1. No SeqEvent broadcast.

`AC3.3` — Post-commit in-memory drift (PG committed but `state.store.apply_*` returned Err) returns **200** with body `{"status":"processed"}` (the durable record committed; client recovery via next-event-heals). Counter `atc_pg_in_memory_drift_total` increments by 1. No SeqEvent broadcast.

`AC3.4` — Successful webhook returns **200** with body `{"status":"processed"}`. No counter increments. SeqEvent broadcast.

`AC3.5` — When `pg_pool: None` (no DB configured), the webhook handler runs the in-memory-only path: no PG calls, no `atc_pg_*` counter increments, behavior matches Phase 2b's pre-shadow code path. (Verifies the `pg_pool.is_some()` gate.)

### AC4: Ordering invariant (broadcast = durable)

`AC4.1` — Under N concurrent webhooks targeting the same `run_id`, the broadcast `SeqEvent` order matches the `outbox.seq` order. (Test fires N tasks against a shared pool and asserts a total order across both signals.)

`AC4.2` — Under N concurrent webhooks targeting **different** `run_id`s, the same invariant holds (no cross-run inversion).

### AC5: Stub run inside txn

`AC5.1` — A job webhook for a `run_id` that has not yet been observed produces: stub `runs` row (status=Queued) AND real `jobs` row AND outbox row, all in one transaction. After commit, the run can later be upgraded by a real run webhook; the predicate `[Queued, InProgress, Completed]` for `RunStatus::Completed` matches the stub `Queued` and accepts the upgrade.

`AC5.2` — If the job UPSERT itself rejects (parity), the stub `runs` row is also rolled back (no orphan stub).

### AC6: Outbox payload is envelope, NOT SeqEvent

`AC6.1` — `payload` round-trips exactly to `RunEventEnvelope` / `JobEventEnvelope`. Decoding as `SeqEvent` produces a shape mismatch (the deserializer either fails on the missing `seq` field or — if not strict — `pool_stats_after` would be missing). Assertion: in Rust, `serde_json::from_value::<RunEventEnvelope>(payload)` succeeds for `kind='run'` rows and `serde_json::from_value::<JobEventEnvelope>(payload)` succeeds for `kind='job'` rows. Additionally assert in SQL that `payload ?? 'pool_stats_after'` is `false` (top-level key absence — sufficient because envelope structs do not nest `pool_stats_after` under any sub-object; see `atc-core::event` envelope definitions).

### AC7: AppState cleanup

`AC7.1` — `AppState` no longer carries `pg_store: Option<Arc<dyn PersistentStore + Send + Sync>>`. The struct compiles, all tests compile after the field-removal sweep, and `cargo clippy -p atc-server -- -D warnings` is clean. Sweep target: 14 `pg_store: None` literals plus 1 `pg_store: Some(Arc::new(PgStore::new(...)))` helper site (codex-verified count) — all reduced to no field reference.

`AC7.2` — `PgStore` struct and `impl PersistentStore for PgStore` are still present and compile. At least one unit test in `persist_pg_tests.rs` exercises `apply_run_event` through the trait against `&pool`.

`AC7.3` — `backend/.sqlx/` cache is regenerated and committed. `cargo sqlx prepare --workspace -- --tests` runs clean; `git diff --stat backend/.sqlx/` shows expected new query files for the outbox INSERTs and the predicated UPSERTs (which now run via `&mut Transaction` rather than `&PgPool` and may produce slightly different cached query metadata).

---

## Verification

```bash
# 1. Regenerate offline sqlx cache against a live DB. CONTRIBUTING.md:241 requires
#    this whenever a query! macro or migration changes; CI compiles offline.
just db-up                                                         # ephemeral PG
DATABASE_URL=postgres://atc:atc@localhost:5432/atc \
  cargo sqlx prepare --workspace -- --tests
git status backend/.sqlx/                                          # expect new/changed files

# 2. Full test sweep (Docker required — testcontainers boots ephemeral PG)
just test

# 3. Targeted backend tests
cargo test -p atc-server --test transactional_writes_tests
cargo test -p atc-server --test outbox_tests
cargo test -p atc-server --test persist_pg_tests

# 4. Lint
cargo clippy -p atc-server -- -D warnings
cargo clippy -p atc-core   -- -D warnings

# 5. Type generation (verify no schema drift after envelope serde changes)
just types
git diff --exit-code frontend/src/lib/types/generated/

# 6. Doc-staleness gate (must pass before push)
scripts/check-docs-lefthook.sh

# 7. In-memory-only mode smoke (verifies pg_pool=None branch still works)
ATC_DATABASE_URL= cargo run -p atc-server -- &
curl -X POST http://127.0.0.1:8080/v1/webhooks/github \
  -H "X-GitHub-Event: workflow_run" \
  -d '{"action":"requested",...}'
# Expect: 200, no atc_pg_* counter increments
```

**Manual smoke test** (ephemeral PG via testcontainer or `just db-up`):

1. Fire a `workflow_run.requested` webhook → **200 `{"status":"processed"}`**, `runs` row exists with `status='Queued'`, `outbox` has 1 row with `kind='run'`.
2. Fire `workflow_run.completed` for same run → 200 `processed`, `runs.status='Completed'`, `outbox` has 2 rows; assert `outbox.seq` strictly increasing.
3. **Re-fire** `workflow_run.requested` for same run → **200 `{"status":"rejected"}`**, `outbox` count unchanged at 2 (the predicated UPSERT short-circuits before the outbox INSERT — no seq is consumed in this path), counter `atc_pg_write_failures_total{kind="parity"}` = 1.
4. Stop the PG container, fire any webhook → **503 Service Unavailable**, counter `atc_pg_write_failures_total{kind="transient"}` increments. (No row in either table since `pool.begin()` failed.)
5. **Demonstrate BIGSERIAL gap separately** (psql session): `BEGIN; INSERT INTO outbox(kind,run_id,payload) VALUES ('run',1,'{}'::jsonb) RETURNING seq;` (note seq_a) `ROLLBACK; BEGIN; INSERT INTO outbox(kind,run_id,payload) VALUES ('run',1,'{}'::jsonb) RETURNING seq;` (note seq_b) `COMMIT;` — assert `seq_b > seq_a` and `seq_a` does not appear in any committed row.

---

## Rollout

This is a single PR — there is no shadow/dual-mode in 2c. The cutover is atomic with the merge:

- `feat(server): add transactional outbox and reverse webhook error policy`
- Squash-merge to main.
- No feature flag gate — Phase 2b's shadow path was already exercised under the same `ATC_DATABASE_URL` config; Phase 2c just collapses dual-write into single-transaction-write. If `ATC_DATABASE_URL` is unset, the server still refuses to start (Phase 2a contract: PG required when set; Phase 4 will gate the requirement on `replicaCount > 1` for Helm).

**Pre-merge checklist:**
- [ ] All ACs have a passing test, named `phase_2c_outbox_ac<N>_<M>_<short_description>`.
- [ ] Documents to Update table satisfied; `Last verified` dates bumped.
- [ ] Metric rename swept (5 locations confirmed via grep).
- [ ] `pg_store: None` test literals removed (sweep verified by grepping for the literal post-merge — should be 0 hits).
- [x] GitHub issue filed: [#50](https://github.com/bojanrajkovic/atc/issues/50) — "Reconcile PersistentStore trait with transactional outbox (Phase 5)".
- [ ] Test plan posted as first comment on PR (per repo convention).

---

## Implementation Notes (advisor-flagged)

**State_handler latency under mutex contention.** Holding the seq mutex across `pool.begin() ... tx.commit()` extends the critical section from microseconds to ~10–50 ms per webhook. The `state_handler` (snapshot reads under the same mutex, per `backend-server.md` § State Snapshot) will see correspondingly higher p99 latency. This is a transient pessimization — Phase 3c retires the `Mutex<u64>` and reads the cursor as `MAX(seq)` over the outbox in the snapshot transaction, eliminating the contention. **Document the regression in the architecture doc; do not attempt to optimize it away in 2c.**

**Outbox payload type disambiguation.** `RunEventEnvelope` and `JobEventEnvelope` (atc-core) are NOT `SeqEvent` (atc-server). The envelopes are the parsed-webhook domain events; `SeqEvent` is the broadcast wrapper that carries `seq` + `pool_stats_after`. ADR 0004 forbids `pool_stats_after` in the outbox — the envelopes already exclude it, so storing envelopes is correct **by structure, not just by convention**. Any "let me JSON-encode the SeqEvent because that's what we broadcast" instinct during implementation is a bug — push back.

**Stub run UPSERT inside the transaction is correct.** PostgreSQL's same-transaction statement visibility guarantees the FK check on the subsequent `jobs` UPSERT sees the just-inserted stub `runs` row. There is no race window here. Verified against current Phase 2b behavior (which already emits the stub via a separate statement on the same pool) — moving it inside the txn does NOT change semantics, only atomicity.

**`predecessors_of` audit is already passing.** Verified during planning:
- `RunStatus::predecessors_of(Completed) = [Queued, InProgress, Completed]` — stub run (Queued) → real Completed run event passes the predicate ✓
- `JobStatus::predecessors_of(Completed) = [InProgress, Completed]` — the existing state machine treats Queued→Completed for jobs as INVALID. **Caveat (codex flag):** GitHub's `workflow_job` docs only state that `completed` fires when the job finishes regardless of conclusion ([Webhook events and payloads — workflow_job](https://docs.github.com/en/webhooks/webhook-events-and-payloads#workflow_job)); they do NOT explicitly rule out a Queued→Completed sequence (e.g., for skipped jobs that never enter InProgress). If real-world GitHub deliveries surface Queued→Completed jobs, the parity counter `atc_pg_write_failures_total{kind="parity"}` will spike — at which point we extend `JobStatus::predecessors_of(Completed)` to include `Queued`. That is a follow-up fix to the state machine, **not** a Phase 2c blocker; the predicate semantics are pre-existing and the in-memory store would reject the same transition today.

No `predecessors_of` changes required for Phase 2c.

**`PersistentStore` trait carries vestigial weight.** The trait stays compiling and tested through `persist_pg_tests.rs`, but `AppState` no longer holds it and the route handler bypasses it for transaction work. [Issue #50](https://github.com/bojanrajkovic/atc/issues/50) tracks the question: "Is the trait carrying its weight, or should it be removed?" Answer that question in Phase 5 hardening — not now.

> **Resolved by [ADR 0005](../architecture-decisions/0005-persistentstore-trait-relocation.md):** Issue #50 closed. The trait was relocated to `atc-server::persist` with internal-transaction-owning impls (`PgStore`, `InMemoryStore`). `AppState` carries `Arc<dyn PersistentStore>` as the write-path dispatch point. The "test-only seam vs. extend vs. delete" question is settled: extend (with trait carrying its weight as the production dispatch interface).

---

## Out of Scope (Phase Boundary Reminders)

These items WILL look tempting during implementation. They are NOT part of Phase 2c.

| Tempting change | Phase that owns it |
|---|---|
| Emit `NOTIFY` after `tx.commit()` | **2d** |
| Add `LISTEN` connection in `atc-server` | **2d** |
| Rename `StateSnapshot.seq` → `lastSeq` | **3a** |
| Invert frontend `>=` comparator to `>` | **3a** |
| Remove `pool_stats_after` from `SeqEvent` | **3b** |
| Add frontend `pools` `$derived.by` | **3b** |
| Read `state_handler` snapshot from PG | **3c** |
| Read WS forwarder from outbox | **3c** |
| Drop in-memory `StateStore` | **3c** |
| Drop `Mutex<u64>` for cursor | **3c** |
| Helm `replicaCount > 1` gate on Postgres URL | **4** |
| Remove SQLite Helm mode | **4** |
| Outbox retention policy + eviction | **5** |

If any of these feel necessary to make 2c work, the plan has a hole — stop and surface it before implementing.

---

## Codex Review — Disposition

The plan was reviewed by `codex exec` (`gpt-5.4`, `model_reasoning_effort = "xhigh"`, `--sandbox read-only`) on 2026-05-04. Disposition of every finding:

### Blockers — all addressed in this plan

| Finding | Disposition |
|---|---|
| Error policy 4xx/5xx based on false "GitHub auto-retries" rationale | **Fixed.** D4 rewritten: 200/200/200/503 policy with operator-observability rationale; no auto-retry claim. |
| AC2.1/AC2.2 cannot prove BIGSERIAL gap because parity rejection short-circuits before outbox INSERT | **Fixed.** AC2.1 now asserts the no-gap path (parity rolls back before INSERT). AC2.2 is a direct DB-level test (`INSERT ... RETURNING seq; ROLLBACK;` then `INSERT ... RETURNING seq; COMMIT;`) that isolates the BIGSERIAL property without needing route-handler injection. New AC2.3 covers stub-run rollback on job-UPSERT rejection. |
| `pg_pool: None` (in-memory-only) contract violated by "PG required to start" | **Fixed.** D4 explicitly branches on `state.pg_pool.is_some()`; in-memory-only path retained. New AC3.5 verifies the branch. ADR 0003 contract through Phase 4 preserved. |
| `backend/.sqlx/` cache update missing | **Fixed.** Added to Files to Modify and Verification (`cargo sqlx prepare --workspace -- --tests`). New AC7.3 verifies the cache is regenerated. |

### Important — addressed

| Finding | Disposition |
|---|---|
| `committed_at DEFAULT now()` is misnamed (PG `now()` is txn-start, not commit time) | **Fixed.** Renamed to `inserted_at`; documented semantics. |
| Pseudocode used non-existent symbols (`state.tx.send`, `state.store.apply`) | **Fixed.** D4 pseudocode now uses real symbols (`state.webhook_tx.send`, `state.store.apply_run_event`/`apply_job_event`, `state.store.pool_stats()`); post-commit drift policy explicit (`atc_pg_in_memory_drift_total` counter, log + skip broadcast, return 200). |
| Metric rename sweep too narrow (5 hits but broader "shadow" terminology spread further) | **Fixed.** D6 is now two-tier: identifier rename (Tier 1) + shadow terminology cleanup (Tier 2, same PR for cohesion). Historical research docs explicitly excluded. |
| `backend-server.md` update entry too narrow | **Fixed.** Documents to Update now lists 7 sub-sections (Webhook Handler, SeqEvent Sidecar, AppState, Lifecycle Wiring, Startup Behavior, Metrics, new Phase 2c subsection). |

### Minor — addressed

- `pg_store: None` count corrected from `~16` to `14 literals + 1 helper site` per codex grep.
- AC6.1 JSONB existence check tightened (top-level `?? 'pool_stats_after'` is sufficient because envelopes do not nest the key under sub-objects; rationale documented).
- `RETURNING seq` semantics explicit: helpers return `Result<i64, PersistError>` (Phase 2c discards; Phase 2d uses).

### Flags acknowledged but not changing the plan

- **`predecessors_of(JobStatus::Completed)` rationale.** Codex flagged that "GitHub does not emit Queued→Completed for jobs" is unproven. The state-machine predicate is pre-existing and unchanged by Phase 2c. If real-world GitHub deliveries surface this transition, the parity counter will spike and we extend the predicate as a separate fix. Note added to Implementation Notes section.

### Strengths preserved

- `&mut **tx` executor binding for `sqlx 0.8.6` (codex confirmed).
- `RunEventEnvelope`/`JobEventEnvelope` (not `SeqEvent`) as outbox payload type (codex confirmed).
- Mutex-across-txn ordering invariant (codex confirmed sound).
- Non-zero count for `pg_store: Some(...)` helper site captured in AC7.1 sweep target.
