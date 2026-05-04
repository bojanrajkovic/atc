# Rollout And Implementation

Last verified: 2026-05-03

## Status

This document is the canonical phasing for the state-externalization work. It is maintained in lockstep with [ADR 0002](../../architecture-decisions/0002-state-externalization-postgres-outbox.md), [ADR 0003](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md), and [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md). The ADRs decide *what* and *why*; this document decides *when* and *in what order*.

Each sub-phase is intended to be roughly one PR's worth of work — small enough that an implementor (human or AI agent) can hold the full scope in context, large enough not to be busywork. Each sub-phase should get its own detailed task-by-task implementation plan in `docs/implementation-plans/` before execution.

## Phased Rollout

The recommended design does not need to land in one PR, but it should be phased so that snapshot and stream semantics stay coherent at every cutover point.

```mermaid
flowchart LR
  P1[Phase 1 ADRs] --> P2A[2a Foundation]
  P2A --> P2B[2b Shadow Writes]
  P2B --> P2C[2c Outbox]
  P2C --> P2D[2d NOTIFY]
  P2D --> P3A[3a Cursor Rename]
  P3A --> P3B[3b Pool Stats Move]
  P3B --> P3C[3c Read Cutover]
  P3C --> P4[Phase 4 Multi-Replica]
  P4 --> P5[Phase 5 Hardening]
```

### Phase 1: ADR and contract decisions

Settle the decisions that shape every later phase.

**Status: complete.** Decisions captured in [ADR 0002](../../architecture-decisions/0002-state-externalization-postgres-outbox.md), [ADR 0003](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md), and [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md).

### Phase 2: Durable write path in shadow mode

Add the durable backend structures without changing frontend-visible behavior. The app continues to serve snapshots and WS from the in-memory path while the durable path is built and validated in parallel. After Phase 2, every webhook produces both an in-memory mutation AND a durable PG write, and a NOTIFY fires post-commit — but no replica is reading from the outbox or PG yet.

#### Phase 2a: PG foundation

