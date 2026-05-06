# 0003 — `last_seq` cursor and multi-replica operator policy

**Status:** Accepted (Phase 1 of state-externalization rollout, 2026-05-03)

## Context

[ADR 0002](./0002-state-externalization-postgres-outbox.md) commits ATC to a
PostgreSQL-backed transactional outbox for live state. That decision forces a
set of related contract and operator-surface choices that are storage-influenced
but not strictly storage-specific:

- the snapshot cursor's name and semantics
- the ordering guarantee the WebSocket contract makes to clients
- whether the frontend hardens against duplicate live delivery
- which `replicaCount > 1` chart shapes are valid
- what survives of the chart's existing "three storage modes" framing
- how outbox retention relates to current-state retention

These decisions are bundled into a separate ADR so the wire contract and
operator surface can be reviewed (and potentially amended) on a different
timeline than the underlying storage choice.

This ADR depends on ADR 0002 and is meant to be read alongside it.

**Single-binary deployment shape.** The frontend is bundled into the backend
binary via `rust-embed` and shipped as one artifact. There is never a
deployed configuration where a frontend at version V1 runs against a backend
at version V2 — the binary uniquely determines both. The only residual
cross-version scenario is a long-lived browser tab whose JS predates the
most recent binary restart. The answer is to refresh the tab after a deploy
that includes a breaking wire-contract change; both sides change in lockstep
with the binary version, so no dual-shape transition window is required.
A future enhancement could automate this by sending a build version through
the WebSocket and triggering a refresh on mismatch — tracked separately,
not in scope for this ADR.

## Decision

### 1. Cursor contract: rename to `last_seq`, semantics shift to "highest committed seq"

Today `StateSnapshot.seq` documents itself as "next seq to assign". After
this ADR, the snapshot returns `last_seq` whose value is the highest seq
already committed and already broadcastable.

Client handshake rule changes from:

- discard buffered events with `seq < snapshot.seq`
- replay buffered events with `seq >= snapshot.seq`

to:

- discard buffered events with `seq <= snapshot.lastSeq`
- replay buffered events with `seq > snapshot.lastSeq`

The rename happens in lockstep with the backend change inside a single
binary version — no dual-field transition window is required (see the
single-binary deployment note in Context). The frontend change is small
but precise: exactly one comparator site in
`frontend/src/lib/connection.ts:116`. Rename the local cursor (currently
`snapshotSeq` at `:14`) and invert the comparator from `>=` to `>`.

**Client bootstrap is unchanged:** a connecting browser opens the WebSocket,
buffers incoming events, fetches `GET /v1/state` for the unevicted
current-state projection plus `last_seq`, then applies live WS events
where `seq > lastSeq` (and discards buffered events where `seq <= lastSeq`).
The PG migration preserves this protocol — the snapshot becomes a SQL
read of the current-state tables transactionally paired with the outbox's
`MAX(seq)` rather than an in-memory read paired with the seq mutex.

### 2. Ordering contract: strictly monotonic, not gapless

The current in-memory implementation produces gapless seq values because the
mutex serializes increments. The durable implementation will use a
`BIGSERIAL` / identity column whose values are strictly monotonic but not
gapless: aborted transactions consume a sequence value without committing
the row.

The frontend already tolerates this — `connection.ts` does no `seq + 1`,
range, or contiguity checks; it only uses an ordered cutoff at handoff and
trusts in-order delivery on the live WS connection. The contract is relaxed
explicitly so that future SQL changes (e.g., `INSERT ... ON CONFLICT DO
NOTHING` with sequence consumption) do not regress correctness.

A client-side gap detector that assumes contiguous numbers is explicitly
out of contract.

### 3. Helm contract: `replicaCount > 1` requires PostgreSQL; SQLite mode is removed

The chart gates `replicaCount > 1` on the presence of a `postgres://` URL via
`config.databaseUrl` or `existingSecret.databaseUrlKey`. The current
single-pod ephemeral mode remains `replicaCount = 1` only.

The chart's existing **local-SQLite mode is removed** as part of the Phase 4
chart update. The chart's storage-mode story collapses from three modes
(ephemeral / local-SQLite / external-Postgres) to two (ephemeral /
external-Postgres).

Rationale for removing SQLite rather than supporting it as a poll-based
single-replica durable mode:

- SQLite has no `LISTEN/NOTIFY` equivalent; preserving it would require two
  SQL flavors with different forwarder implementations (Postgres push,
  SQLite poll)
