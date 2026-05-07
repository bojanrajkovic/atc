# 0002 — PostgreSQL outbox + symmetric replicas for live state

**Status:** Accepted (Phase 1 of state-externalization rollout, 2026-05-03)

Last verified: 2026-05-04

## Context

ATC's backend currently keeps all live state in process memory. `atc-server`
holds an `Arc<StateStore>` plus a `Mutex<u64>` seq counter, and the webhook
handler holds that mutex across store mutation, seq assignment, and broadcast
to a `tokio::sync::broadcast` channel. `GET /v1/state` holds the same mutex
across snapshot read and seq read, which is what makes the current cursor /
snapshot / stream contract atomic.

This works for `replicaCount = 1`. It does not work for `replicaCount > 1`:

- each replica would maintain a disjoint `StateStore`
- WebSocket subscribers on replica A would never see events ingested by replica B
- the seq counter would diverge between replicas, breaking the snapshot/stream
  reconciliation protocol

This ADR makes the storage-architecture decisions that unblock multi-replica.
Issue [#7](https://github.com/bojanrajkovic/atc/issues/7) is the umbrella
work item. Design research is in
[`docs/architecture/state-externalization-research/`](../architecture/state-externalization-research/README.md).

This ADR is paired with [ADR 0003](./0003-state-cursor-contract-and-operator-policy.md),
which covers the wire-contract changes (cursor semantics, ordering relaxation,
frontend dedupe stance) and operator-surface decisions (Helm gating, SQLite
mode removal, retention). The two ADRs are interdependent — neither stands
alone — but they are split because the wire contract and operator policy are
storage-agnostic in spirit and may evolve on different timelines than the
underlying storage choice.

[ADR 0004](./0004-frontend-derived-pool-stats.md) supersedes this ADR's
original Decision 4 by moving runner pool stats derivation from the backend
to the frontend; the outbox now stores domain events only. Decision 4 below
reflects that supersession.

## Decision

### 1. Storage backend: PostgreSQL with current-state tables and a transactional outbox

Live state moves from the in-process `StateStore` to PostgreSQL:

- current-state tables keyed by GitHub identities (`run_id`, `job_id`)
- an append-only `outbox` (or `events`) table with an ATC-owned monotonic `seq`
- one database transaction per ingested domain event, performing both the
  current-state upsert and the outbox row insert before commit

Rejected alternatives, with the deciding reason:

- **Redis hashes + Pub/Sub** — at-most-once fan-out; no durable replay path
  for reconnects or new replicas
- **Redis hashes + Streams** — viable, but turns ATC's relational shape into
  manual Redis data modeling, and either changes the wire cursor from `u64`
  to a Redis Stream ID string or duplicates the counter
- **Queue-first (NATS JetStream / RabbitMQ) + DB projection** — strong for
  a future SaaS / multi-tenant ingestion pipeline, but introduces queue + DB
  for a problem that needs only one stateful dependency at ATC's current stage
- **Leader-routed in-memory store** — does not survive leader restart, does
  not give horizontal scaling of live state, and reintroduces a distinguished
  node into a system this work is trying to make symmetric

### 2. Atomicity: one transaction for current-state mutation + outbox append

The current-state row(s) and the outbox row are written in the same
transaction. This is the durable equivalent of the in-memory rule that the
seq mutex is held across store mutation, seq assignment, and broadcast.

Splitting these writes is forbidden: a `GET /v1/state` could otherwise observe
a committed `last_seq` whose corresponding state mutation has not yet
committed, violating the snapshot/cursor invariant.

**Concurrency control on the current-state mutation** uses atomic
`UPDATE ... WHERE status IN (allowed_predecessors)`, where the predecessor
set is parameterized from the existing Rust state machine
(`RunStatus::predecessors_of(target)` and equivalent for `JobStatus`). The
state machine itself stays in `atc-core` as the single source of truth; SQL
does not encode the transition rules, it consumes them as parameters at call
time. A `0 rows affected` result is the failure signal, mapped to today's
`StoreError::InvalidTransition`. First-sight creation uses
`INSERT ... ON CONFLICT (id) DO UPDATE ... WHERE jobs.status IN (predecessors)`
so the upsert respects the predicate when the row already exists. Idempotent
same-status replay is handled by including the target status in the
predecessor set or by detecting the no-op via the result.

Alternatives considered and recorded as fallbacks for transitions that prove
harder to express as a single `WHERE` predicate: `SELECT ... FOR UPDATE`
then validate-then-update in Rust (preserves rich validation error reasons
at the cost of an extra round-trip per write) and `SERIALIZABLE` isolation
with retry-on-conflict (simpler control flow at the cost of conflict aborts
under contention). The single-statement `UPDATE`-with-predicate is the
default because ATC's transitions are simple-predicate state machines,
contention is low, and same-row concurrency is rare.

### 3. Cross-replica wake-up: PostgreSQL `LISTEN/NOTIFY`, payload limited to a seq token

`NOTIFY` carries only a small token (`seq`, optionally a discriminator).
It does not carry the event payload.

Reasoning:

- `NOTIFY` payloads are limited to ~8000 bytes; job events with steps and
  `poolStatsAfter` exceed that budget without a hard guarantee
- treating the notification as a wake-up makes recovery straightforward —
  a missed `NOTIFY` is recovered by re-reading outbox rows by `seq`, the
  same path used at startup
- listener loss must not produce data loss; outbox replay does

**Connection-pool compatibility:** Each replica's database activity has two
paths with different pool requirements. Webhook writes, snapshot reads, and
outbox forwarder reads work through any pool mode (including transaction-mode
PgBouncer) because each operation is a single transaction or read-only query
with no session state held between transactions. The `LISTEN` side requires
a **session-compatible connection** (direct PostgreSQL, session-mode pooler,
or a connection-pinning pooler), because LISTEN registrations are
session-scoped and a transaction-mode pooler reassigns the underlying
connection between transactions, losing the registration. The clean
implementation is a normal connection pool for queries/transactions plus a
single dedicated long-lived connection held by the listener task — pooling
adds nothing for a process that does `LISTEN` once and then receives
forever.

**Polling as an alternative to LISTEN was considered and rejected.** Polling
the outbox on a fixed interval would eliminate the session-mode requirement
but introduces a latency floor (typically 100ms+ per poll cycle) that
degrades the dashboard's real-time feel. `LISTEN/NOTIFY` gives essentially
zero added latency from commit to fan-out, which matches the product
requirement of seeing GitHub Actions transitions in near-real time.

### 4. Outbox stores domain events only — no derived state

The outbox row stores the full domain event with no derived sidecar. Pool
stats are derived frontend-side from `runStore.jobs` per
[ADR 0004](./0004-frontend-derived-pool-stats.md); the WS payload carries
events only, not `poolStatsAfter`.

Earlier drafts of this ADR specified persisting `poolStatsAfter` in the
outbox row to preserve commit-time semantics across replay. That approach
was superseded by ADR 0004, which moved the derivation to the consumer and
removed the wire-side sidecar entirely. The outbox is free of
denormalization, and concurrent webhook writers do not need to coordinate
on derived state.

### 5. Replica topology: symmetric replicas, one serialized forwarder loop each

Each app replica:

- runs its own outbox forwarder loop
- holds its own `last_forwarded_seq` watermark
- reacts to `NOTIFY` wake-ups level-triggered (a wake-up arriving during a
  drain sets a "needs another pass" flag and returns; it does not start a
  second concurrent fetch)
- queries strictly with `seq > last_forwarded_seq ORDER BY seq`
- advances the watermark only after rows are accepted for local fan-out

Both `GET /v1/state` and `GET /v1/ws` continue to be served by every replica
without routing through a distinguished node. Leader election is rejected as
the primary answer because:

- it does not eliminate replay-overlap inside the leader
- failover still needs a durable handoff
- it reintroduces topological coupling between snapshot and stream paths
  that the current architecture intentionally avoids

Leader election may be reconsidered later as a hardening / cost-reduction
measure, not as the multi-replica enabling mechanism.

**Replica startup watermark:** On startup, each replica initializes its
`last_forwarded_seq` to `MAX(seq)` from the outbox at boot. Replicas do not
replay historical outbox rows to live WebSocket clients on startup — those
rows are already reflected in the current-state projection that any newly
connecting client receives via `GET /v1/state`. Browser reconnect /
catch-up runs through the snapshot path, not through the live WS forwarder.

## Consequences

### Positive

- Multi-replica deployments become semantically correct, not just renderable.
- Reconnect, restart, and new-replica catch-up share a single recovery path:
  read outbox by `seq > watermark`.
- Snapshot and stream stay reconciled across replicas because both derive from
  the same durable cursor.
- Future features (per-repo filtering, history views, audit, debugging) all
  benefit from a queryable durable representation rather than process memory.
- Concurrent webhook writers do not need write-skew protection for derived
  state; pool stats derivation is pulled to the consumer per ADR 0004,
  eliminating the `SERIALIZABLE`-vs-`READ COMMITTED` tradeoff that would
  otherwise have to be made at the write boundary.

### Negative / costs

- **Operators running multi-replica must provision PostgreSQL.** The chart's
  existing `databaseUrl` field becomes load-bearing for the first time;
  the in-memory mode survives only as a dev/test convenience.
- **Cross-replica fan-out multiplies DB read load.** N replicas all `LISTEN`
  on the same channel and all `SELECT` from the outbox after each `NOTIFY`.
  Acceptable at ATC's scale; the most likely future bottleneck if replica
  counts grow significantly.

### Out of scope

- Exact SQL schema for `runs`, `jobs`, and `outbox`
- Migration tooling choice (`sqlx-cli`, `refinery`, `sea-orm-migration`, or other)
- Connection pooling / Postgres client choice (`sqlx`, `tokio-postgres`,
  `deadpool-postgres`, etc.)
- Whether raw GitHub webhook JSON is also persisted for audit, separate from
  the canonical domain-event projection
- Database connection configuration shape (single `ATC_DATABASE_URL` vs.
  separate main and listener URLs). The two-path requirement above is fixed;
  how it is exposed in config is a Phase 2 implementation decision
- **Wire-contract changes** (cursor rename, ordering relaxation, frontend
  dedupe) — see ADR 0003
- **Operator-surface decisions** (Helm gating, SQLite mode removal,
  retention) — see ADR 0003

## Implementation Status

- **Decision 1** (PostgreSQL as durable state backend, sqlx crate, `sqlx::migrate!` for migrations): implemented in Phase 2a (PR #48). Schema: `runs` and `jobs` tables in `0001_initial_runs_jobs.sql`.
- **Decision 2** (atomic current-state UPSERT + outbox INSERT in one transaction): implemented in Phase 2c. `migrations/0002_outbox.sql` adds the outbox table; `upsert_*_in_txn` and `insert_outbox_*_in_txn` helpers in `persist.rs` drive the transaction. Webhook handler holds seq mutex across `pool.begin()…tx.commit()` to preserve broadcast order = durable order.
- **Decision 3** (NOTIFY emission after commit; listener connection using session-compatible path): IMPLEMENTED in Phase 2d — see feat/phase-2d-notify-listener branch. `SELECT pg_notify('atc_outbox', seq::text)` emitted inside the webhook transaction before `tx.commit()`. Dedicated `PgListener` listener task; `ATC_DATABASE_LISTENER_URL` config option for session-mode override. Five new metrics wired.
- **Decision 4** (original pool-stats persistence — superseded by ADR 0004): outbox payload stores `RunEventEnvelope`/`JobEventEnvelope` only (no `pool_stats_after`). ADR 0004 governs pool stats from Phase 3b onward.
- **Decision 5** (startup watermark and forwarder design): PARTIAL — listener structure, coalescing via `Arc<Notify>`, and watermark init (`COALESCE(MAX(seq), 0)`) implemented in Phase 2d. Drain task fetches `seq > watermark ORDER BY seq` and logs rows (stub). Only `forward_to_ws_clients` step deferred to Phase 3c, when the drain loop gains the actual WS forwarding call.
- **Operational metrics** (Out of scope item, now implemented): six metrics shipped on 2026-05-06 in `docs/design-plans/2026-05-06-phase-5-operational-metrics.md` — `atc_pg_outbox_lag_seconds`, `atc_pg_drain_pass_duration_seconds`, `atc_pg_wake_coalesced_total`, `atc_pg_drain_startup_seconds`, `atc_pg_broadcast_watermark`, `atc_pg_min_pending_seq`. Per-metric documentation lives in `docs/architecture/backend-server.md` § Operational metrics, governed by the Metric authoring contract subsection codified in the same file. The original ADR text said "replay duration" — the implemented metric is `atc_pg_drain_startup_seconds` and measures startup-init latency, not replay backlog (Phase 3c restart-recovery contract precludes a historical-replay backlog).

Phase 2c PR: `feat(server): add transactional outbox and reverse webhook error policy` (squash-merge commits `877b2c6`–`02ddd72`).

## Related

- ADR 0003 — [`last_seq` cursor and multi-replica operator policy](./0003-state-cursor-contract-and-operator-policy.md)
- ADR 0004 — [Frontend-derived pool stats](./0004-frontend-derived-pool-stats.md) (supersedes original Decision 4)
- Issue: [#7 — design: externalize live state to support multi-replica deployments](https://github.com/bojanrajkovic/atc/issues/7)
- Research: [`docs/architecture/state-externalization-research/`](../architecture/state-externalization-research/README.md)
  - [`backend-architecture.md`](../architecture/state-externalization-research/backend-architecture.md) — alternative comparison and recommended ADR positions
  - [`overlap-and-forwarding.md`](../architecture/state-externalization-research/overlap-and-forwarding.md) — wake-up coalescing, leader-election analysis
  - [`rollout-and-implementation.md`](../architecture/state-externalization-research/rollout-and-implementation.md) — phased rollout plan
- Current architecture: [`docs/architecture/backend-server.md`](../architecture/backend-server.md)
  (§ AppState, § SeqEvent Sidecar Contract, § Webhook Ingestion, § REST State Snapshot)