**Status: complete (PR #48).**

Bootstrap the database integration without touching any read paths or the webhook handler logic.

In scope:
- Pick a PG client crate (`sqlx`, `tokio-postgres`, `sea-orm`, etc.) — ADR 0002 leaves this open as a Phase 2 implementation choice
- Pick a migration tool (`sqlx-cli`, `refinery`, `sea-orm-migration`, etc.) — same Phase 2 choice
- Initial SQL schema for `runs` and `jobs` tables only (no outbox yet); FK from `jobs.run_id` to `runs.id`; whatever indexes are needed for the Phase 3c snapshot queries
- Connection pool wiring against `ATC_DATABASE_URL` (already exists in chart)
- Add a DB connectivity check to `/readyz` so the readiness probe catches misconfiguration
- Add testcontainers-based integration test infrastructure
- The in-memory `StateStore` is unchanged; nothing else is wired to PG yet

Acceptance: `just test` runs an integration test that boots an ephemeral Postgres, runs migrations, and confirms the readiness probe passes.

ADR refs: [ADR 0002 Decision 1](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (storage backend), [ADR 0002 Out of scope](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (config shape).

#### Phase 2b: Shadow current-state writes

Implement the atomic-update pattern for `runs` and `jobs` and start writing to PG alongside the in-memory store.

In scope:
- Implement atomic `UPDATE ... WHERE status IN (predecessors)` for runs and jobs; predecessor sets parameterized from the existing Rust state machine (e.g., `RunStatus::predecessors_of(target)` and equivalent for `JobStatus`)
- Implement first-sight creation via `INSERT ... ON CONFLICT (id) DO UPDATE ... WHERE status IN (predecessors)`
- Map `0 rows affected` to `StoreError::InvalidTransition` (the existing error type)
- Webhook handler now writes to PG in addition to the in-memory store (shadow mode — both writes happen, in-memory still authoritative for reads)
- Integration tests verifying PG state matches in-memory state after a sequence of webhook events
- Tests for invalid transitions (rejected via `0 rows affected`)
- Tests for idempotent same-status replay

Acceptance: a test that fires a sequence of run/job webhooks against a real Postgres and asserts PG row state matches the in-memory store at each step.

ADR refs: [ADR 0002 Decision 2](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (atomicity + concurrency control via UPDATE-WHERE-predicate).

#### Phase 2c: Outbox table + transactional writes

Add the outbox and make the current-state write + outbox append atomic.

In scope:
- Add `outbox` table with `BIGSERIAL seq` primary key, plus columns for the domain event payload (per [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md): domain event only, no derived sidecar)
- Schema for the event payload: probably JSONB for the domain event body; minimal projected columns (`run_id`, `job_id`, event kind, `created_at`) for indexing/debugging
- Webhook handler wraps current-state UPSERT + outbox INSERT in one transaction
- Tests: writes succeed/fail together; outbox rows align with current-state rows
- Tests: aborted transactions consume seq values without committing rows (validates monotonic-not-gapless behavior)
- The outbox is written but not yet read by anyone

Acceptance: a test that triggers a transaction abort (e.g., via constraint violation) and confirms the seq advances without producing a committed outbox row, and a separate test that confirms successful writes produce both a current-state row and a matching outbox row in a single transaction.

ADR refs: [ADR 0002 Decision 2](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (atomicity), [ADR 0003 Decision 2](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (monotonic-not-gapless ordering), [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md) (outbox stores domain events only).

#### Phase 2d: NOTIFY emission + listener stub

Wire up `LISTEN/NOTIFY` end-to-end without yet using it for forwarding.

In scope:
- Webhook handler emits `NOTIFY` after commit; payload is just the seq (or a small token) per ADR 0002 Decision 3
- Add a separate dedicated long-lived listener connection on a session-mode-compatible path
- Add `ATC_DATABASE_LISTENER_URL` config option that defaults to `ATC_DATABASE_URL` if unset
- Listener task in `atc-server` runs the level-triggered drain loop (per `overlap-and-forwarding.md` pseudocode) — but instead of forwarding to WS clients, it just logs received notifications and the rows it would have fetched
- Tests verify NOTIFY fires after commit and the listener task receives it
- Tests verify wake-up coalescing (multiple notifications during a drain produce one extra pass, not concurrent fetches)

Acceptance: an integration test that fires N webhooks and confirms the listener observes N notifications and would have fetched all N outbox rows in seq order.

ADR refs: [ADR 0002 Decision 3](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (NOTIFY + connection-pool compatibility), [ADR 0002 Decision 5](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (forwarder design including startup watermark).

### Phase 3: Single-replica read-path cutover

Switch every read path to the durable backend. The cursor rename and pool stats removal both ship in Phase 3 because they're wire-contract changes that need to land before the read path can use the new shape; they cannot be deferred to Phase 4 without risking inconsistency.

#### Phase 3a: Cursor rename

Rename the snapshot cursor and ship the lockstep frontend change.

In scope:
- Rename `StateSnapshot.seq` → `StateSnapshot.lastSeq`; semantics shift from "next seq to assign" to "highest committed seq"
- ts-rs regenerates the `StateSnapshot` TypeScript type
- Frontend `connection.ts:14` rename `snapshotSeq` → `snapshotLastSeq` (or similar)
- Frontend `connection.ts:116` invert the comparator from `>=` to `>`
- Update test fixtures and assertions referencing the old field name and comparator
- Backend and frontend ship together in one binary version (no transition window per [ADR 0003 Context](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md))

Acceptance: existing connection-buffering tests pass with the new field name and comparator; an end-to-end test confirms a buffered event with `seq == lastSeq` is correctly discarded (vs. previously replayed).

ADR refs: [ADR 0003 Decision 1](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (cursor rename), [ADR 0003 Context](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (single-binary deployment shape).

#### Phase 3b: Pool stats moved to frontend

Remove backend pool stats computation; add frontend derivation.

In scope:
- Backend: delete `StateStore::pool_stats()` and the snapshot-time inline pool-stats computation
- Backend: remove `pool_stats` from `StateSnapshot` and `pool_stats_after` from `SeqEvent`
- Backend: webhook handler no longer computes `pool_stats_after` under the seq mutex
- Frontend: add a `pools` `$derived.by` to `runStore` (or thin wrapper in `runnerStore`) that replicates the existing backend algorithm — skip Waiting/Completed, group by sorted-label set, count Queued/InProgress, derive `group_name` and `is_elastic` from observed runners, sort lexicographically by labels
- Frontend: remove `runnerStore.loadPools()` call sites and the dispatcher's `if (seqEvent.poolStatsAfter != null)` block
- ts-rs regenerates the affected types
- Test churn: backend tests asserting on `pool_stats_after` go away; frontend tests fed `poolStatsAfter` through fixtures now drive derivation through the underlying jobs
- Backend and frontend ship together in one binary version

Acceptance: existing pool-display E2E tests pass with the derived store; a test confirms duplicate Job events produce idempotent recomputes that yield the same pool stats.

ADR refs: [ADR 0004](../../architecture-decisions/0004-frontend-derived-pool-stats.md) (entire ADR).

#### Phase 3c: Read-path cutover

Switch `GET /v1/state` to read from PG and `/v1/ws` to forward from the outbox. Remove the in-memory store.

In scope:
- `GET /v1/state` reads the unevicted current-state projection from PG (`runs` and `jobs` tables); `lastSeq` comes from `MAX(seq)` over the outbox in the same transaction
- WS forwarder: the listener task from Phase 2d gains the actual forwarding path — drain rows by `seq > last_forwarded_seq ORDER BY seq`, push as `SeqEvent`s to local WS clients, advance watermark only after acceptance
- On startup, initialize `last_forwarded_seq` to `MAX(seq)` from outbox at boot (no historical replay to live WS clients)
- Remove the in-memory `StateStore`, the broadcast channel, and the seq mutex from `AppState`
- Update `main.rs` lifecycle wiring (no more `start_eviction_task` for in-memory; eviction becomes a SQL `DELETE` task — implementation choice between application-side periodic SQL DELETE or `pg_cron` / scheduled query)
- Update integration tests to verify end-to-end via PG (snapshot returns PG state, WS receives outbox-forwarded events in seq order)
- Single-replica deployment is now fully on durable storage

Acceptance: an end-to-end test that fires webhooks, confirms `GET /v1/state` returns the expected projection from PG, and confirms `/v1/ws` delivers SeqEvents in seq order via the LISTEN/drain pipeline.

ADR refs: [ADR 0002 Decision 5](../../architecture-decisions/0002-state-externalization-postgres-outbox.md) (forwarder design + startup watermark), [ADR 0003 Decision 4](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (TTL eviction as SQL DELETE).

### Phase 4: Multi-replica enablement

Update Helm chart and operationally validate multi-replica. This is the phase where issue #7 is closed in substance.

In scope:
- Update Helm chart: gate `replicaCount > 1` on the presence of a `postgres://` URL via `config.databaseUrl` or `existingSecret`
- Remove SQLite mode from chart entirely (per ADR 0003 Decision 3): the chart's storage-mode story collapses from three modes (ephemeral / local-SQLite / external-Postgres) to two (ephemeral / external-Postgres)
- Update `deploy/helm/atc/values.yaml:106-132` storage-mode docs
- Remove SQLite values-matrix tests under `deploy/helm/atc/tests/`
- Update existing `{{ fail }}` guard logic for the new mode constraints
- Operationally test: deploy with `replicaCount = 2` against a shared Postgres; verify both replicas serve snapshots, both forward events, and clients on either replica see consistent state
- Update `docs/architecture/deployment.md` to reflect the two-mode story

Acceptance: a multi-replica test deployment shows two pods both serving `/v1/state` and `/v1/ws` against the same PG, with consistent state across both.

ADR refs: [ADR 0003 Decision 3](../../architecture-decisions/0003-state-cursor-contract-and-operator-policy.md) (Helm gating + SQLite removal).

### Phase 5: Hardening and cleanup

After multi-replica correctness is proven. These items are independent and can land as separate PRs.

In scope:
- Add metrics for outbox lag, forwarding watermark, wake-up coalescing, and replay duration (per ADR 0002 Out of scope: operational metrics)
- Decide whether the production in-memory path remains as a dev-only mode or is removed entirely (per ADR 0003 Out of scope)
- Decide outbox retention duration and eviction strategy; implement the chosen approach (per ADR 0003 Decision 4: retention is decided separately from current-state TTL)
- Optionally: persist raw GitHub webhook JSON alongside domain events for audit/debug (per ADR 0002 Out of scope)

ADR refs: various ADR Out of scope sections.

## Implementation Checklist

A cross-reference for the ADRs. The canonical decisions are in the ADRs themselves; this checklist is a quick map from "what must / should / should not happen" to "which sub-phase and which ADR."

### Must

These are the pieces that need to land for the recommended design to be coherent.

1. Transactional current-state update plus outbox append (one DB transaction) — Phase 2c, ADR 0002 Decision 2
2. Durable monotonic cursor on the outbox (`BIGSERIAL` seq) — Phase 2c, ADR 0003 Decision 2
3. Snapshot contract uses `lastSeq` semantics, not "next seq to assign" — Phase 3a, ADR 0003 Decision 1
4. Replica forwarders fetch `seq > watermark`, never `>=` — Phase 3c, ADR 0002 Decision 5
5. Replica forwarders replay `ORDER BY seq` — Phase 3c, ADR 0002 Decision 5
6. One serialized outbox-drain loop per replica — Phase 2d / Phase 3c, ADR 0002 Decision 5
7. Atomic `UPDATE ... WHERE status IN (predecessors)` for state machine transitions — Phase 2b, ADR 0002 Decision 2
8. Pool stats derived frontend-side; outbox stores domain events only — Phase 3b, ADR 0004

### Should

Defaults the implementation should prefer unless there is a specific reason to deviate.

1. `NOTIFY` only as wake-up signal; payload is just the seq — Phase 2d, ADR 0002 Decision 3
2. Coalesce wake-ups (level-triggered drain loop) — Phase 2d / Phase 3c, ADR 0002 Decision 5
3. Listener uses a dedicated long-lived session-mode-compatible connection — Phase 2d, ADR 0002 Decision 3
4. Symmetric replicas (no leader; both serve `GET /v1/state` and `/v1/ws`) — Phase 4, ADR 0002 Decision 5
5. Initialize `last_forwarded_seq` to `MAX(seq)` at replica startup — Phase 3c, ADR 0002 Decision 5

### Optional

Worthwhile improvements that are not first-order requirements.

1. Persist raw webhook JSON alongside domain events for audit/debug — Phase 5, ADR 0002 Out of scope
2. Add operational metrics (outbox lag, forwarding watermark, wake-up coalescing, replay duration) — Phase 5
3. Server-side leader election for a single active forwarder topology — explicitly rejected as the primary mechanism in ADR 0002 Decision 5; only attractive if the system wants a distinguished forwarder for broader reasons

### Not Recommended

These are designs that should NOT substitute for the must-have core.

1. Leader election as the primary multi-replica mechanism — does not eliminate the need for transactional outbox semantics, durable watermarks, or ordered replay (ADR 0002 Decision 5)
2. Frontend `highestAppliedSeq` dedupe — explicitly decided against; the forwarder design prevents overlap by construction (ADR 0003)
3. Reimplementing transition rules in PL/pgSQL or check constraints — the Rust state machine remains the single source of truth; SQL just consumes predecessor sets as parameters (ADR 0002 Decision 2)
4. Client-visible gap detection assuming contiguous cursors — incompatible with `BIGSERIAL` semantics under aborted transactions (ADR 0003 Decision 2)
