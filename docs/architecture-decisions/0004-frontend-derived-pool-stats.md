# 0004 — Frontend-derived pool stats

**Status:** Accepted (Phase 1 of state-externalization rollout, 2026-05-03)

## Context

Today the backend computes runner pool statistics (`RunnerPoolStats`) from the
in-memory `RunStateMachine` and ships them in two places:

- `StateSnapshot.poolStats` on `GET /v1/state`
- `SeqEvent.poolStatsAfter` on every Job event over `/v1/ws`

The frontend wholesale-replaces `runnerStore.pools` from each value
(`runners.svelte.ts:6-8` and `dispatcher.ts:104-106`).

[ADR 0002](./0002-state-externalization-postgres-outbox.md) commits ATC to a
multi-writer architecture: any replica behind the load balancer can ingest a
webhook. Pool stats computed inside the webhook write transaction are
derived over the entire `jobs` collection — a predicate read that under
naive `READ COMMITTED` would let two concurrent webhooks each produce a
`poolStatsAfter` that misses the other's effects, regressing the dashboard
until the next event corrects it. Avoiding that regression on the backend
would require `SERIALIZABLE` isolation with retry-on-conflict, or a
leader-routed compute, or a per-pool projection table — all backend
complexity to preserve a derivation that does not actually need to live
there.

## Decision

Move `RunnerPoolStats` derivation from the backend to the frontend.

Concretely:

- **Wire contract:** Remove `poolStats` from `StateSnapshot`. Remove
  `poolStatsAfter` from `SeqEvent`. Both REST snapshot and WS event
  payloads carry only the underlying entity data (runs, jobs, events).
  Backend and frontend ship together in a single binary, so the removals
  happen in lockstep — no dual-shape transition window is required (see
  ADR 0003 Context for the single-binary deployment note).
- **Backend:** `RunStateMachine::pool_stats()` and the snapshot-time inline
  pool stats computation are deleted as part of this change — no dead
  production code remains. If a backend self-test for derivation parity
  is wanted later, it can be reintroduced as a test helper.
- **Frontend:** Add a `pools` `$derived.by` to `runStore` (or a thin
  `runnerStore` rune that reads `runStore.jobs`). The derivation
  replicates the existing backend algorithm: iterate jobs, skip
  `Waiting`/`Completed`, group by sorted-label set, count `Queued` and
  `InProgress`, pull `group_name` from observed runners, set
  `is_elastic` if any runner has `group_id == 0`, and sort the resulting
  array lexicographically by labels.

The frontend's derivation runs against state that is always self-consistent
(single writer = the dispatcher), so no concurrent-writer reasoning is
required.

## Consequences

### Positive

- **Eliminates the concurrent-writer consistency problem.** Multi-replica
  webhook ingestion no longer needs `SERIALIZABLE` isolation,
  retry-on-conflict handling, or a pool-stats projection table for
  write-side correctness.
- **Cleaner outbox semantics.** The outbox stores domain events with no
  derived sidecar — no denormalization, no replay-time-vs-commit-time
  semantic question, no risk of stale sidecars regressing pool UI.
- **Smaller wire payloads.** Job events drop the `poolStatsAfter` array;
  the snapshot drops `poolStats`.
- **Better separation of concerns.** Backend stores facts; frontend
  computes views. The wire contract carries the minimum needed for
  derivation.
- **Single derivation site.** Today the backend computes pool stats with
  one algorithm and the frontend trusts the result; both sides
  effectively encode the algorithm (frontend in test fixtures and
  assertions). After this ADR, only one site computes — no risk of
  drift between backend and frontend implementations of the same logic.

### Negative / costs

- **Operator-configured capacity (`total` field) needs a separate
  delivery path** when it is implemented. Currently `total` is always
  `None`. When a future feature lets operators configure pool capacity,
  that configuration is per-pool config (not derived state) — it would
  flow via a config endpoint or separate event channel, not by
  re-introducing a computed sidecar.

### Out of scope

- The `total` (capacity) field's future delivery path
- Whether `runnerStore` becomes a `$derived` over `runStore.jobs` or
  is reorganized into a derivation in `runStore` directly
- Frontend test restructuring details (fixture shape, assertion changes)

## Implementation Status

**Status: complete (Phase 3b, feat/phase-3a-3b-wire-contract).**

