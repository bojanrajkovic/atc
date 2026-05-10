---
plan_name: phase-3c-read-path-cutover
status: ready-for-implementation
last_updated: 2026-05-05
---

# Phase 3c: Read-Path Cutover (PG → /v1/state, drain → /v1/ws)

## Context

Phases 1–3b are merged: durable Postgres write path is live (every webhook does a transactional UPSERT → outbox INSERT → `pg_notify` → commit), and the Phase 2d listener task receives NOTIFYs and drains outbox rows but currently only **logs** them (`listener.rs:144` — the explicit "stub: not forwarding" message). The wire contract is clean: `StateSnapshot { lastSeq, runs, jobs }`, `SeqEvent { seq, event }`. `GET /v1/state` and `/v1/ws` still serve from in-memory.

Phase 3c completes the migration: PG becomes authoritative for both reads (snapshot) and the live event stream. Once landed, single-replica deployments are fully on durable storage; multi-replica becomes feasible (Phase 4).

**Why this matters:** today, a process restart drops live WS subscribers and forces a snapshot re-fetch. After 3c, the outbox is the durable event log; replicas are stateless caches over PG. This is the prerequisite for symmetric-replicas (ADR 0002 Decision 5).

## Design Decisions

### D1. `min_pending_seq` lives on `AppState` as `Arc<AtomicI64>` (always present)

**Decision:** Add `pub min_pending_seq: Arc<AtomicI64>` (init `i64::MAX`) as a first-class field on `AppState`. **Not** `Option<Arc<AtomicI64>>`.

**Rationale:** the atomic is 8 bytes; the cost of carrying it in in-memory mode is zero. Threading an `Option` through both the listener and drain-task spawn would introduce matching/unwrapping noise and obscure the gap-healing intent. `Arc<AtomicI64>` is `Clone`, so the listener and drain tasks each receive a clone as a spawn argument. **The webhook handler does not touch `min_pending_seq`** — only the listener (`fetch_min` on each NOTIFY) and the drain task (`swap` at pass start). The field exists on `AppState` for spawn-argument plumbing, not handler access. Field is no-op when `pg_pool` is `None`.

### D2. Gap-healing fetches `min_pending_seq` at NOTIFY arrival, not at handler commit

**Decision:** the listener task parses each `pg_notify` payload as i64 and calls `min_pending_seq.fetch_min(seq, Release)` on every NOTIFY. The webhook handler does NOT touch `min_pending_seq`. The drain task's `swap(MAX, AcqRel)` at pass start captures any seq registered by the listener since the previous swap.

**Why not register at handler commit time:** an alternative is to have the handler call `fetch_min(seq)` immediately after `INSERT INTO outbox … RETURNING seq` (before commit) and reset to `MAX` after commit. This loses rows under concurrent commits. Trace A (seq=10) and B (seq=11) overlapping, B commits first:

1. A `fetch_min(10)` → atomic=10. B `fetch_min(11)` → atomic=10.
2. B commits (NOTIFY 11). B `compare_exchange(11, MAX)` → atomic stays 10 (current ≠ 11).
3. Drain pass: swap → backstop=10, atomic=MAX. Query floor = `min(0, 9) = 0`. SELECT > 0 returns seq=11 only (A still uncommitted under READ COMMITTED). Forward seq=11. watermark=11.
4. A commits (NOTIFY 10). A `compare_exchange(10, MAX)` → atomic stays MAX (current ≠ 10).
5. Drain pass: swap → backstop=MAX. Query floor = `min(11, MAX-1) = 11`. SELECT > 11 returns nothing. **seq=10 missed.**

The handler-driven reset clobbers A's in-flight signal before the drain pass that should have re-scanned. Listener-driven fixes it: A's NOTIFY arriving after commit calls `fetch_min(10)` → atomic=10 → the drain pass it wakes captures backstop=10 → query floor 9 → SELECT > 9 returns both rows → rescan succeeds. Listener-side `fetch_min` is causally chained to commit visibility (NOTIFY only fires post-commit), which closes the race.

