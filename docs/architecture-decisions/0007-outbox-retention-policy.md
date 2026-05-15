# ADR 0007 — Outbox retention policy

Date: 2026-05-14
Status: Accepted

## Context

The Postgres `outbox` table (`backend/crates/atc-server/migrations/0002_outbox.sql`) is append-only and unbounded. Every webhook commit writes one row; nothing trims it. [ADR 0003](0003-state-cursor-contract-and-operator-policy.md) Decision 4 deferred retention design because runs/jobs TTL eviction (in-memory mode only) and outbox retention exist for different reasons:

- Runs/jobs TTL bounds the working-set memory of the in-memory store.
- The outbox is the durable broadcast log on the PG path; its lifetime is bounded by how long a replica's drain task can be behind before a row is safe to delete.

[ADR 0006](0006-stores-own-background-task-lifecycle.md) landed the architectural seam this ADR builds on: each `PersistentStore` owns its own background-task lifecycle, so `PgStore` is the natural owner of an outbox retention task in symmetry with `InMemoryStore::spawn_eviction`.

Issue [#67](https://github.com/coderinserepeat/atc/issues/67) is the ticket for this work.

## Decision

The outbox is retired via **time-based retention with a multi-replica watermark floor**, implemented as two background tasks owned by `PgStore`: a per-replica heartbeat that maintains a new `outbox_watermarks` table, and a sweep task that runs the deletion statement on every replica symmetrically — *not* via a leader-elected sweeper.

### Mechanism

A new `outbox_watermarks(replica_id TEXT PK, broadcast_watermark BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL)` table records each live replica's commit-order cursor. Every `PgStore::start_inner` generates a `<hostname>-<uuid8>` `replica_id`, then spawns:

1. **Heartbeat task** — every 30 s, `UPSERT (replica_id, broadcast_watermark.load(Acquire), clock.now())`. Also refreshes two atomic mirrors used by the retention gauges (`atc_pg_outbox_min_replica_watermark`, `atc_pg_outbox_oldest_row_age_seconds`). First iteration runs unconditionally so dashboards and tests observe the row within milliseconds of startup.
2. **Sweep task** — every 300 s, runs the deletion statement:

   ```sql
   WITH victims AS (
     SELECT seq FROM outbox
     WHERE inserted_at < $1            -- $1 = clock.now() - retention
       AND seq <= (
         SELECT COALESCE(MIN(broadcast_watermark), 0)
         FROM outbox_watermarks
         WHERE updated_at > $2          -- $2 = clock.now() - stale_threshold (90 s)
       )
     ORDER BY seq
     LIMIT $3                            -- $3 = sweep_max_rows (10 000)
     FOR UPDATE SKIP LOCKED
   )
   DELETE FROM outbox WHERE seq IN (SELECT seq FROM victims)
   RETURNING seq;
   ```

   The sweep also piggybacks `DELETE FROM outbox_watermarks WHERE updated_at < $4` (`$4 = clock.now() - 1h`) to clean up dead replica rows.

Defaults: retention = 7 days (tunable via `ATC_OUTBOX_RETENTION`, humantime); heartbeat = 30 s; stale threshold = 90 s; sweep interval = 300 s; sweep cap = 10 000 rows / tick; watermark cleanup window = 1 h.

### Locked design decisions

1. **Time-based primary, watermark floor secondary.** Time gives operators a calendar-time mental model (default 7 d); the watermark `AND` clause is the invariant that protects multi-replica correctness — a row whose `seq` exceeds the lagging replica's broadcast cursor must not be deleted, no matter how old it is.

2. **Symmetric every-replica sweep, *not* an advisory-lock pseudo-leader.** [ADR 0002](0002-state-externalization-postgres-outbox.md) Decision 5 explicitly stepped away from leader semantics; the watermark table doubles as a cluster-wide replica-health surface (`atc_pg_outbox_min_replica_watermark` is the cluster floor observable from any replica). The ordered-CTE + `FOR UPDATE SKIP LOCKED` shape eliminates deadlock and wasted scan work that an unordered DELETE would invite under contention.

3. **Hard 1 h retention floor enforced at startup.** `PgStore::start_inner` returns `PgStoreStartError::RetentionTooShort` if `outbox_retention < 1h`. Operators see the failure at process startup, not silently degraded retention. The floor exists because Postgres `inserted_at` defaults to `transaction_timestamp()` (transaction-start time), not commit time. Under MVCC, a long-held writer transaction can commit a row whose `inserted_at` is already past `now() - retention` for sub-hour retentions — the row would commit, become visible, and immediately satisfy the deletion predicate before any replica had a chance to drain it. 1 h dominates any practical writer transaction in this codebase (webhook handlers commit within milliseconds).

   We rejected a softer "warn but accept" check: Postgres defaults `idle_in_transaction_session_timeout`, `statement_timeout`, and `transaction_timeout` to `0` (disabled), so probing them at startup isn't a real defense. A hard floor is operationally cheaper than a probe + warning that operators won't see.

4. **Rust-bound clocks throughout — no SQL `now()` on the retention path.** Every cutoff (`retention_cutoff`, `stale_cutoff`, `watermark_cleanup_cutoff`, and the heartbeat UPSERT's `updated_at`) is bound from `Clock::now()`. The migration omits `DEFAULT now()` on `outbox_watermarks.updated_at` to force callers to bind explicitly. `TestClock`-driven integration tests are deterministic as a result, matching the discipline locked in by [ADR 0006](0006-stores-own-background-task-lifecycle.md) and the `eviction.sweep` precedent.

5. **Per-tick root spans, *not* task-lifetime roots.** `outbox.heartbeat.tick` and `outbox.sweep.tick` are decorated as `#[tracing::instrument(...)]` async functions; the spawn site has no `.instrument(span)` wrapper. A task-lifetime root that never ends until process shutdown would hold the unfinished span in SDK memory for the pod lifetime, give every tick a single trace id for the entire process, and be lost on SIGKILL/OOM. This matches the precedent set by `eviction.sweep` / `listener.recv` / `drain.pass` and PR #170, which retrofit the same pattern onto `listener.task` and `drain.task`.

6. **No partition rotation, no `pg_cron`, no FK changes.** Partition rotation is the natural follow-up if outbox write volume grows past ~10 M rows/day, but DELETE-based retention is operationally simpler at current scale and avoids a vacuum-tuning conversation.

## Consequences

### Positive

- **Unbounded growth eliminated.** At a 100 webhooks/s sustained rate with 7 d retention, the outbox cap is ~60 M rows — well within Postgres's comfortable single-table range.
- **Multi-replica safe by construction.** Cluster-wide `MIN(broadcast_watermark)` is the floor under which deletions can happen; a lagging or partitioned replica simply ages out of the floor calculation after `stale_threshold` and stops blocking deletion. Recovering replicas re-seed `broadcast_watermark` from `MAX(seq)` over the surviving outbox rows, so they don't try to re-broadcast retired seqs.
- **Operator surface is one humantime env var.** Defaults are safe; tuning is calendar-time obvious; the 1 h floor stops dangerous overrides at startup.
- **Cluster-health observability bundled in.** `outbox_watermarks` is, incidentally, a queryable "list of live replicas" surface for ad-hoc debugging.

### Negative

- **`PgStore` now owns four background tasks (listener, drain, heartbeat, sweep) instead of two.** Shutdown grew two more cooperative-join steps and two more timeout constants. Worst-case shutdown budget is unchanged because the new tasks cooperate on the same cancellation token and exit within milliseconds; the join budgets (2 s each) are slack, not expected wait.
- **Operators on sub-hour retention need to file an issue and adopt partition rotation.** This is a real constraint — it makes ATC unsuitable for the "I want to keep one hour of outbox and delete the rest immediately" use case. We accept this in exchange for not having a silent-data-loss footgun under MVCC.
- **Heartbeat-task DB cost.** Three small queries every 30 s per replica (UPSERT + two SELECT MIN scans). Indexable + small; trivial under normal load. If it ever shows up in profiling, the cadence is a one-liner change.

### Future work captured separately

- **Partition rotation.** Re-evaluate `pg_partman` / native RANGE partitioning by week if outbox write volume crosses ~10 M rows/day.
- **Operator-tunable internal cadences.** Heartbeat / stale / sweep intervals are hardcoded for v1; a future ADR can promote them to env-var surface if operators discover real need.
- **Cross-replica audit / debug tooling.** Reading historical outbox after retention is a different consumer (forensic, ops); not in scope here.

## References

- Issue: [#67](https://github.com/coderinserepeat/atc/issues/67)
- Design plan: [`docs/design-plans/2026-05-14-outbox-retention.md`](../design-plans/2026-05-14-outbox-retention.md)
- Implementation: `backend/crates/atc-server/src/persist/pg.rs` (`spawn_outbox_heartbeat`, `spawn_outbox_sweep`, `OUTBOX_RETENTION_FLOOR`)
- Migration: `backend/crates/atc-server/migrations/0004_outbox_watermarks.sql`
- Operator surface: [`docs/architecture/deployment.md`](../architecture/deployment.md) § `ATC_OUTBOX_RETENTION`
- Metrics + spans: [`docs/architecture/metrics.md`](../architecture/metrics.md) § Operational metrics, § Span inventory
- Related ADRs: [0002](0002-state-externalization-postgres-outbox.md) (PG outbox externalisation), [0003](0003-state-cursor-contract-and-operator-policy.md) D4 (deferred retention), [0006](0006-stores-own-background-task-lifecycle.md) (store-owned lifecycle).
