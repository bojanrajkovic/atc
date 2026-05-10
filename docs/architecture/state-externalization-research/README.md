# State Externalization Research

Last verified: 2026-05-07 (state-externalization rollout closed: all five phases shipped or issue-tracked; see "Status" section for the disposition map)

## Purpose

This document set records design research for [issue #7](https://github.com/bojanrajkovic/atc/issues/7), `design: externalize live state to support multi-replica deployments`.

It is pre-ADR research that informed the canonical decisions now in the ADRs (see Status below).

## Status

The canonical decisions are now in:

- [ADR 0002](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) — PostgreSQL outbox + symmetric replicas for live state
- [ADR 0003](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) — `last_seq` cursor and multi-replica operator policy
- [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md) — Frontend-derived pool stats

The ADRs are canonical. This document set is preserved for analysis and rejected alternatives.

**Rollout status: complete as of 2026-05-07.** All five implementation phases shipped: Phase 2a (PR #48 — sqlx pool + migrations), Phase 2b/2c (transactional outbox), Phase 2d (LISTEN/NOTIFY listener), Phase 3a/3b (wire contract alignment, PR #54), Phase 3c (PG-backed read path), Phase 4 (multi-replica enablement, PR #57 closing #7), Phase 5 (operational metrics, PR #63). Out-of-scope follow-ups deferred during the rollout are now tracked as discrete issues:

- ~~[#50](https://github.com/bojanrajkovic/atc/issues/50) — Reconcile `PersistentStore` trait with transactional outbox (post-2c code cleanup)~~ — **CLOSED** by [ADR 0005](../../architecture-decisions/0005-persistentstore-trait-relocation.md): trait relocated to `atc-server::persist` with `PgStore` + `InMemoryStore` impls; `AppState` carries `Arc<dyn PersistentStore>` for the write path.
- [#64](https://github.com/bojanrajkovic/atc/issues/64) — Bundle the Grafana dashboard as a Helm ConfigMap
- [#65](https://github.com/bojanrajkovic/atc/issues/65) — Persist raw GitHub webhook JSON alongside domain-event projection
- [#66](https://github.com/bojanrajkovic/atc/issues/66) — Backfill seven-element interpretation blocks for legacy `atc_pg_*` counters
- [#67](https://github.com/bojanrajkovic/atc/issues/67) — Design outbox retention / eviction strategy

Notable decisions that diverge from the research recommendations:

- **Pool stats are derived frontend-side, not persisted as a sidecar** (ADR 0004) — supersedes recommendations here to persist `poolStatsAfter` in the outbox
- **Frontend live-stream dedupe is not added** — supersedes recommendations here to add `highestAppliedSeq` as defense in depth (the forwarder design prevents overlap by construction)
- **Concurrency control uses atomic `UPDATE ... WHERE status IN (predecessors)`** parameterized from the Rust state machine, not stored procedures or check constraints (ADR 0002 Decision 2)
- **No backward-compatible cursor transition window** — frontend and backend ship together in one binary via `rust-embed`, so the cursor rename happens in lockstep (ADR 0003 Context)
- **Helm gating has no `unsafe` escape hatch** — multi-replica simply requires a `postgres://` URL; SQLite mode is removed entirely (ADR 0003 Decision 3)

## Inputs

Primary discussion inputs:

- [Issue #7 body](https://github.com/bojanrajkovic/atc/issues/7)
- [Comment: Phase 9 design considerations](https://github.com/bojanrajkovic/atc/issues/7#issuecomment-4230507060)
- [Comment: monotonic ordering cursor / outbox shape](https://github.com/bojanrajkovic/atc/issues/7#issuecomment-4230531833)
- [Comment: seq/store atomicity constraint](https://github.com/bojanrajkovic/atc/issues/7#issuecomment-4230538568)

Repository inputs:

- `docs/architecture/backend-server.md`
- `docs/architecture/deployment.md`
- `docs/design-plans/2026-04-08-helm-chart.md`
- `docs/design-plans/2026-04-11-server-wiring.md`
- `backend/crates/atc-server/src/routes.rs`
- `backend/crates/atc-server/src/state.rs`
- `backend/crates/atc-core/src/store.rs`
- `frontend/src/lib/connection.ts`

External reference inputs:

- [PostgreSQL `NOTIFY`](https://www.postgresql.org/docs/current/sql-notify.html)
- [PostgreSQL transaction isolation notes on `serial` / sequences](https://www.postgresql.org/docs/current/transaction-iso.html)
- [Redis Pub/Sub delivery semantics](https://redis.io/docs/latest/develop/pubsub/)
- [Redis Streams](https://redis.io/docs/latest/develop/data-types/streams/)
- [NATS JetStream consumers](https://docs.nats.io/nats-concepts/jetstream/consumers)

## Current Repo Observations

Two points matter before evaluating alternatives:

1. The implemented server contract is now stricter than the older Phase 9 design plan. `atc-server` no longer uses an `AtomicU64`; it uses `Mutex<u64>` held across store mutation plus seq assignment, and `GET /v1/state` holds the same mutex across snapshot plus seq read. The code is already built around atomic snapshot/stream handoff.
2. The issue body's statement that Helm blocks all `replicaCount > 1` deployments was already loose as of 2026-05-03 (the guard only rejected `persistence.enabled=true` with `replicaCount > 1`; stateless multi-replica still rendered without any precondition). Phase 4 (2026-05-06) replaced that guard with a template-render-time `{{ fail }}` that ties `replicaCount > 1` to the presence of a Postgres URL via `config.databaseUrl` or `existingSecret`. The persistence machinery itself was retired alongside SQLite (see ADR 0003 Phase 4 implementation note and `deployment.md` § "Storage-mode evolution"). Symmetric stateless-multi-replica is no longer renderable.

## Document Map

- [backend-architecture.md](./backend-architecture.md) — invariants, storage/event-format answers, alternative comparison, recommended backend shape, ADR positions
- [frontend-impact.md](./frontend-impact.md) — current frontend cursor assumptions, impact of contract changes, `poolStatsAfter` sensitivity, optional client hardening
- [overlap-and-forwarding.md](./overlap-and-forwarding.md) — overlap delivery failure modes, replica drain semantics, wake-up coalescing, leader-election tradeoffs
- [rollout-and-implementation.md](./rollout-and-implementation.md) — phased rollout and `must / should / optional` implementation checklist
- [additional-backends.md](./additional-backends.md) — forward-looking research on alternative state backends (CockroachDB, NATS JetStream, DynamoDB, FoundationDB, EventStoreDB) and composed multi-store shapes; not a roadmap, a reference for the day a Postgres switch is contemplated

## Recommended Direction

Recommendation: adopt PostgreSQL current-state tables plus a transactional outbox, with `LISTEN/NOTIFY` used only as a wake-up path.

Recommended shape:

1. Keep current-state tables keyed by GitHub IDs.
2. Add an append-only outbox carrying domain events (per ADR 0004; not the WS-ready payload).
3. Write current-state mutation and outbox append in one transaction.
4. Use a durable monotonic cursor for snapshot/stream reconciliation.
5. Have each replica run one serialized outbox forwarder loop.
6. Keep replicas symmetric for both `GET /v1/state` and `/v1/ws`.

## End-State Dataflow

```mermaid
flowchart LR
  GH[GitHub Webhooks] --> WH[Webhook Handler]
  WH --> TXN[DB Transaction]
  TXN --> CUR[(Current-State Tables)]
  TXN --> OUT[(Outbox Table)]
  TXN --> NTFY[NOTIFY Wake-up]

  NTFY --> FA[Replica A Forwarder]
  NTFY --> FB[Replica B Forwarder]

  OUT --> FA
  OUT --> FB

  CUR --> SA[Replica A GET /v1/state]
  CUR --> SB[Replica B GET /v1/state]

  FA --> WSA[Replica A /v1/ws]
  FB --> WSB[Replica B /v1/ws]

  SA --> CA[Browser Clients]
  SB --> CA
  WSA --> CA
  WSB --> CA
```

## Summary

The recommended design is not "just add Postgres" and it is not "leader election solves it." The coherent plan is:

- transactional current-state mutation plus outbox append (per ADR 0002)
- durable monotonic cursor renamed to `last_seq` (per ADR 0003)
- serialized per-replica outbox forwarders (per ADR 0002)
- pool stats derived frontend-side; outbox stores domain events only (per ADR 0004)

Frontend live-stream dedupe was considered but explicitly decided against — the forwarder design prevents overlap by construction.