- the maintenance and test-matrix cost of dual SQL backends outweighs the
  value of preserving "single-binary + PVC durable mode" as a deployment
  shape
- ephemeral mode covers dev / homelab "I just want to see it run"
- Postgres covers durable / multi-replica / homelab-with-Postgres-already-installed

The existing guard against `persistence.enabled = true` with `replicaCount > 1`
stays in place — that guard is about RWO PVC semantics and is orthogonal.

The in-memory mode is preserved for local development (`just dev`) and for
tests. It is no longer a documented production deployment shape.

### 4. Retention is decided separately from current-state TTL

Current-state TTL eviction continues to behave as it does today (completed
runs/jobs evicted after a configurable interval). Outbox row retention is a
separate decision deferred to Phase 5, because the constraints differ:

- current-state TTL exists to keep the dashboard view bounded
- outbox retention exists to support replica fan-out recovery and optional
  audit/debug tooling

Tying them together would either evict outbox rows that recovery still needs
or retain current-state rows that the UI no longer wants to show.

Current-state TTL eviction becomes a SQL `DELETE` against the current-state
tables, replacing today's in-process tokio task that scans HashMaps under
the store's `RwLock`. Concurrent multi-replica eviction is correctness-safe
via PostgreSQL row locking — each row is DELETEd exactly once and other
replicas' identical statements simply affect zero rows. Cascade behavior
between `runs` and `jobs` (FK `ON DELETE CASCADE` vs. two-step DELETE vs.
independent run-row TTL) is a Phase 2 schema decision.

## Consequences

### Positive

- Frontend WS contract changes shape minimally — only the cursor field is
  renamed and its comparator inverted at one site.
- Frontend live-stream behavior remains unchanged in the common case.
- The chart's storage-mode story simplifies from three modes to two,
  reducing values-matrix surface and operator decision overhead.
- Outbox retention is free to be tuned for replication recovery without
  affecting the current-state TTL the dashboard view depends on.
- Dedupe deferral with explicit triggers gives a defensible "ship if observed"
  policy rather than a defensive "ship just in case" cost.

### Negative / costs