- **Outbox stores domain events only (no `pool_stats_after`)**: enforced in Phase 2c. Outbox `payload` JSONB stores `RunEventEnvelope` / `JobEventEnvelope` — the parsed-webhook domain events. `SeqEvent.pool_stats_after` is never written to the outbox. Verified by `phase_2c_outbox_ac6_1_payload_is_envelope_not_seq_event` test.
- **Frontend derivation of pool stats**: complete in Phase 3b. The pure function `computePoolStats(jobs: Job[]): RunnerPoolStats[]` is exported from `frontend/src/lib/stores/runners.svelte.ts` and replicates the original backend algorithm: dedupe labels via `Set` then sort, use `JSON.stringify(sortedLabels)` as the map key (collision-free given the sorted normalization), skip `Waiting`/`Completed` jobs, count `Queued` and `InProgress` per label set, derive `groupName` from the most recent observed `runner.groupName`, set `isElastic = true` when any observed `runner.groupId === 0n` (bigint-aware), and return the resulting array sorted lexicographically by labels. `RunnerStore` exposes `readonly pools = $derived.by(() => computePoolStats(runStore.jobs))` — no `$state`, no `loadPools`, no `clear`. The flat `runStore.jobs` `$derived.by<Job[]>` view (added in `runs.svelte.ts`) is the single dependency.
- **Backend deletions**: `RunStateMachine::pool_stats()` removed; the snapshot-time inline pool-stats computation in `RunStateMachine::snapshot()` removed (snapshot now returns `QueryResult { runs, jobs }` only); `StateSnapshot.pool_stats` field removed; `SeqEvent.pool_stats_after` field removed; `tests/sidecar_tests.rs` (~530 lines) deleted; `store/tests/runner_pools.rs` (512 lines) deleted; the dispatcher's `if (seqEvent.poolStatsAfter != null)` block removed; `connection.ts` no longer calls `runnerStore.loadPools(snapshot.poolStats)` on snapshot load; `e2e/lib/ws-mock.ts` `makeJobSeqEvent` no longer emits `poolStatsAfter`.

## Related

- ADR 0002 — [PostgreSQL outbox + symmetric replicas for live state](./0002-state-externalization-postgres-outbox.md)
  (originally specified `poolStatsAfter` persistence in outbox; superseded
  here)
- ADR 0003 — [`last_seq` cursor and multi-replica operator policy](./0003-state-cursor-contract-and-operator-policy.md)
- Issue: [#7 — design: externalize live state to support multi-replica deployments](https://github.com/bojanrajkovic/atc/issues/7)
- Backend derivation pre-Phase-3b: `RunStateMachine::pool_stats()` and the
  snapshot-time inline computation lived in `backend/crates/atc-core/src/state_machine.rs`
  (renamed from `store.rs` in #50). Both were deleted in Phase 3b alongside the
  `StateSnapshot.pool_stats` and `SeqEvent.pool_stats_after` wire fields.
- Frontend derivation today: `computePoolStats` in `frontend/src/lib/stores/runners.svelte.ts`,
  invoked from a `$derived.by(...)` computed over `runStore.jobs`. The previous
  imperative `loadPools(snapshot.poolStats)` and per-event `if (seqEvent.poolStatsAfter)`
  dispatch sites are gone.

## Footnote — operator-declared capacity (issue #16)

Issue #16 (closed by [`docs/design-plans/2026-05-13-issue-16-runner-pool-capacity.md`](../design-plans/2026-05-13-issue-16-runner-pool-capacity.md)) introduces operator-declared pool capacity. This does **not** undo this ADR: capacity is operator config, not derived state.

The delivery path:

1. Loaded server-side from `/etc/atc/config.yaml` via figment.
2. Held in `AppState::runner_pool_capacities` (single source of truth) — the `PersistentStore` trait stays untouched.
3. Composed onto `StateSnapshot.runner_pool_capacities` at the route layer in `routes::state_handler` — not in the store.
4. Merged frontend-side by `computePoolStats(jobs, capacities)` into `RunnerPoolStats.total`, keyed by canonical label set.

No backend `atc_runner_pool_*` Prometheus gauge is reintroduced — that path would force re-derivation server-side and invalidate this ADR. The frontend remains the single derivation site for all pool stats; capacity arrives as inert config alongside the entity data, not as a computed sidecar.
