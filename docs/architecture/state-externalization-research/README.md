# State Externalization Research

Last verified: 2026-05-03

## Purpose

This document set records design research for [issue #7](https://github.com/bojanrajkovic/atc/issues/7), `design: externalize live state to support multi-replica deployments`.

It is pre-ADR research that informed the canonical decisions now in the ADRs (see Status below).

## Status

The canonical decisions are now in:

- [ADR 0002](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) — PostgreSQL outbox + symmetric replicas for live state
- [ADR 0003](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) — `last_seq` cursor and multi-replica operator policy
- [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md) — Frontend-derived pool stats

The ADRs are canonical. This document set is preserved for analysis and rejected alternatives. Notable decisions that diverge from the research recommendations:

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
2. The issue body's statement that Helm blocks all `replicaCount > 1` deployments no longer matches the repository as of 2026-05-03. The current Helm `fail` guard only rejects `persistence.enabled=true` with `replicaCount > 1`. Stateless multi-replica still renders, but it is not semantically correct because live state remains process-local.

## Document Map

- [backend-architecture.md](./backend-architecture.md) — invariants, storage/event-format answers, alternative comparison, recommended backend shape, ADR positions
- [frontend-impact.md](./frontend-impact.md) — current frontend cursor assumptions, impact of contract changes, `poolStatsAfter` sensitivity, optional client hardening
- [overlap-and-forwarding.md](./overlap-and-forwarding.md) — overlap delivery failure modes, replica drain semantics, wake-up coalescing, leader-election tradeoffs
- [rollout-and-implementation.md](./rollout-and-implementation.md) — phased rollout and `must / should / optional` implementation checklist

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