- **Removing SQLite mode breaks existing chart documentation** in
  `deploy/helm/atc/values.yaml:106-132` and any SQLite values-matrix tests
  under `deploy/helm/atc/tests/`. Both must be updated as part of the
  Phase 4 chart change. Operators currently running the SQLite mode in
  production (if any exist outside the maintainer's own use) need a
  migration note.

### Out of scope

- Storage architecture itself — see ADR 0002
- Outbox retention duration, eviction strategy, and any audit/history surface
- Whether the in-memory mode survives in production binaries or becomes a
  dev-only feature flag

## Implementation Status

- **Decision 1** (`StateSnapshot.seq` → `lastSeq` rename, `>=` → `>` comparator flip): **complete (Phase 3a, feat/phase-3a-3b-wire-contract).** The wire field is `lastSeq` (`#[serde(rename_all = "camelCase")]` over `last_seq: u64`); the snapshot reflects all events with `seq <= lastSeq`. The frontend buffer filter is `buffered.seq > snapshotLastSeq` (strict `>`); the bigint reviver allowlist now includes `'lastSeq'`. The in-memory `Mutex<u64>` counter shifted from post-increment to pre-increment (`*seq_guard += 1; let seq = *seq_guard;`) — first successful commit broadcasts `seq=1`, never `seq=0`. `lastSeq=0` is therefore the unambiguous "no events committed since startup" cold-start sentinel. Pre-increment was chosen over the alternative of leaving the counter post-increment and serializing `last_seq = *seq_guard - 1`: pre-increment is simpler at the call site, keeps `lastSeq=0` semantically clean, and produces no transient negative-cursor states during reorg.
- **Decision 2** (monotonic-not-gapless BIGSERIAL cursor on outbox): implemented in Phase 2c. The `outbox.seq BIGSERIAL PRIMARY KEY` materializes the durable cursor. Aborted transactions consume seq values without producing committed rows — verified by `phase_2c_outbox_ac2_2_bigserial_gap_property` test. The in-memory `Mutex<u64>` cursor remains the broadcast source through Phase 3c.
- **Decision 3** (operator error policy: 503 for transient PG failures): implemented in Phase 2c. Webhook handler returns 503 when `pool.begin()` or `tx.commit()` fail; parity rejections (predicated UPSERT 0 rows) return 200 `{"status":"rejected"}`.
- **Decision 4** (TTL eviction via SQL DELETE; outbox retention separate from current-state retention): deferred to Phase 5.

## Phase 3c implementation notes (2026-05-06)

REPEATABLE READ on snapshot reads is now the contract: `state_handler`'s PG branch opens a REPEATABLE READ transaction and reads runs/jobs/MAX(outbox.seq) from the same MVCC snapshot. This guarantees `lastSeq` is a true upper bound on the runs/jobs content. Without REPEATABLE READ, a concurrent webhook commit between the runs SELECT and the seq SELECT could advance `lastSeq` past content that the snapshot hasn't materialized — the frontend's `seq > lastSeq` filter at `connection.ts:113` would then permanently drop a real event.

The drain task implements bounded ring-buffer dedup (2048 seqs / ~16 KB per replica) to preserve this ADR's no-frontend-dedup stance under Phase 3c's gap-healing rescans. The rescan window can re-fetch a row already broadcast (when a delayed commit arrives after a rescan-eligible commit was forwarded); the ring suppresses the duplicate broadcast. Counter: `atc_pg_drain_duplicate_skipped_total`.

Decision 4 (PG-side TTL eviction deferred to Phase 5) still holds — the in-memory eviction task remains, but in PG mode the in-memory store stays empty so eviction is a no-op.

## Phase 4 implementation note (2026-05-06)

Decision 3 was implemented in Phase 4 (`docs/design-plans/2026-05-06-phase-4-multi-replica-enablement.md`, PR closing #7). Two clarifications worth recording for later readers:

- **`persistence.*` machinery removed alongside SQLite.** ADR 0003 D3 says SQLite is "removed entirely" but does not directly speak to the chart's `persistence.*` value surface, the `templates/pvc.yaml` template, or the persistence-conditional volume mounts in `deployment.yaml` — those existed only because SQLite-with-PVC was Mode 2. With SQLite gone, an audit (`grep -rn -i "persistence|persistent_volume|PersistentVolume|PVC"` over `backend/`, `frontend/src/`, and non-Helm `docs/`) returned zero hits referring to Kubernetes PVCs. There was no current consumer and no roadmapped consumer that requires a PVC over a ConfigMap/env-var configuration model, so the persistence surface was retired. If a future use case requires a PVC (e.g., a sidecar buffering audit logs to disk), the surface should be re-introduced tightly scoped to that consumer rather than as a general-purpose toggle.
- **The "existing guard ... stays in place" passage above no longer applies.** It referred to the pre-Phase-4 `persistence.enabled=true` + `replicaCount > 1` guard. That guard became unreachable when persistence was removed; Phase 4 replaced it with a `replicaCount > 1` ⇒ Postgres URL required guard at the same site in `templates/deployment.yaml`. The new guard is checked at template-render time, allowing remediation guidance in the failure message.

The chart now has two storage modes (ephemeral, external-Postgres) and a constant `RollingUpdate` strategy. Operators upgrading from a pre-Phase-4 chart with `persistence:` keys in their values files will see schema validation reject the unknown property — a deliberate breaking change in a 0.x chart, called out in the Phase 4 PR's release notes. See `docs/architecture/deployment.md` § "Storage-mode evolution" for the full historical context.

## Related

- ADR 0002 — [PostgreSQL outbox + symmetric replicas for live state](./0002-state-externalization-postgres-outbox.md)
- ADR 0004 — [Frontend-derived pool stats](./0004-frontend-derived-pool-stats.md)
- Issue: [#7 — design: externalize live state to support multi-replica deployments](https://github.com/bojanrajkovic/atc/issues/7)
- Research: [`docs/architecture/state-externalization-research/`](../architecture/state-externalization-research/README.md)
  - [`frontend-impact.md`](../architecture/state-externalization-research/frontend-impact.md) — cursor rename impact, optional hardening
  - [`rollout-and-implementation.md`](../architecture/state-externalization-research/rollout-and-implementation.md) — phased rollout plan
- Frontend cursor handling: `frontend/src/lib/connection.ts:14` (private field), `:116` (comparator)
- Helm multi-replica guard (Phase 4): `deploy/helm/atc/templates/deployment.yaml` (top of file)
- Chart storage-mode docs (Phase 4): `deploy/helm/atc/values.yaml` (the two-mode block under `config.databaseUrl`)
- Storage-mode-evolution history: `docs/architecture/deployment.md` § "Storage-mode evolution"