### D3. Bounded ring-buffer dedup in the drain task

**Decision:** the drain task maintains a bounded `VecDeque<i64>` + `HashSet<i64>` pair (capacity 2048 seqs) of recently-broadcast seqs. Before broadcasting a row, the drain checks the set; on hit, increment a duplicate-skipped metric and skip the broadcast. On miss, broadcast and insert (evicting the oldest seq if the buffer is full).

**Why server-side dedup is necessary:** the live `webhook_tx` broadcast feeds two consumers in the frontend. (1) `runStore.applyRunEvent` / `applyJobEvent` (`frontend/src/lib/stores/runs.svelte.ts:114, 180`) is idempotent in outcome — the apply rebuilds the entity from the payload and `SvelteMap.set()`s it; re-applying produces the same state. (2) `liveRegion.observeFlush` (`frontend/src/lib/aria/live-region.svelte.ts:41`), wired via `EventDispatcher.setOnFlush`, announces `RunEvent::Requested` and `RunEvent::Completed` to assistive tech. Re-applying the same `SeqEvent` produces a *duplicate audible announcement* — a real UX defect.

ADR 0003 explicitly declined frontend dedupe on the assumption that the backend would not overlap deliveries. Phase 3c's gap-healing rescan reopens that assumption by design. Rather than reopen the ADR, we add the smallest backend-side gate that preserves the contract: a 2048-entry recently-seen ring. Memory cost: ~16 KB per replica.

**Sizing:** at 100 webhooks/sec peak, 2048 entries cover ~20 seconds of drain history — orders of magnitude wider than the in-flight window between concurrent commits (milliseconds). The ring is a defensive cushion against the realistic rescan window, not a correctness guarantee for unbounded delays.

**Why not a single-i64 dedup gate:** in the A/B scenario from §D2, the rescan returns both seq=10 and seq=11. A `seq > last_forwarded=11` gate drops seq=10 — the row we're trying to recover. The ring treats each seq individually, so seq=11 (in ring) is skipped while seq=10 (not in ring) is broadcast.

**Why not a `HashSet` alone:** without bounded eviction, the set grows unbounded. The `VecDeque` provides FIFO eviction in O(1); the `HashSet` provides O(1) membership. Pair gives us both with bounded memory.

**Metric:** `atc_pg_drain_duplicate_skipped_total` (Counter, no labels) increments on each duplicate skip.

