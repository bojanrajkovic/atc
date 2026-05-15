---
issue: 67
slug: outbox-retention
status: draft
---

## Context

The Postgres `outbox` table is append-only and unbounded; nothing trims it today. ADR 0003 Decision 4 explicitly deferred retention design — outbox lifetime is decoupled from runs/jobs TTL because the two exist for different reasons. With ADR 0006 landed (eviction machinery now lives inside the stores) the architectural seam exists for `PgStore` to own its own sweep task in symmetry with `InMemoryStore::spawn_eviction`.

Intended outcome: bounded growth of the `outbox` table, safe under multi-replica writers, with operator-tunable retention and observable behavior — without reintroducing a leader concept.

This plan resolves issue [#67](https://github.com/coderinserepeat/atc/issues/67).

## Definition of Done

1. New migration `0004_outbox_watermarks.sql` creates the `outbox_watermarks` table (`replica_id TEXT PRIMARY KEY, broadcast_watermark BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL`, no `DEFAULT now()`).
2. `PgStore` spawns a heartbeat task (30s cadence) that upserts the local replica's row in `outbox_watermarks` with `updated_at` bound from `Clock::now()`.
3. `PgStore` spawns a sweep task (300s cadence) that deletes outbox rows older than `outbox_retention` AND with `seq <= MIN(broadcast_watermark)` across non-stale replicas, using an ordered-CTE + `FOR UPDATE SKIP LOCKED` for multi-replica safety.
4. `PgStore::start_inner` returns a fatal config error if `outbox_retention < 1h`. Verified by an integration test asserting `30m` fails to start.
5. Three new metric instruments are emitted: `atc_pg_outbox_rows_deleted_total` (counter), `atc_pg_outbox_min_replica_watermark` (observable gauge, atomic-mirrored), `atc_pg_outbox_oldest_row_age_seconds` (observable gauge, atomic-mirrored). Both gauges render NaN when there is no live data.
6. Heartbeat span (`outbox.heartbeat.tick`) and sweep span (`outbox.sweep.tick`) are per-tick roots (no task-lifetime parent), mirroring the precedent set by `eviction.sweep` / `listener.recv` / `drain.pass`.
7. Config surface: `ATC_OUTBOX_RETENTION` env var (humantime-parseable, default `7d`) wired through `Config::outbox_retention`.
8. Helm chart surfaces `config.outboxRetention` in `values.yaml` (default `7d`) and adds it under the `config` block in `values.schema.json`; `templates/deployment.yaml` maps to `ATC_OUTBOX_RETENTION`.
9. New ADR `0007-outbox-retention-policy.md` records the decision; ADR 0003 D4 gets a postscript pointing at 0007.
10. Architecture doc updates: `backend-server.md` describes the retention tasks in the persist section; `metrics.md` adds the three new instruments to the operational-metrics catalog AND the two new spans to the span inventory; `deployment.md` documents the 1h floor and the rolling-deploy assumption.
11. `backend/crates/atc-server/CLAUDE.md` Spans bullet and Modules table reference the new tasks; `shutdown.rs` "no live emitter when shutdown fires" comment block names the heartbeat + sweep tasks alongside listener + drain.
12. `cargo sqlx prepare --workspace` is run after all new `query!`/`query_scalar!` macros are added; offline cache is committed.
13. All existing tests pass; new tests (heartbeat upsert, sweep positive/negative matrix, contention, multi-replica, post-eviction watermark re-seed, config-floor fatal) pass.

## Locked Decisions

The following are not open for re-evaluation during implementation.

1. **Retention = time-based primary, watermark floor secondary.** `DELETE … WHERE inserted_at < now() - retention AND seq <= MIN(replica watermarks)`. Time gives operators a calendar-time mental model (default 7d); the watermark `AND` clause is the invariant that protects multi-replica correctness.

2. **Symmetric every-replica sweep via `outbox_watermarks` table** — *not* an advisory-lock pseudo-leader. Postgres row-locking + `SKIP LOCKED` serializes deletes; the watermark table doubles as a cluster-wide health surface. *Source*: ADR 0002 D5 (symmetric replicas); ADR 0006 (stores own background-task lifecycle).

3. **Default retention = 7 days.** Operator-tunable via `ATC_OUTBOX_RETENTION` (humantime-parseable: `7d`, `24h`, etc.).

4. **All implementation in one PR.** Heartbeating without a sweep is pure write amplification; sweeping without heartbeats is unsafe. The six components (migration, heartbeat task, sweep task, config, metrics, Helm) are interlocked.

5. **No leader election, no `pg_cron`, no FK changes.** Partition rotation is out of scope and re-evaluable if write volume grows materially.

6. **Retention must dominate the longest practical uncommitted-transaction lifetime — enforced by a hard floor.** `inserted_at` defaults to `now()` at row insert, which is *transaction-start* time, not commit time. A row from a long-held write transaction is MVCC-invisible until commit, then materializes with a stale `inserted_at`. If `retention < max(uncommitted_tx_duration)`, the row can commit, become visible, and immediately satisfy the `inserted_at < now() - retention` predicate before any replica has drained it. Postgres defaults `idle_in_transaction_session_timeout`, `statement_timeout`, and `transaction_timeout` to `0` (disabled), so probing them at startup is not a real defense.

   **Enforcement**: `PgStore::start_inner` returns `PgStoreStartError::RetentionTooShort` if `outbox_retention < 1h`. 1h is the supported floor; sub-floor values are explicitly unsupported. The 1h floor comfortably dominates any practical writer transaction in this codebase (webhook handlers commit within milliseconds); operators who need shorter retention should file an issue and propose partition rotation instead.

   *Sources*: [Postgres runtime-config-client](https://www.postgresql.org/docs/current/runtime-config-client.html), [Postgres functions-datetime](https://www.postgresql.org/docs/current/functions-datetime.html).

7. **Cutoff timestamps are bound Rust-side, not via SQL `now()` — for every timestamp the retention path touches.** Both sweep statement cutoffs (`$1 = clock.now() - retention`, `$2 = clock.now() - stale_threshold`), the heartbeat UPSERT's `updated_at = $clock.now()`, and the piggyback watermark-cleanup cutoff are bound from `Clock::now()`. SQL `now()` is wall-clock and indifferent to `TestClock`; this discipline matches ADR 0006 and the eviction-fold plan. The `outbox_watermarks` table is created with `updated_at TIMESTAMPTZ NOT NULL` (no `DEFAULT now()`) so callers cannot accidentally let the DB pick the value.

8. **Sweep uses an ordered candidate-selection CTE with `FOR UPDATE SKIP LOCKED`.** Postgres does not guarantee row-visitation order in a bare `DELETE` without `ORDER BY`, so the "no deadlock under contention" claim requires `SKIP LOCKED` semantics to be load-bearing. The CTE shape also implements the `sweep_max_rows` cap and yields a `RETURNING seq` row count for the deleted-rows counter without depending on `sqlx`'s `affected_rows` reporting under `WITH`/`RETURNING`. *Sources*: [queries-order](https://www.postgresql.org/docs/current/queries-order.html), [locking-indexes](https://www.postgresql.org/docs/current/locking-indexes.html).

9. **Per-tick root spans, not task-lifetime roots.** Heartbeat emits `outbox.heartbeat.tick` per tick (no parent); sweep emits `outbox.sweep.tick` per tick (no parent). *Source*: `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md` head note; superseded `listener.task` / `drain.task` retrofit captured as a follow-up (now landed via #170 — task-lifetime roots dropped on listener and drain).

10. **New ADR, not amendment.** `docs/architecture-decisions/0007-outbox-retention-policy.md`. Repo precedent favors a new ADR for substantive new policy. ADR 0003 D4 gets a postscript link.

11. **Rolling-deploy assumption: retention >> 3 × heartbeat_interval.** During a rolling deploy, old-version replicas (no heartbeat) coexist briefly with new-version replicas. With the 7d default this is a non-issue. With sub-hour retention (blocked at <1h by Locked Decision 6), a deploy could in principle delete rows the old replicas haven't yet broadcast. The combination of the 1h hard floor + 90s stale threshold means a deploy must finish within 1h to be safe — well within typical Helm rolling-update timeouts. Documented in `deployment.md`'s operator surface.

12. **Implementation is inline in `pg.rs`, not a new `pg/retention.rs` file.** `scripts/doc-mapping.sh` currently maps `persist/pg.rs` to both `backend-server.md` and `metrics.md`; a nested `pg/retention.rs` would require a doc-mapping update for no architectural benefit.

## Architecture

### Sweep statement

Cutoffs bound Rust-side via `Clock` (Locked Decision 7); ordered candidate-selection CTE with `SKIP LOCKED` (Locked Decision 8):

```sql
WITH victims AS (
  SELECT seq
  FROM outbox
  WHERE inserted_at < $1   -- $1 = clock.now() - retention
    AND seq <= (
      SELECT COALESCE(MIN(broadcast_watermark), 0)
      FROM outbox_watermarks
      WHERE updated_at > $2   -- $2 = clock.now() - stale_threshold
    )
  ORDER BY seq
  LIMIT $3                   -- $3 = sweep_max_rows
  FOR UPDATE SKIP LOCKED
)
DELETE FROM outbox WHERE seq IN (SELECT seq FROM victims)
RETURNING seq;
```

Under simultaneous execution from all replicas:

- The CTE locks candidate `seq` values in monotonic order via the PK index, with `SKIP LOCKED` so a second sweeper acquires a disjoint set rather than waiting.
- Each sweeper deletes its locked subset; total work is bounded by `sweep_max_rows` per replica per tick.
- **No deadlock**: ordered acquisition + `SKIP LOCKED` rule out the cycle that an unordered DELETE could form against a concurrent writer holding a row lock further up the seq range.
- **No wasted scan work**: the second sweeper's CTE skips the rows the first is holding, instead of finding them all gone.
- **No correctness risk**: rows ineligible for deletion (seq > min watermark, or `inserted_at` within retention) never enter the candidate set.

### Constants (hardcoded; not operator-tunable)

| Name | Value | Reason |
|------|-------|--------|
| `heartbeat_interval` | 30 s | Frequent enough to keep `min_replica_watermark` close to live state; cheap UPSERT. |
| `stale_threshold` | 90 s | 3× heartbeat — tolerates one missed tick before excluding a replica from the floor. |
| `sweep_interval` | 300 s | Every 5 minutes is sufficient for a 7-day retention window. |
| `sweep_max_rows` | 10 000 | Bounds per-tick work under traffic spikes; total deleted bounded by `sweep_max_rows × replicas`. |
| `watermark_cleanup_window` | 1 h | 30× stale_threshold margin; safely cleans up dead replica rows without false-positives. |

### Replica identity

`replica_id = format!("{}-{}", hostname_or_unknown, &uuid::Uuid::new_v4().simple().to_string()[..8])`. UUID suffix prevents collision when two `PgStore` instances run against the same database with the same hostname (multi-instance dev, integration tests).

### Sequencing diagram

```
3 replicas A/B/C; heartbeat=30s; stale_threshold=90s; sweep_interval=300s

t=0    A: bcast seq=100        B: bcast seq=100        C: bcast seq=100
t=30   HB(A,100,30)            HB(B,100,30)            HB(C,100,30)
t=60   A: bcast 200            B: bcast 200            C: NETWORK PARTITION
       HB(A,200,60)            HB(B,200,60)            (no HB)
t=150  HB(A,200,150)           HB(B,200,150)           (C row updated_at=30 → stale)

t=300  SWEEP TICK on A and B simultaneously:
       ┌── A's tx ──────────────────────────────────────────────────┐
       │ SELECT MIN(wm) WHERE updated_at > now()-90s  → 200          │
       │   (C excluded: 150-30=120 > 90)                              │
       │ DELETE FROM outbox WHERE inserted_at < now()-7d              │
       │                     AND seq <= 200                            │
       │   row locks acquired in seq order via PK index                │
       │ COMMIT (k rows deleted)                                       │
       └────────────────────────────────────────────────────────────────┘
       ┌── B's tx (SKIP LOCKED → disjoint candidate set) ─────────────┐
       │ SELECT MIN(wm) → 200                                          │
       │ DELETE … → rows A didn't lock                                 │
       │ COMMIT                                                        │
       └────────────────────────────────────────────────────────────────┘

t=350  C recovers from partition.
       PgStore::start_inner already seeds broadcast_watermark from
       COALESCE(MAX(seq), 0) FROM outbox (pg.rs:281-328). C never reads
       seq ≤ 200 again. WebSocket clients on C reconnected to A/B during
       the partition and rebuilt state via /v1/state snapshot.
```

### Span shape

Both new tasks emit per-tick root spans (no task-lifetime parent):

- `outbox.heartbeat.tick` — fields: `replica_id`, `broadcast_watermark`, `min_replica_watermark`, `oldest_row_age_seconds`. Emitted from a `#[tracing::instrument(...)]`-decorated async function that the heartbeat task calls on each tick.
- `outbox.sweep.tick` — fields: `cutoff_age_seconds`, `min_replica_watermark`, `rows_deleted`, `watermarks_cleaned`. Same pattern.

Why per-tick: a long-lived `.instrument(span)` at the spawn site would never close under normal operation, holding the unfinished span in SDK memory for the pod's lifetime and losing it entirely on SIGKILL/OOM. See `docs/architecture/metrics.md` § "Task-lifetime root spans are an anti-pattern".

### Why not an advisory-lock pseudo-leader

- Reintroduces a leader-shaped concept that ADR 0002 D5 explicitly stepped back from.
- Solves only deletion coordination; the watermark table also provides cluster-wide replica health visibility.
- Failure modes are no simpler — advisory locks release on connection close, which is the same handoff story as "next replica sweeps on next tick."
- With ordered-CTE + `FOR UPDATE SKIP LOCKED`, concurrent sweepers don't waste work either.

## Critical Files

### New
- `backend/crates/atc-server/migrations/0004_outbox_watermarks.sql` — new table; `cargo sqlx prepare` regeneration required.
- `docs/architecture-decisions/0007-outbox-retention-policy.md` — new ADR.

### Modified
- `backend/crates/atc-server/src/persist/pg.rs` — `PgStore::start_inner` validates retention floor (≥ 1h), spawns the two new tasks, tracks JoinHandles, owns the gauge atomics. Heartbeat + sweep implementations inline. New `PgStoreStartError::RetentionTooShort` variant. `shutdown()` joins both new tasks.
- `backend/crates/atc-server/src/shutdown.rs` — extend "no live emitter when shutdown fires" comment block (lines 225-237) to name the heartbeat + sweep tasks. New `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT` and `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP` constants (2s each).
- `backend/crates/atc-server/src/config.rs` — add `outbox_retention: humantime_serde::Duration` (env `ATC_OUTBOX_RETENTION`, default `7d`). Validation enforced inside `PgStore::start_inner` per Locked Decision 6.
- `backend/crates/atc-server/src/main.rs` — pass `cfg.outbox_retention` into `PgStore::start`.
- `backend/crates/atc-server/src/metrics.rs` — register three new instruments via cached-instrument pattern.
- `backend/crates/atc-server/Cargo.toml` — add `humantime-serde` and `uuid` (`v4` + `fast-rng` features).
- `deploy/helm/atc/values.yaml` — surface `config.outboxRetention` (default `"7d"`).
- `deploy/helm/atc/values.schema.json` — declare `outboxRetention` under `config` (`type: string`, humantime regex hint).
- `deploy/helm/atc/templates/deployment.yaml` — map to `ATC_OUTBOX_RETENTION` env var.
- `docs/architecture/backend-server.md` — describe retention tasks in persist module section.
- `docs/architecture/metrics.md` — add three instruments to operational-metrics catalog; add two spans to span inventory.
- `docs/architecture/deployment.md` — operator surface for `ATC_OUTBOX_RETENTION`, the 1h floor, the rolling-deploy assumption.
- `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` — postscript on Decision 4 pointing at ADR 0007.
- `backend/crates/atc-server/CLAUDE.md` — Modules table mentions retention tasks; Spans bullet adds `outbox.heartbeat.tick` and `outbox.sweep.tick`.

### Reused (do not reimplement)
- `atc-core::clock::Clock` (`backend/crates/atc-core/src/clock.rs`) — wall-clock abstraction. `SystemClock` in prod, `TestClock` in tests.
- `PgMetrics` cached-instrument + `Weak<AtomicI64>` callback pattern in `metrics.rs:163-302`.
- `tokio::time::interval` + `tokio_util::sync::CancellationToken` pattern from `spawn_listener_task` / `spawn_drain_task`.
- `serial_test::serial` + `OnceLock` exporter for integration tests with PG container (per `atc-server/CLAUDE.md` § Testing).

## Implementation Phases

**Phase 1 — design plan + branch.** Branch `feat/outbox-retention` already created. This document is the Phase 1 artifact.

**Phase 2 — migration.** Write `0004_outbox_watermarks.sql`:
```sql
CREATE TABLE outbox_watermarks (
    replica_id          TEXT        PRIMARY KEY,
    broadcast_watermark BIGINT      NOT NULL,
    updated_at          TIMESTAMPTZ NOT NULL
);
```
No `DEFAULT now()` on `updated_at` (Locked Decision 7). No index beyond the PK. `cargo sqlx prepare --workspace` cache regeneration deferred to end of Phase 5 (single pass over all new macros).

**Phase 3 — heartbeat task (TDD).**

Failing test first: starting from a fresh database, start a `PgStore`, assert that within `≤ heartbeat_interval + slack` the `outbox_watermarks` table contains a row for this replica with `broadcast_watermark = COALESCE(MAX(seq), 0)`.

Implementation:
- Add `uuid` (`v4`) dependency.
- `PgStoreStartError::RetentionTooShort { configured, minimum }` variant added.
- `PgStore::start_inner` accepts `outbox_retention: Duration`; validates `>= Duration::from_secs(3600)` and returns the new variant if not.
- `PgStore::start_inner` generates `replica_id` and stores it on `PgStore`.
- New atomics `min_replica_watermark_atomic: Arc<AtomicI64>` and `oldest_row_age_seconds_atomic: Arc<AtomicI64>` initialised to `-1` (NaN sentinel — Locked Decision: pre-first-tick state is NaN, not 0).
- `PgStore::spawn_outbox_heartbeat(...)` returns `JoinHandle<()>`. Loop body: cancel/tick `select!`, then per-tick async function decorated with `#[tracing::instrument(name = "outbox.heartbeat.tick", skip_all, fields(...))]`. UPSERT binds `(replica_id, broadcast_watermark.load(Acquire), clock.now())`. Same tick refreshes the two atomics via:
  - `SELECT COALESCE(MIN(broadcast_watermark), -1) FROM outbox_watermarks WHERE updated_at > $stale_cutoff`
  - `SELECT EXTRACT(EPOCH FROM ($clock_now - MIN(inserted_at))) FROM outbox` (returns NULL if empty → store `-1`)
- `PgStoreHandles` extended with `heartbeat: JoinHandle<()>`.
- `PgStore::shutdown()` joins `heartbeat` with `SHUTDOWN_TIMEOUT_OUTBOX_HEARTBEAT`.

**Phase 4 — sweep task (TDD).**

Failing tests first (all using Rust-bound timestamps, per Locked Decision 7):
- Positive: insert rows with `inserted_at = clock.now() - 8d`, set heartbeats above N via the same UPSERT path, tick sweep, assert rows gone + counter incremented.
- Negative: rows with `inserted_at = clock.now() - 1h` (within retention) survive.
- Negative: rows with `seq > MIN(watermark)` survive.
- Negative: no fresh heartbeats → CTE's `COALESCE(MIN(...), 0)` = 0 → `seq <= 0` matches nothing → 0 rows deleted.
- Contention: two `PgStore` instances, one round of sweep, total deleted = expected; per-instance shares are disjoint (`SKIP LOCKED` semantics) — verify via per-instance counter increments.
- Post-eviction watermark re-seed: insert N rows, sweep deletes them, restart `PgStore`, assert `broadcast_watermark.load(Acquire)` after `start_inner` equals `MAX(seq)` over surviving rows.

Implementation:
- `PgStore::spawn_outbox_sweep(...)` returns `JoinHandle<()>`. Same `select!` shape; per-tick async function decorated with `#[tracing::instrument(name = "outbox.sweep.tick", ...)]`.
- Sweep statement from the Architecture § above. `RETURNING seq` row count → `metrics.outbox_rows_deleted.add(n, &[])`.
- Piggyback `DELETE FROM outbox_watermarks WHERE updated_at < $clock_now - watermark_cleanup_window`.
- `PgStoreHandles` extended with `sweep: JoinHandle<()>`. `shutdown()` joins with `SHUTDOWN_TIMEOUT_OUTBOX_SWEEP`.

**Phase 5 — metrics.**

- `atc_pg_outbox_rows_deleted_total` (`Counter<u64>`, no attrs) — incremented by sweep task using `RETURNING seq` count.
- `atc_pg_outbox_min_replica_watermark` (`ObservableGauge<f64>`) — callback reads `Weak<AtomicI64>::upgrade().load(Acquire)`; populated by heartbeat task every 30 s. `-1` → NaN.
- `atc_pg_outbox_oldest_row_age_seconds` (`ObservableGauge<f64>`) — same shape. `-1` → NaN.

`PgMetrics::register_with_meter` extends to take two new `Weak<AtomicI64>` arguments. The 30-second refresh cadence is documented in `metrics.md` so operators know the gauges are not collection-tick-fresh.

**Phase 6 — config + Helm.**

- Add `humantime-serde` dependency.
- `Config::outbox_retention: std::time::Duration` with `#[serde(default = "default_outbox_retention", with = "humantime_serde")]`; default `Duration::from_secs(7 * 86400)`.
- `main.rs` passes `cfg.outbox_retention` into `PgStore::start`.
- Floor validation in `PgStore::start_inner` (already covered in Phase 3).
- Helm `values.yaml`: `outboxRetention: "7d"` under `config`.
- `values.schema.json`: add `outboxRetention` (`type: string`) under the `config` object.
- `templates/deployment.yaml`: `ATC_OUTBOX_RETENTION` env var from `.Values.config.outboxRetention`.
- Integration test: `Config::load` with `ATC_OUTBOX_RETENTION=30m` → store-start fails with `PgStoreStartError::RetentionTooShort`.

**Phase 7 — docs.**

- `backend-server.md`: persist module section names the two new tasks.
- `metrics.md`: § Operational metrics gets the three new instruments (full seven-element authoring contract for each); § Span inventory gets the two new spans.
- `deployment.md`: § Operator surface adds `ATC_OUTBOX_RETENTION`, the 1h floor, and the rolling-deploy assumption.
- ADR `0007-outbox-retention-policy.md`: documents the retention design.
- ADR 0003 D4: postscript pointing at 0007.
- `backend/crates/atc-server/CLAUDE.md`: Modules table mentions retention tasks; Spans bullet adds `outbox.heartbeat.tick` / `outbox.sweep.tick`.
- `shutdown.rs` comment block names the two new tasks.

**Phase 8 — final verification + Codex review.**

- `cargo sqlx prepare --workspace` at backend root; commit the cache diff.
- `just lint`, `just test` clean.
- Pre-push doc-staleness gate clean.
- Open PR; run `/codex-review-plan` PR-review variant against the implementation diff; triage and address.

## Verification

### Unit / integration (run via `just test`)
- Heartbeat upsert test (Phase 3).
- Sweep matrix tests (Phase 4: positive, three negatives, contention, post-eviction re-seed).
- Retention floor fatal: `30m` → store fails to start.
- End-to-end retention with `TestClock`: insert events, advance clock past retention, tick sweep, assert deletion + counter increment + outbox-lag metric stable.
- Multi-replica simulation: two `PgStore` instances against the same database, both run sweep idempotently; aggregate `atc_pg_outbox_rows_deleted_total` matches expected row count.

### End-to-end manual
- `just dev` against local Postgres (in-memory mode is single-replica; not relevant).
- `just otel-dev-stack` for collector + Grafana (no `/metrics` endpoint — metrics flow through OTLP).
- Drive webhooks via `curl` to populate outbox; `SELECT count(*) FROM outbox;` baseline.
- `SELECT * FROM outbox_watermarks;` shows dev replica heartbeating, `updated_at` advancing every 30 s.
- For fast verification of the sweep: insert old rows directly (`INSERT INTO outbox(...) VALUES (..., now() - INTERVAL '8d')`) since the 1h retention floor blocks shorter `ATC_OUTBOX_RETENTION` values.
- Wait one sweep interval (5 min); `SELECT count(*) FROM outbox;` drops.
- Grafana: `atc_pg_outbox_rows_deleted_total`, `atc_pg_outbox_min_replica_watermark`, `atc_pg_outbox_oldest_row_age_seconds` present and moving.
- Negative test: start with `ATC_OUTBOX_RETENTION=30m` — process must fail to start with a clear config error citing the 1h floor.

### Doc-staleness gate
- `just lint` and pre-push hook both clean via `scripts/check-docs-lefthook.sh`.

### Codex review pass
- `/codex-review-plan` PR-review variant against final implementation diff before merge.

## Out of Scope

- Partition rotation (`pg_partman` / native RANGE partitioning by week). Reconsider if outbox write volume grows past ~10 M rows/day.
- Operator-tunable values for heartbeat/stale/sweep cadence (hardcoded for v1).
- Cross-replica audit / debug tooling reading historical outbox.
- Foreign-key changes to `outbox` (none currently; none added).
- Renaming `outbox_watermarks` to a more general `replica_state` table for future cross-replica state.

## Docs to Update

- `docs/architecture/backend-server.md`
- `docs/architecture/metrics.md`
- `docs/architecture/deployment.md`
- `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` (postscript)
- `docs/architecture-decisions/0007-outbox-retention-policy.md` (new)
- `backend/crates/atc-server/CLAUDE.md`
- `backend/crates/atc-server/src/shutdown.rs` (emitter-comment block)