**Backfill / out-of-band-write caveat:** the row-lock argument that justifies same-entity correctness applies to normal webhook-handler writes only. Future backfill tooling, replay flows, or out-of-band outbox inserts could manufacture same-entity backward delivery (they bypass the handler's transactional UPSERT path). Such tools MUST go through `upsert_*_in_txn` + `insert_outbox_*_in_txn`, OR the frontend will need a `highestAppliedSeq` per-entity guard. Out of scope for Phase 3c; tracked in §Risks.

### D4. In PG mode, the webhook handler stops broadcasting and stops applying to in-memory store

**Decision:** when `pg_pool` is `Some`, after a successful PG commit the handler **does not** call `webhook_tx.send(seq_event)` and **does not** call `state.store.apply_run_event(...)` / `apply_job_event(...)`. The drain task is the sole writer to `webhook_tx` in PG mode. The in-memory store is dormant in PG mode.

**Why:** otherwise every event fires twice (handler + drain) and in-memory store accumulates state that is never read.

**In-memory mode (`pg_pool` is `None`)** is unchanged: handler locks `seq` mutex, applies to in-memory store, broadcasts via `webhook_tx`. The fields `seq: Mutex<u64>`, `store: Arc<StateStore>`, and `webhook_tx` stay on `AppState` for this mode.

### D5. `/readyz` reflects listener health via a heartbeat atomic + 5s tick timer

**Decision:** add `pub last_drain_pass_at: Arc<AtomicI64>` (epoch milliseconds) to `AppState`. The drain-task loop's `tokio::select!` (see pseudocode) wakes on either (a) `drain_notify.notified()` or (b) a 5-second `sleep`; in BOTH cases the heartbeat is stored before the conditional pass body. Quiet periods (no NOTIFY) keep the heartbeat fresh via the timer arm. `/readyz` checks: PG `SELECT 1` succeeds AND (drain heartbeat is < 30s old OR pg_pool is None). Threshold hard-coded at 30s; configurable later if needed.

**Why this and not connection-state polling:** `PgListener` lacks a public `is_connected` predicate. The drain task is the load-bearing health signal — if it stalls, NOTIFYs may still arrive but events won't reach WS clients. A heartbeat makes "is forwarding alive?" observable directly. 30s threshold gives 6× margin over the 5s tick.

### D6. PG snapshot reads use a `REPEATABLE READ` transaction

**Decision:** `state_handler` PG path opens one transaction at `REPEATABLE READ` isolation, executes three queries (`SELECT * FROM runs`, `SELECT * FROM jobs`, `SELECT COALESCE(MAX(seq), 0) FROM outbox`), and commits.

**Why not READ COMMITTED:** under READ COMMITTED, each statement sees its own snapshot. A concurrent webhook commit between `SELECT * FROM runs` and `SELECT MAX(seq)` can advance MAX(seq) without the runs/jobs SELECTs reflecting that commit. The frontend hands the snapshot off to the WS buffer drain at `connection.ts:113` with `if (buffered.seq > snapshotLastSeq)`. If `lastSeq` is ahead of the runs/jobs content, a buffered event with `seq == lastSeq` (whose mutation IS NOT in the snapshot's runs/jobs) gets discarded by the frontend filter. The mutation is permanently invisible until the next reconnect.

**Why REPEATABLE READ is correct:** all three SELECTs see the same MVCC snapshot. Either MAX(seq) reflects committed-before-snapshot-start state OR all three queries reflect the same later moment. In both cases, `lastSeq` is a true upper bound on the runs/jobs content, and any buffered event with `seq > lastSeq` is genuinely beyond the snapshot.

**Cost:** REPEATABLE READ adds a slightly heavier MVCC visibility check per query but no row-level locks. Acceptable for a low-frequency endpoint (snapshot fetches happen once per page load + reconnect). No serialization failures expected since the snapshot is read-only.

### D7. TTL eviction stays in-memory only for Phase 3c

**Decision:** keep `store.start_eviction_task(60s)` running. In PG mode the in-memory store stays empty (D4), so eviction is a no-op. PG-side TTL eviction (SQL `DELETE`) is explicitly deferred to Phase 5 per ADR 0003 Decision 4.

## Files to Modify

| File | Change |
|------|--------|
| `backend/crates/atc-server/src/state.rs` | Add `min_pending_seq: Arc<AtomicI64>` and `last_drain_pass_at: Arc<AtomicI64>` fields to `AppState`. Add appropriate `use` statements. |
| `backend/crates/atc-server/src/main.rs` | Construct both atomics before `AppState`. Pass clones to `spawn_drain_task` (new params). Listener task gets `min_pending_seq.clone()` too — see listener changes below. |
| `backend/crates/atc-server/src/listener.rs` | (a) `spawn_listener_task` now takes `min_pending_seq: Arc<AtomicI64>`; on each NOTIFY, parse `notification.payload()` as i64 and `fetch_min(seq, Release)`. (b) `spawn_drain_task` takes `min_pending_seq`, `last_drain_pass_at`, `webhook_tx: broadcast::Sender<SeqEvent>`. Owns the `recent_ring`/`recent_set` dedup state and the `tokio::select!` heartbeat-vs-notify arm (5s tick). (c) `drain_pass` decodes `payload` JSONB by `kind` discriminator into `WebhookEvent::{Run,Job}`, applies ring-buffer dedup, broadcasts `SeqEvent`, updates heartbeat. Replace the "stub: not forwarding" log with the forwarding call. Use a separate `page_cursor` local (not `watermark`) for batch pagination within a pass. |
| `backend/crates/atc-server/src/persist.rs` | New `pub(crate)` functions, all taking `&mut Transaction<'_, Postgres>` so `state_handler` can compose them inside one REPEATABLE READ tx: `read_all_runs(&mut tx) -> Vec<WorkflowRun>` (filters out placeholder rows — see new migration below), `read_all_jobs(&mut tx) -> Vec<Job>`, `read_last_seq(&mut tx) -> i64`. Maps PG row types to `atc_core` domain types. |
| `backend/crates/atc-server/migrations/0003_runs_placeholder.sql` | New migration: `ALTER TABLE runs ADD COLUMN placeholder BOOLEAN NOT NULL DEFAULT false;` Update `upsert_job_in_txn`'s stub-run INSERT (currently in `persist.rs:413–516`) to pass `placeholder = true` for FK-only stub rows. `read_all_runs` SELECTs `WHERE placeholder = false`. Realigns PG `/v1/state` semantics with the in-memory store, which never exposed FK-only stub runs. |
| `backend/crates/atc-server/src/metrics.rs` | Register two new counters: `atc_pg_drain_duplicate_skipped_total`, `atc_pg_drain_unknown_kind_total`. (Existing `atc_pg_drain_rows_total` and `atc_pg_drain_passes_total` already registered.) |
| `backend/crates/atc-server/src/routes.rs` | (a) `state_handler`: branch on `pg_pool.is_some()`. PG path opens a REPEATABLE READ tx (`pool.begin().await?` then `SET TRANSACTION ISOLATION LEVEL REPEATABLE READ` or use `pool.begin_with(...)` if sqlx supports it directly), calls the three new readers, returns `StateSnapshot { last_seq: u64::try_from(max_seq).unwrap_or(0), runs, jobs }`. In-memory path unchanged. (b) `webhook_handler` PG branch: stop locking `seq` mutex; stop applying to `state.store`; stop broadcasting via `webhook_tx`. The seq returned from outbox INSERT is informational only (still useful for the response body / metrics). (c) `readyz_handler`: extend with heartbeat-staleness check when `pg_pool.is_some()`. |
| `backend/crates/atc-server/src/ws.rs` | No code change. Subscriber model is unchanged; the broadcast channel is just fed by a different writer in PG mode. |
| `.sqlx/` | Three new compiled queries (read all runs, read all jobs, last seq). Run `cargo sqlx prepare` and commit the `.json` files. CI's `--offline` build verifies the cache matches. |
| `backend/crates/atc-server/tests/` | New tests: see Test Plan. |

**Migration:** one new migration (`0003_runs_placeholder.sql`) adds the `placeholder` column to `runs` so `/v1/state` can hide FK-only stub rows. No backfill needed (default is false; existing rows are real).

## Existing Code to Reuse

- `backend/crates/atc-server/src/persist.rs::upsert_run_in_txn`, `insert_outbox_run_in_txn`, `notify_outbox_seq_in_txn` — keep using as-is in `webhook_handler` PG branch.
- `backend/crates/atc-server/src/listener.rs::connect_listener` — listener URL plumbing already in place via `ATC_DATABASE_LISTENER_URL`.
- `backend/crates/atc-core::WorkflowRun`, `Job`, `WebhookEvent`, `RunEventEnvelope`, `JobEventEnvelope` — domain types for the new readers and the payload deserializer.
- Initial watermark query at `main.rs:141–148` (`SELECT COALESCE(MAX(seq), 0) FROM outbox`) — keep, used to seed the drain's `watermark` so cold-start doesn't re-scan history. The dedup ring starts empty at boot: the first pass after startup queries `seq > watermark`, so it never touches any seq below or equal to the initial watermark and the empty ring cannot create false-negative skips.
- `.sqlx/` offline cache pattern — `cargo sqlx prepare` workflow already established (~34 cached queries today).

## Drain-Task Algorithm (precise)

```text
let mut watermark: i64       = initial_watermark;   // pass-end high-water; advances only after pass completes
let mut recent_ring:  VecDeque<i64> = VecDeque::with_capacity(2048);
let mut recent_set:   HashSet<i64>  = HashSet::with_capacity(2048);
const DEDUP_CAP:      usize = 2048;
const HEARTBEAT_TICK: Duration = Duration::from_secs(5);

// On startup, prime the dedup ring with the initial watermark sentinel? No — startup
// watermark equals MAX(seq) at boot, so the first pass naturally queries `> watermark`
// and never sees any seq <= initial_watermark. Empty ring at boot is correct.

loop:
    // Wait for either a NOTIFY-driven wake-up or a heartbeat tick.
    let woken_by_notify = tokio::select! {
        _ = drain_notify.notified()       => true
        _ = tokio::time::sleep(HEARTBEAT_TICK) => false
    };

    last_drain_pass_at.store(now_millis(), Relaxed);   // tick heartbeat unconditionally

    if !woken_by_notify {
        continue;   // no work to do; just kept /readyz fresh
    }

    // ---------- Drain pass starts ----------
    let backstop = min_pending_seq.swap(i64::MAX, AcqRel);
    let pass_start_floor = watermark.min(backstop.saturating_sub(1));

    let mut page_cursor: i64 = pass_start_floor;        // SEPARATE from `watermark`
    let mut max_seq_seen: Option<i64> = None;

    loop:  // batch loop within this drain pass
        rows = SELECT seq, kind, run_id, job_id, payload
                 FROM outbox
                WHERE seq > $page_cursor
                ORDER BY seq
                LIMIT $DRAIN_BATCH_SIZE;

        if rows.is_empty(): break;

        for row in rows:
            // Decode by `kind` discriminator. The outbox stores RunEventEnvelope or
            // JobEventEnvelope (NOT WebhookEvent); we rewrap into WebhookEvent for the wire.
            let event: WebhookEvent = match row.kind.as_str() {
                "run" => WebhookEvent::Run(serde_json::from_value(row.payload)?),
                "job" => WebhookEvent::Job(serde_json::from_value(row.payload)?),
                other => { metric: atc_pg_drain_unknown_kind_total++; continue; }
            };

            if recent_set.contains(&row.seq) {
                metric: atc_pg_drain_duplicate_skipped_total++;
            } else {
                let seq_u64 = u64::try_from(row.seq)
                    .expect("BIGSERIAL is positive; would overflow i64::MAX before u64");
                let _ = webhook_tx.send(SeqEvent { seq: seq_u64, event });

                recent_ring.push_back(row.seq);
                recent_set.insert(row.seq);
                if recent_ring.len() > DEDUP_CAP {
                    if let Some(evicted) = recent_ring.pop_front() {
                        recent_set.remove(&evicted);
                    }
                }
            }

            max_seq_seen = Some(max_seq_seen.unwrap_or(row.seq).max(row.seq));
            page_cursor = row.seq;   // advance page cursor to last seq SEEN this batch
            metric: atc_pg_drain_rows_total++;

        if rows.len() < DRAIN_BATCH_SIZE: break;
    // ---------- Drain pass ends ----------

    // Advance watermark only at pass end, to the highest seq actually seen this pass.
    // Critically, this is `max(watermark, max_seq_seen)`, NOT `page_cursor` (which
    // could under-advance if the pass was empty).
    if let Some(seen) = max_seq_seen {
        watermark = watermark.max(seen);
    }
    last_drain_pass_at.store(now_millis(), Relaxed);
    metric: atc_pg_drain_passes_total++
```

**Why the separate `page_cursor`:** during a backstop-lowered rescan, `pass_start_floor` can be far below `watermark` — e.g., `watermark=1000`, `backstop=1` gives `pass_start_floor=0`. The first batch returns seqs 1..500. If we used `watermark` as the next-batch floor, the second batch would `SELECT > 1000` and skip seqs 501..1000. `page_cursor` is set from the last seq actually seen in the previous batch, so pagination advances within the rescan window correctly. `watermark` updates only at pass end.

**Memory ordering:** `swap(i64::MAX, AcqRel)` on the drain side synchronizes with the listener's `fetch_min(seq, Release)` so any min-update happens-before the swap that captures it. Use `Release` (not `Relaxed`) on the listener side; the drain's `Acquire` half of `AcqRel` pairs with it.

**Listener task addition:**

```text
on PgNotification:
    let seq = match notification.payload().parse::<i64>() {
        Ok(s) => s,
        Err(e) => { log::warn!("malformed NOTIFY payload: {e}"); continue; }
    };
    min_pending_seq.fetch_min(seq, Release);
    drain_notify.notify_one();
```

## Webhook-Handler PG Branch (precise)

```text
// drop the seq mutex acquisition entirely in PG mode
let mut tx = pool.begin().await?
upsert_run_in_txn(&mut tx, env)?
let seq = insert_outbox_run_in_txn(&mut tx, env)?
notify_outbox_seq_in_txn(&mut tx, seq)?
tx.commit().await?

// no in-memory apply
// no webhook_tx.send

return 200 with { status: "accepted", seq }
```

Failure paths (parity rejection, transient DB error) keep their existing metric increments and HTTP-status mappings.

## Test Plan

Tests live in `backend/crates/atc-server/tests/`. Tier 3 (testcontainers PG) for everything below. Mark all with `#[serial_test::serial]` because of the global Prometheus recorder.

| ID | Name (file:test) | Asserts |
|----|------|---------|
| T1 | `phase_3c_state_pg_read::snapshot_returns_pg_state` | After firing N webhooks via HTTP, `GET /v1/state` returns `lastSeq == N` and runs/jobs reflect the PG projection (not in-memory). Achieve coverage by directly mutating PG outside the handler and asserting the response sees the change. |
| T2 | `phase_3c_state_pg_read::snapshot_is_consistent_under_concurrent_commits` | Open the snapshot read in one task; while it's open, fire a webhook from another task. Assert: returned `lastSeq` and runs/jobs reflect the SAME MVCC snapshot — i.e., either both reflect the concurrent commit OR neither does. Specifically: there is no return where `lastSeq == seq_of_concurrent_commit` but `runs` does NOT include the run that commit mutated. (REPEATABLE READ guarantees this.) |
| T3 | `phase_3c_state_pg_read::snapshot_falls_back_when_pg_pool_none` | With `pg_pool: None`, returns the in-memory `lastSeq` and `store.snapshot()`. (Verifies D4 didn't regress in-memory mode.) |
| T4 | `phase_3c_drain_forwards::ws_receives_seq_event_in_order` | Subscribe a WS client, fire 5 webhooks; client receives 5 `SeqEvent`s with seqs 1..=5 in order. The seq comes from BIGSERIAL, not the in-memory mutex. |
| T5 | `phase_3c_drain_forwards::handler_does_not_double_broadcast_in_pg_mode` | Fire one webhook in PG mode; assert WS receives exactly one event (i.e., the handler is silent and only the drain broadcasts). |
| T6 | `phase_3c_gap_healing::overlapping_commits_in_reverse_order_deliver_both` | Manually craft an A/B race using two concurrent `pool.begin()` transactions where A INSERTs outbox seq=N (run X), B INSERTs seq=N+1 (run Y, DIFFERENT entity to bypass row-lock serialization), B commits first. Assert: WS client receives seq=N+1 (single delivery), receives seq=N (single delivery), AND `atc_pg_drain_duplicate_skipped_total` is incremented exactly once (the rescan re-fetches seq=N+1 and the ring-buffer suppresses the duplicate broadcast). Implementation: `tokio::join!` with explicit `BEGIN` … `INSERT INTO outbox … RETURNING seq` … `pg_notify` … `COMMIT` SQL to bypass the handler and force the race. |
| T6b | `phase_3c_gap_healing::rescan_paginates_correctly_across_more_than_one_batch` | Force a rescan window larger than `DRAIN_BATCH_SIZE` (500). Set up: insert 600 outbox rows with seqs 1..=600 in committed state, then start the drain with `initial_watermark=600` and the dedup ring already containing seqs 1..=600 (simulate the post-forward state); inject a low NOTIFY `seq=1` via `min_pending_seq.fetch_min(1)` to simulate a delayed commit becoming visible. Assert: the resulting rescan SELECTs all 600 rows in two batches (500 + 100) without skipping seqs 501..=600. The dedup ring skips broadcasts for all 600 (already-seen), but the test asserts the page-cursor advances correctly batch-to-batch by inspecting `atc_pg_drain_rows_total` (must increment by 600, not 500). Guards against the page-cursor-vs-watermark hazard. |
| T7 | `phase_3c_gap_healing::min_pending_seq_swap_resets_on_drain_pass` | Direct unit-style test on the listener+drain interaction: write seq=5 via fetch_min from a fake listener; assert drain pass reads backstop=5 and resets atomic to MAX. |
| T8 | `phase_3c_readyz::heartbeat_stale_returns_503` | Spawn drain task, kill it (or freeze its heartbeat), wait 31s (use `tokio::time::pause()` and advance), `/readyz` returns 503. |
| T9 | `phase_3c_readyz::heartbeat_fresh_returns_200` | Heartbeat updated within the last 30s → 200. |
| T10 | `phase_3c_restart_recovery::no_historical_replay_after_restart` | Commit rows directly to PG (bypassing handler), then start a fresh server and connect a WS client AFTER startup. Fire one more webhook. Assert: WS client receives only the post-startup event, NOT the rows committed before startup. (The initial watermark `MAX(seq)` at boot causes the drain to skip historical rows on its first pass.) Verifies ADR 0002 Decision 5 "no historical replay to live WS clients." |
| T11 | `phase_3c_row_lock_serialization::same_entity_concurrent_commits_arrive_in_seq_order` | Fire two webhooks for the **same run** (e.g., A=`Requested→Queued`, B=`InProgress`) on overlapping handler tasks via `tokio::join!`. Assert: seq=N (A's outbox row) is allocated and committed before seq=N+1 (B's), the drain forwards them in seq order to the WS client, no rescan duplicate metric increments. This is the test obligation for the row-lock claim in §D3 — same-entity events serialize at the DB and never produce backward gap-fills. |

**Existing tests:** sweep `notify_listener_tests.rs` and `outbox_tests.rs` — anything that asserted "drain logs but does not forward" now needs to assert forwarding. Update assertions, don't delete tests.

## Documents to Update

The doc-staleness gate (`scripts/check-docs-lefthook.sh` + `scripts/doc-mapping.sh`) will block the push if these aren't updated alongside the code. Plan:

| Doc | Update |
|-----|--------|
| `docs/architecture/backend-server.md` | Update Domain Model + Outbox Forwarder sections: `/v1/state` reads from PG (REPEATABLE READ), `/v1/ws` is fed by drain task with bounded ring-buffer dedup and listener-driven gap-healing via `min_pending_seq`. Document the `placeholder` runs column added in migration 0003 and the read-side filter. |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Mark Phase 3c "Done" with status notes: (1) bounded ring-buffer dedup (2048 seqs) added beyond the doc's spec to preserve ADR 0003 single-delivery under gap-healing rescans; (2) in-memory `seq` mutex / `webhook_tx` / `store` retained for dev mode (intentional scope reduction from "remove in-memory store" in the doc); (3) PG-side TTL eviction still deferred to Phase 5. |
| `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` | Append a "Phase 3c implementation notes" section: REPEATABLE READ on snapshot reads is now the contract; describe the bounded ring-buffer that preserves the no-frontend-dedup stance under gap-healing rescans. |
| `backend/crates/atc-server/CLAUDE.md` | Update the listener/drain-task summary to reflect actual forwarding (no longer a stub), the dedup ring buffer, the heartbeat-tick timer, and the `placeholder` column flow through `upsert_job_in_txn`'s stub-run INSERT. |
| `backend/crates/atc-server/migrations/CLAUDE.md` (if it exists; otherwise inline in `atc-server/CLAUDE.md`) | Add the `placeholder` column rationale and link to the read-filter. |

`scripts/doc-mapping.sh`: confirm the listener.rs / routes.rs / persist.rs entries already point at `backend-server.md`. If not, add them.

## Verification

End-to-end smoke after implementation:

1. `cargo sqlx prepare --workspace` (must succeed; commits new `.sqlx/` JSON).
2. `just check` (cargo check + sqlx offline mode).
3. `just lint` (clippy + rustfmt).
4. `just test` (workspace tests; this triggers the testcontainers tier).
5. `just types` (regenerate TS — should be a no-op since wire contract is unchanged).
6. `just dev`, then in another terminal:
   - `curl http://localhost:8080/v1/state` → returns `{ lastSeq: 0, runs: [], jobs: [] }` against an empty PG.
   - `wscat -c ws://localhost:8080/v1/ws` → idle.
   - `curl -X POST http://localhost:8080/v1/webhooks/github -H 'X-GitHub-Event: workflow_run' -d @fixtures/run-requested.json` → 200.
   - WS shows one `SeqEvent` with `seq: 1`.
   - `curl http://localhost:8080/v1/state` → `lastSeq: 1`, run present.
   - Restart the server; WS reconnects; `lastSeq: 1` still in snapshot.
7. `curl http://localhost:8080/readyz` returns 200; if you `pg_terminate_backend` the listener connection and wait 31s, it returns 503.

## Risks & Open Questions

- **Concurrent-commit gap-healing relies on `pg_notify` ordering relative to the listener's `fetch_min`.** If a NOTIFY is dropped (PG `LISTEN/NOTIFY` is best-effort under extreme load), a row could be missed permanently. Mitigation: the periodic SQL DELETE eviction task (Phase 5) will need an analogous "scan for orphans below watermark" sweep. Out of scope for 3c.
- **Row-lock safety argument is scoped to webhook-handler writes only.** §D3 relies on PostgreSQL row-level locks serializing same-entity UPSERTs. This holds for the current handler path (`upsert_*_in_txn`). Future backfill tooling, replay flows, or out-of-band outbox inserts would need to either (a) go through the same transactional helpers, or (b) include the entity's predecessor predicate to maintain seq-order delivery. If neither, the frontend would need a `highestAppliedSeq` per-entity guard. Out of scope for Phase 3c; flag in any future PR that adds backfill/replay code.
- **`/readyz` heartbeat is now baked into the drain-task algorithm via a 5s `tokio::select!` arm.** Quiet-period flap risk addressed in the pseudocode (heartbeat ticks at the top of every loop iteration, regardless of NOTIFY arrival). 30s staleness threshold gives 6× margin over the 5s tick. Test T8 forces threshold expiry by pausing tokio time and advancing past 30s.
- **`min_pending_seq` thread-safety.** `swap(MAX, AcqRel)` on the drain side and `fetch_min(seq, Release)` on the listener side. No producer-side reset path, so no clobber risk. The atomic monotonically decreases between drain swaps and resets to MAX at each swap. Listener side uses Release ordering so the atomic update happens-before the `drain_notify.notify_one()` that the drain's Acquire will synchronize with.

## Out of Scope (Phase 3c)

- Removing the in-memory store, `seq` mutex, or `webhook_tx` from `AppState`. They stay for in-memory mode (`pg_pool: None`).
- PG-side TTL eviction (Phase 5).
- Multi-replica deployment (Phase 4 — needs Helm + replicaCount > 1 contract).
- `min_pending_seq` durability across restarts (init to `i64::MAX` is correct: at boot there are no in-flight handlers).
