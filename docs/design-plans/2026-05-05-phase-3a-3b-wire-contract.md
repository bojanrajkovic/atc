# Phase 3a + 3b: Wire-Contract Prerequisites

**Branch:** `feat/phase-3a-3b-wire-contract` off `main` (tip: 340f675)
**PR target:** `main`
**PR title (squash commit subject):** `feat: align the state cursor contract and derive runner pools in the frontend`
**Implementation guidance:** `docs/implementation-guidance.md` governs all implementation work for this plan.

---

## Context

ATC's state-externalization rollout is moving the live-state read path from an in-memory `StateStore` to a Postgres-backed transactional outbox (ADR 0002). Phase 2 landed the durable write path in shadow mode: every webhook now produces both an in-memory mutation AND a durable PG write inside one transaction, with a NOTIFY firing post-commit and a listener task draining the outbox (currently it just logs; no WS forwarding yet).

Phase 3 is the read-path cutover. It has three sub-phases:

1. **Phase 3a (this PR):** Rename the snapshot cursor field and shift its semantics. ADR 0003 commits to `lastSeq` ("highest committed seq") in place of `seq` ("next seq to assign"), with the frontend filter comparator inverting from `>=` to `>`. The frontend lives at exactly one cursor site (`connection.ts`).
2. **Phase 3b (this PR):** Move pool-stats derivation from the backend to the frontend. ADR 0004 commits to deleting `StateStore::pool_stats()`, removing `pool_stats` from `StateSnapshot` and `pool_stats_after` from `SeqEvent`, and adding a `$derived.by` in `RunnerStore` that replicates the algorithm.
3. **Phase 3c (next PR):** Switch `GET /v1/state` to read from PG and `/v1/ws` to forward from the outbox via the listener/drain task. The exact disposition of the in-memory store and counter is 3c's concern (ADR 0003 preserves them as a dev/test runtime mode); 3a/3b don't presuppose a shape.

3a and 3b ship together because they are wire-contract changes, and the binary is single-artifact (frontend embedded via `rust-embed`, no transition window). They both must land before 3c so that the wire shape Phase 3c reads from PG is the same shape the frontend already expects. Splitting them into separate PRs would require two binary versions where the frontend filter logic and pool-stats codepath are mid-migration — pointless churn given the lockstep deploy.

The motivation for combining them is locked. Two design correctness concerns drive the architecture below:

- **Cursor semantics under cold start.** A naive `lastSeq = seq_guard.saturating_sub(1)` against the current 0-based post-increment counter is *not* mathematically equivalent to the old `>=` comparator. There is a reachable race at server cold start (snapshot returns `lastSeq=0`; webhook commits `seq=0` over WS *after* the mutex is released but *before* the client processes the snapshot; new filter `0n > 0n` is false; first event silently dropped). Switching the in-memory counter to **pre-increment (1-based)** in Phase 3a eliminates the boundary case and aligns with the BIGSERIAL outbox cursor — Phase 3c then flips the source without changing semantics. The cost is mechanical: tests asserting specific seq values shift from `0,1,2…` to `1,2,3…`.
- **Frontend pool derivation fidelity.** `StateStore::pool_stats()` groups jobs by `LabelSet` (sort + dedupe), iterates `Queued`/`InProgress` only, and detects elastic groups by `RunnerInfo.group_id == 0`. The frontend replica must match all three: dedupe via `Set` (not just sort), use a collision-free key (JSON-stringified or `\x00`-delimited — labels can theoretically contain commas), and compare `groupId === 0n` because `RunnerInfo.groupId` is `bigint | null` (a literal `0` would never match).

---

## Summary

- **Phase 3a:**
  - In-memory `Mutex<u64>` switches to **pre-increment** (1-based; first event broadcasts `seq=1`).
  - `StateSnapshot.seq` → `StateSnapshot.lastSeq`; value is `*seq_guard` (no `saturating_sub`).
  - Frontend filter: `buffered.seq > snapshotLastSeq` (renamed local; inverted comparator).
  - Frontend JSON reviver allowlist gains `'lastSeq'`; keeps `'seq'` (still needed for `SeqEvent.seq`).
- **Phase 3b:**
  - Delete `StateStore::pool_stats()` and pool-stats inline in `snapshot()`.
  - Remove `pool_stats` from `StateSnapshot`, `pool_stats_after` from `SeqEvent`.
  - Add a flat `jobs: Job[]` `$derived.by` to `RunStore` (single source the pool derivation iterates).
  - Rewrite `RunnerStore.pools` as a `$derived.by` that replicates `LabelSet` semantics and uses bigint-aware grouping.

Both changes ship in one binary version bump.

---

## Definition of Done

**Primary deliverables:**

1. In-memory `webhook_handler` is pre-increment in both PG and in-memory paths: `*seq_guard += 1; let seq = *seq_guard;`. First event after server start broadcasts `seq=1`.
2. `StateSnapshot.lastSeq: u64` (renamed from `seq`); value is `*seq_guard` (no `saturating_sub`); ts-rs regenerates the TypeScript type.
3. `connection.ts` field renamed (`snapshotLastSeq`); comparator inverted (`>`); JSON reviver allowlist includes `'lastSeq'`.
4. `pool_stats: Vec<RunnerPoolStats>` removed from `StateSnapshot`; `pool_stats_after` removed from `SeqEvent`; `StateStore::pool_stats()` deleted; pool-stats inline removed from `snapshot()`.
5. `RunStore` has a `jobs: Job[]` `$derived.by` flat view across `jobsByRun.values()`.
6. `RunnerStore.pools` is `$derived.by` over `runStore.jobs`; replicates `LabelSet` semantics (dedup via `Set`); uses `JSON.stringify(sortedLabels)` as collision-free map key; uses `groupId === 0n` for elastic detection. No `loadPools()`/`clear()` remain on `RunnerStore`; no callers reference them.
7. `frontend/CLAUDE.md` updated to remove `poolStatsAfter` sidecar references and add `RunStore.jobs` derived.
8. `just types` regenerates clean and is idempotent; no manual edits to any generated file.
9. All checks green: `just lint`, `just check`, `cargo test`, `pnpm test`, `pnpm test:e2e`.

**Success criteria:**

- Existing connection-buffering tests pass with renamed field, inverted comparator, and shifted seq values.
- The existing equality boundary test in `connection.buffering.test.ts:206` is **inverted** (event with `seq == lastSeq` is now DISCARDED), not duplicated.
- A new empty-snapshot/first-event test verifies the pre-increment fix: `lastSeq=0n`, buffered `seq=1n` → DISPATCHED.
- Pool-display assertions pass with the derived store; `runnerStore.pools` is read-only (compile error on `loadPools`).
- A test verifies that `LabelSet`-equivalent inputs (e.g., `['a','a','b']` vs `['a','b']`) key identically in the derivation.
- Duplicate Job event produces idempotent `runnerStore.pools` (deep-equal across two dispatches).
- A backend test verifies pre-increment: the first webhook after server start broadcasts `seq=1`, never `seq=0`.

**Key exclusions:**

- Phase 3c (read-path cutover from PG; whether the in-memory store/counter remain as a dev/test runtime mode is 3c's call) — not in this PR.
- `outbox.seq` BIGSERIAL is unchanged (already 1-based; aligns with the new in-memory pre-increment).
- No Helm chart changes.

---

## Architecture

### Phase 3a: Cursor rename + in-memory counter pre-increment

**Why pre-increment:** With post-increment, `lastSeq = seq_guard.saturating_sub(1)` overloads `0` to mean both "no commits" and "first commit just landed". Reachable race:

1. Client opens WS, starts snapshot fetch.
2. `state_handler` runs while `seq_guard=0` → snapshot is empty + `lastSeq=0`. Mutex released.
3. Webhook arrives, broadcasts `seq=0` over WS, increments counter to 1.
4. Client buffers the WS event before the snapshot response is processed.
5. Filter `0n > 0n` → false → first event silently dropped.

Pre-increment makes `1` the first valid seq, so `lastSeq=0` is unambiguously "no commits". The filter `seq > 0` correctly replays seq=1 in the race above. The 1-based numbering also matches the BIGSERIAL outbox cursor exactly, so Phase 3c will flip the source from `*seq_guard` to `COALESCE(MAX(outbox.seq), 0)` without changing semantics.

**Backend:**

- `webhook_handler` (`backend/crates/atc-server/src/routes.rs` — both PG and in-memory paths): change seq assignment from `let seq = *seq_guard; *seq_guard = seq + 1;` to `*seq_guard += 1; let seq = *seq_guard;`. Verify the `SeqEvent` broadcast and the outbox `INSERT` use the post-increment value.
- `state_handler` (`backend/crates/atc-server/src/routes.rs`): `let last_seq = *seq_guard;` (no `saturating_sub`). Lock is still held across snapshot read (preserves invariant from `routes.rs:176`).
- `StateSnapshot` (`backend/crates/atc-server/src/routes.rs`): rename `seq: u64` → `last_seq: u64`. Update doc comment to "highest committed seq". Remove `pool_stats: Vec<RunnerPoolStats>` field (Phase 3b).
- ts-rs generates `StateSnapshot.ts` with `lastSeq: bigint` (camelCased via `#[serde(rename_all = "camelCase")]`).

**Correctness table (post-fix):**

| seq_guard | Events committed | lastSeq | Reachable scenario | Filter behavior |
|-----------|-----------------|---------|--------------------|-----------------|
| 0 | 0 | 0 | Cold start, no events | Buffer empty (no broadcast); no filter test. ✓ |
| 0→1 (race) | 1 (seq=1) | 0 (snapshot pre-webhook) | First event lands during snapshot fetch | Buffered `seq=1n > 0n` → replay. ✓ |
| 1 | 1 | 1 | Steady state after one event | Snapshot includes the event; buffered `seq=1n > 1n` → discard. ✓ |
| N (N≥1) | N | N | Steady state | `seq ≤ N` discarded; `seq > N` replayed. ✓ |

**Frontend (`frontend/src/lib/connection.ts`):**

- `:14`: rename `private snapshotSeq: bigint = 0n` → `private snapshotLastSeq: bigint = 0n`.
- `:25` (JSON reviver allowlist): add `'lastSeq'` to the array. KEEP `'seq'` — `SeqEvent.seq` is still on the wire and still needs bigint conversion.
- `:109`: `this.snapshotSeq = snapshot.seq` → `this.snapshotLastSeq = snapshot.lastSeq`.
- `:116`: `buffered.seq >= this.snapshotSeq` → `buffered.seq > this.snapshotLastSeq`.

### Phase 3b: Pool stats to frontend

**Backend deletions** (verify line numbers at edit time — they may have drifted):

| Item | Location | Action |
|------|----------|--------|
| `StateStore::pool_stats()` | `backend/crates/atc-core/src/store.rs` | Delete the method |
| Pool-stats inline in `snapshot()` | `backend/crates/atc-core/src/store.rs` | Remove pool-stats compute; return type becomes `QueryResult` |
| `pool_stats: Vec<RunnerPoolStats>` field | `StateSnapshot` in `backend/crates/atc-server/src/routes.rs` | Delete field |
| `pool_stats_after: Option<Vec<…>>` field | `SeqEvent` in `backend/crates/atc-server/src/state.rs` | Delete field |
| `pool_stats_after` compute (PG path) | `backend/crates/atc-server/src/routes.rs` webhook handler | Remove `Some(state.store.pool_stats().await)` |
| `pool_stats_after` compute (in-memory path) | same | Remove the equivalent compute |
| `SeqEvent { … }` literal | both webhook paths | Remove `pool_stats_after` field |

**`RunnerPoolStats` is retained** (in `atc-core/src/lib.rs` or wherever it lives). The frontend still consumes the ts-rs-generated TypeScript type for derivation results.

After removal, `SeqEvent` is:

```rust
pub struct SeqEvent {
    pub seq: u64,
    pub event: WebhookEvent,
}
```

Run `just types` after.

**Frontend — RunStore additions (`frontend/src/lib/stores/runs.svelte.ts`):**

`RunStore` currently exposes `jobsByRun: SvelteMap<bigint, Job[]>`. Add a flat `$derived.by` view for consumers (the pool derivation, future filtering work):

```typescript
class RunStore {
  // existing: runs, jobsByRun, jobsByRunId, jobStatsByRun, …

  /** Flat view across all runs. Used by RunnerStore.pools and any future
   *  consumer that needs to iterate jobs without grouping. Iterates
   *  jobsByRun.values() to avoid manual flattening at consumer sites. */
  jobs = $derived.by<Job[]>(() => {
    const result: Job[] = []
    for (const arr of this.jobsByRun.values()) {
      for (const job of arr) result.push(job)
    }
    return result
  })
}
```

**Frontend — RunnerStore rewrite (`frontend/src/lib/stores/runners.svelte.ts`):**

`runnerStore` (not `runStore`) owns the pool derivation — pool stats are a view concern, isolated from run/job lifecycle. `TopBar.svelte` already reads `runnerStore.pools` and does not change.

```typescript
import type { Job } from '$lib/types/generated/Job'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
import { runStore } from './runs.svelte'

/** Frontend replica of backend `StateStore::pool_stats()`.
 *  - LabelSet equivalence: dedupe via Set, then sort.
 *  - Collision-free map key: JSON.stringify on the deduped sorted array
 *    (commas inside labels would break a naive `join(',')` key).
 *  - Bigint-aware: `groupId === 0n` (RunnerInfo.groupId is bigint | null;
 *    a literal `0` would never match).
 *  Pure function for testability — exported separately from the store. */
export function computePoolStats(jobs: Job[]): RunnerPoolStats[] {
  const statsMap = new Map<string, RunnerPoolStats>()

  for (const job of jobs) {
    if (job.status === 'Waiting' || job.status === 'Completed') continue

    const sortedLabels = [...new Set(job.labels)].sort()
    const key = JSON.stringify(sortedLabels)
    if (!statsMap.has(key)) {
      statsMap.set(key, {
        labels: sortedLabels,
        queued: 0,
        running: 0,
        groupName: null,
        isElastic: false,
        total: null,
      })
    }
    const entry = statsMap.get(key)!

    if (job.status === 'Queued') {
      entry.queued++
    } else if (job.status === 'InProgress') {
      entry.running++
      if (job.runner?.groupName != null) {
        entry.groupName = job.runner.groupName
      }
      if (job.runner?.groupId === 0n) {
        entry.isElastic = true
      }
    }
  }

  return [...statsMap.values()].sort((a, b) =>
    JSON.stringify(a.labels).localeCompare(JSON.stringify(b.labels))
  )
}

class RunnerStore {
  readonly pools = $derived.by(() => computePoolStats(runStore.jobs))
}

export const runnerStore = new RunnerStore()
```

`loadPools()` and `clear()` are deleted. The derived value resets automatically when `runStore.jobsByRun` mutates. Note: `Job.runner` is `RunnerInfo | null` (nullable, not optional), but `?.` chains correctly through both — the pattern is fine.

**Frontend — call-site cleanup:**

- `frontend/src/lib/dispatcher.ts:104-106`: remove `if (seqEvent.poolStatsAfter != null) { runnerStore.loadPools(seqEvent.poolStatsAfter) }`.
- `frontend/src/lib/connection.ts` (~108): remove `runnerStore.loadPools(snapshot.poolStats)`. Pool will derive automatically from loaded runs/jobs.

---

## Phase 3c Compatibility (Dual-Mode Preservation)

ADR 0003 keeps the in-memory `Mutex<u64>` counter alive through Phase 3c and explicitly preserves the in-memory mode for `just dev` and tests; whether it survives in production binaries is deferred to Phase 5 ("Decide whether the production in-memory path remains as a dev-only mode or is removed entirely"). Therefore every decision in this PR must be valid for BOTH read-paths Phase 3c can plausibly choose between: the existing in-memory path AND a future PG-backed path.

| Aspect | In-memory mode (`just dev`, tests) | PG mode (3c+ production) | Wire-shape difference |
|--------|-----------------------------------|--------------------------|-----------------------|
| Cursor source | `*seq_guard` (pre-increment, 1-based) | `COALESCE(MAX(outbox.seq), 0)` (BIGSERIAL, 1-based) | None — identical values |
| `lastSeq = 0` semantics | "No webhooks since boot" | "No outbox rows" | Identical |
| First event seq | `1` (pre-increment guarantees) | `1` (BIGSERIAL starts at 1) | Identical |
| Pool stats source | Frontend derives from `runStore.jobs` | Same — derivation is store-agnostic (single-writer dispatcher) | None |
| `StateSnapshot` shape | `{ last_seq, runs, jobs }` from in-memory store | Same struct, sourced from PG SELECTs | None |
| Filter behavior | `buffered.seq > lastSeq` ⇒ replay; else discard | Identical | Identical |

**Why pre-increment is the right primitive for dual-mode:** BIGSERIAL starts at 1; pre-increment in-memory is also 1-based by construction. The frontend filter works identically in both modes — no per-mode branching, no "did this snapshot come from PG or in-memory?" detection. Phase 3c can flip `state_handler` between `*seq_guard` and `COALESCE(MAX(outbox.seq), 0)` (or run them side-by-side under a config flag) with no other change to the wire contract.

**What this plan does NOT decide (deferred to Phase 3c):**
- Whether `state_handler` reads `*seq_guard` (in-memory mode) or `COALESCE(MAX(outbox.seq), 0)` (PG mode) at runtime, behind a build feature, behind a config flag, or via parallel `AppState` shapes.
- Whether the in-memory `StateStore` survives in production builds or becomes a dev-only feature flag (per ADR 0003 Out of scope; Phase 5 territory).

**What this plan does NOT add:**
- No PG-only assumption (e.g., `INTEGER unsigned` overflow handling specific to BIGSERIAL).
- No in-memory-only assumption (e.g., gapless seq, since BIGSERIAL is monotonic-not-gapless per ADR 0003 Decision 2 — and the frontend already tolerates gaps because it does no contiguity checks).
- No assumption about which path Phase 3c picks first.

---

## Sub-Agent Delegation

Per Rule 14 of `docs/implementation-guidance.md`, the orchestrating context does NOT write code inline. It dispatches to sub-agents, reviews their output, and sequences phases.

| Phase | Delegate to | Why this agent |
|-------|-------------|----------------|
| 1 — Backend wire-contract | `ed3d-basic-agents:sonnet-general-purpose` | Cross-file Rust edits across both PG and in-memory paths; needs detail discipline to keep them in sync |
| 2 — Type regeneration | `ed3d-basic-agents:haiku-general-purpose` | Run `just types`, diff-verify expected files only |
| 3 — Frontend wire-contract | `ed3d-basic-agents:sonnet-general-purpose` | Cross-file TS edits + new derived view in `runs.svelte.ts` + `RunnerStore` rewrite |
| 4a — Backend test mechanical | `ed3d-basic-agents:haiku-general-purpose` | Pure rename + numeric shift across many files |
| 4b — Backend test reasoning | `ed3d-basic-agents:sonnet-general-purpose` | Sidecar relocation, outbox positive/negative guard logic, new pre-increment assertion |
| 5a — Frontend test mechanical | `ed3d-basic-agents:haiku-general-purpose` | Pure rename + seq shifts in fixture literals |
| 5b — Frontend test reasoning | `ed3d-basic-agents:sonnet-general-purpose` | Boundary test inversion, new pool-derivation tests, e2e selector rewrite |
| 6 — Documentation sweep | `ed3d-extending-claude:project-claude-librarian` | Coordinated update across architecture docs, CLAUDE.md/AGENTS.md pairs, ADRs |
| 7 — Final verification | Orchestrator | Run `just lint`, `just check`, `just test`, `just types` for the go/no-go gate |

**Parallelism opportunities:**
- Phases 4a + 5a (mechanical rename/shift work) dispatch in parallel after Phase 3 lands — independent test surfaces.
- Phases 4b + 5b (reasoning work) dispatch in parallel after their respective mechanical predecessors complete.
- Phase 6 (docs) waits on all code changes settling so the librarian sees the final diff.

**Sequencing constraints:**
- Phase 2 ⇐ Phase 1 (TS types regenerate from Rust)
- Phase 3 ⇐ Phase 2 (frontend imports the regenerated TS types)
- Phases 4 + 5 ⇐ Phase 3 (tests reference the new symbol shapes)
- Phase 7 ⇐ all prior phases

---

## Implementation Phases

### Phase 1 — Backend wire-contract changes
**Delegate to:** `ed3d-basic-agents:sonnet-general-purpose` (single agent; sequential within).

Tasks for the sub-agent:
1. `webhook_handler` (both PG and in-memory paths in `backend/crates/atc-server/src/routes.rs`): switch to pre-increment (`*seq_guard += 1; let seq = *seq_guard;`). Verify the broadcast and outbox INSERT use the post-increment value in BOTH paths.
2. `state_handler` (`backend/crates/atc-server/src/routes.rs`): `let last_seq = *seq_guard;` (no `saturating_sub`). Lock is still held across snapshot read.
3. `StateSnapshot` (`backend/crates/atc-server/src/routes.rs`): rename `seq: u64` → `last_seq: u64`; remove `pool_stats` field; update doc comment to "highest committed seq".
4. `atc-core/src/store.rs` `snapshot()`: remove pool-stats compute; return `QueryResult` only.
5. `atc-core/src/store.rs`: delete `pool_stats()` method.
6. `atc-server/src/state.rs` `SeqEvent`: remove `pool_stats_after` field and its doc.
7. `routes.rs` webhook (both paths): remove `pool_stats_after` compute and constructor field.
8. Gate: `cargo check -p atc-server -p atc-core` must pass before reporting back.

### Phase 2 — Type regeneration
**Delegate to:** `ed3d-basic-agents:haiku-general-purpose`.

Tasks for the sub-agent:
1. Run `just types`.
2. Verify the diff:
   - `frontend/src/lib/types/generated/StateSnapshot.ts` has `lastSeq: bigint`; no `seq`, no `poolStats`.
   - `frontend/src/lib/types/generated/SeqEvent.ts` has no `poolStatsAfter`.
   - `frontend/src/lib/types/generated/RunnerPoolStats.ts` is unchanged (still needed for derivation result type).
   - No other generated file changes unexpectedly.
3. Run `just types` again — confirm zero diff (idempotence gate).

### Phase 3 — Frontend wire-contract changes
**Delegate to:** `ed3d-basic-agents:sonnet-general-purpose` (single agent; sequential within).

Tasks for the sub-agent:
1. `connection.ts:14`: rename `snapshotSeq` → `snapshotLastSeq`.
2. `connection.ts:25` (JSON reviver allowlist): add `'lastSeq'`. KEEP `'seq'` — `SeqEvent.seq` is still on the wire and still needs bigint conversion.
3. `connection.ts:109`: `this.snapshotSeq = snapshot.seq` → `this.snapshotLastSeq = snapshot.lastSeq`.
4. `connection.ts:116`: invert comparator — `buffered.seq > this.snapshotLastSeq`.
5. `connection.ts` (~108): remove `runnerStore.loadPools(snapshot.poolStats)`.
6. `dispatcher.ts:104-106`: remove the `if (seqEvent.poolStatsAfter != null) …` sidecar branch.
7. `runs.svelte.ts`: add the flat `jobs: Job[]` `$derived.by` per Architecture.
8. `runners.svelte.ts`: rewrite per Architecture — extract `computePoolStats` as exported pure function; change `pools` to `$derived.by`; delete `loadPools` and `clear`.
9. Gate: `pnpm check` passes; no remaining references to `poolStats`, `poolStatsAfter`, or `snapshotSeq`.

### Phase 4 — Backend test churn
Two sub-phases run sequentially within Phase 4. **Phase 4 as a whole runs in parallel with Phase 5** (independent test surfaces).

#### Phase 4a — mechanical
**Delegate to:** `ed3d-basic-agents:haiku-general-purpose`.

Tasks for the sub-agent:
- Shift asserted seq values from `N` to `N+1` in: `state_tests.rs`, `routes_tests.rs`, `e2e_tests.rs`, `transactional_writes_tests.rs`, and any other backend integration test asserting specific numeric seq values.
- Rename `json["seq"]` → `json["lastSeq"]` everywhere in snapshot-response assertions.
- Remove `poolStats` field references from `StateSnapshot` fixtures.

#### Phase 4b — reasoning
**Delegate to:** `ed3d-basic-agents:sonnet-general-purpose`.

Tasks for the sub-agent:
- `state_tests.rs`: empty-snapshot assertion expects `lastSeq == 0`; after one committed event (`seq=1`), snapshot expects `lastSeq == 1`. Remove `SeqEvent` serialization tests for `poolStatsAfter` (populated and null cases).
- `outbox_tests.rs`: KEEP the negative shape guards (`seq` and `pool_stats_after` absent on the payload — serde silently ignores unknown fields, so absence checks remain load-bearing). UPDATE the positive assertion to "payload deserializes as the domain envelope (`RunEventEnvelope` / `JobEventEnvelope`)". Both guards must hold.
- `sidecar_tests.rs`: relocate the final invalid-Job-transition test (asserts no broadcast on invalid transition — this is non-sidecar coverage) to `state_tests.rs` or `webhook_ingestion_tests.rs`. Then delete the rest of `sidecar_tests.rs`.
- `atc-core/src/store/tests/runner_pools.rs`: tests the deleted `pool_stats()` method. Remove the file. The frontend now owns derivation; backend self-tests for it are out of scope.
- Add a pre-increment assertion test (in `routes_tests.rs` or `webhook_ingestion_tests.rs`): the first webhook after server start broadcasts `seq=1`, never `seq=0`.
- Verify `webhook_ingestion_tests.rs` and `ws_tests.rs` do not currently assert on snapshot `seq` or `poolStats` (no rename touches needed there for the snapshot field; SeqEvent seq value shifts may still apply and were handled in 4a).
- Gate: `cargo test` passes.

### Phase 5 — Frontend test churn
Two sub-phases run sequentially within Phase 5. **Phase 5 as a whole runs in parallel with Phase 4.**

#### Phase 5a — mechanical
**Delegate to:** `ed3d-basic-agents:haiku-general-purpose`.

Tasks for the sub-agent:
- Rename `seq:` → `lastSeq:` in `StateSnapshot` fixture literals across: `frontend/src/lib/__tests__/connection-test-helpers.ts` (note the actual path is under `__tests__/`), `connection.connect.test.ts`, `connection.buffering.test.ts`, `connection.reconnect.test.ts`, `connection.aria-silence.test.ts`, `frontend/e2e/lib/ws-mock.ts`.
- Shift asserted `SeqEvent.seq` numeric values from `N` to `N+1` across tests that construct `SeqEvent` fixtures or assert specific seq numbers: `connection.*.test.ts`, `dispatcher.test.ts`, `dispatcher.browser.test.ts`, `dispatcher.perf.browser.test.ts`, `aria/live-region.test.ts`, `aria/transition-kinds.test.ts`, `stores/runners.test.ts`, `e2e/lib/ws-mock.ts`.

#### Phase 5b — reasoning
**Delegate to:** `ed3d-basic-agents:sonnet-general-purpose`.

Tasks for the sub-agent:
- `connection-test-helpers.ts`: remove `poolStats: []` from `defaultSnapshot` (after the rename in 5a).
- `connection.buffering.test.ts:206`: INVERT the existing equality boundary test — old expectation was `seq == snapshot.seq` REPLAYED; new expectation is `seq == snapshot.lastSeq` DISCARDED. Don't add a duplicate boundary test.
- `connection.buffering.test.ts:287`: the sidecar-specific case (related to `poolStatsAfter`) — rewrite if it's actually testing buffering semantics, or remove if it's only testing the sidecar dispatch.
- ADD a new test in `connection.buffering.test.ts`: empty-snapshot/first-event path — `snapshotLastSeq=0n`, buffered `seq=1n` → DISPATCHED. Documents the pre-increment fix.
- `dispatcher.browser.test.ts`: remove `poolStatsAfter` tests; add pool-derivation tests:
  - Queued Job (labels `['ubuntu-latest']`) → pool with `queued=1, running=0`.
  - Same job InProgress with runner `{ groupId: 0n, groupName: 'Default' }` → pool with `running=1, queued=0, isElastic=true, groupName='Default'`.
  - Then Completed → pool gone.
  - Duplicate Job event → idempotent recompute (deep-equal across two dispatches).
  - LabelSet parity: a Job with labels `['a','a','b']` keys identically to a Job with labels `['a','b']`.
- `dispatcher.test.ts:195`: similar sidecar-related coverage — rewrite or remove.
- `stores/runners.test.ts`: rewrite for the `$derived.by` shape — exercise `computePoolStats` directly + via `runnerStore.pools` after `runStore.jobsByRun` mutations.
- `aria/live-region.test.ts:31` and `aria/transition-kinds.test.ts:23`: if these only need fixture renames + seq shifts, they fell under 5a. If they assert seq numbers semantically (e.g., "burst announces 5 events numbered 0–4"), update assertions to the new 1-based numbering.
- `e2e/lib/ws-mock.ts`: remove `poolStatsAfter` from `makeJobSeqEvent`. Update any helper that constructs synthetic snapshots to use `lastSeq` and 1-based seqs.
- `pool-indicators.test.ts` (e2e): full rewrite. Drive via WS Job events with runner data. Selector text changes too: with frontend derivation, queued-only pools label by `labels.join(', ')` (e.g., `runner-pool-ubuntu-latest`) — `groupName` only populates from InProgress jobs with runner info. Update selectors to match `TopBar.svelte`'s rendering rule (`groupName ?? labels.join(', ')`). Core assertions (pool shown, queued badge, running count, pool disappears on Completed) remain.
- Gate: `pnpm test` (unit + browser) and `pnpm test:e2e` pass.

### Phase 6 — Documentation sweep
**Delegate to:** `ed3d-extending-claude:project-claude-librarian`.

Tasks for the sub-agent — apply each row of the Documents to Update table (below). Pair-symlink rule: every `CLAUDE.md` change must be reflected in its `AGENTS.md` symlink (already symlinked; no separate edit needed, but the librarian must verify).

Gate: `scripts/check-docs-lefthook.sh` passes (the pre-push doc-staleness gate).

### Phase 7 — Final verification
**Run by orchestrator** — no delegation; this is the dispatch-and-review final gate.

1. `just lint` passes.
2. `just check` passes.
3. `just test` full suite passes.
4. `just types` is idempotent (running it twice produces no diff).
5. `just dev` smoke test: open dashboard; confirm initial snapshot has `lastSeq` field (devtools network tab); fire a webhook via test fixture; confirm:
   - First event after a fresh start has `seq=1` on the wire (verifies dual-mode parity with future BIGSERIAL).
   - Pool indicator updates as job events flow (Queued → InProgress → Completed).
   - WS reconnect after a forced disconnect correctly filters buffered events with the new comparator.
6. Pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) passes.

---

## Acceptance Criteria

**Phase 3a:**
- AC1: `just types` produces `StateSnapshot.ts` with `lastSeq: bigint`; no `seq` field.
- AC2: `state_tests.rs` assertions: empty snapshot has `json["lastSeq"] == 0`; after one event committed (which now has `seq=1`), snapshot has `json["lastSeq"] == 1`.
- AC3: `connection.ts:116` reads `buffered.seq > this.snapshotLastSeq`.
- AC4: `connection.ts:25` JSON reviver includes `'lastSeq'` in the allowlist; `'seq'` is preserved.
- AC5: Boundary test inverted at `connection.buffering.test.ts:206`: event with `seq == lastSeq` is DISCARDED. The duplicate that the prior plan would have added is NOT present.
- AC6: New empty-snapshot/first-event test passes: `lastSeq=0n`, buffered `seq=1n` → DISPATCHED.
- AC7: Pre-increment assertion: a backend test confirms the first webhook broadcasts `seq=1`, not `seq=0`.
- AC8: All existing connection tests (buffering, connect, reconnect, aria-silence) pass with renamed fixtures and shifted seq values.

**Phase 3b:**
- AC9: `just types` produces `SeqEvent.ts` with no `poolStatsAfter`; `StateSnapshot.ts` has no `poolStats`. `RunnerPoolStats.ts` still exists.
- AC10: `cargo test` passes after `sidecar_tests.rs` deletion (with invalid-transition test relocated), `state_tests.rs` pool removals, `runner_pools.rs` removal, and `outbox_tests.rs` keeping negative shape guards.
- AC11: `runStore.jobs` is a flat `$derived.by` `Job[]` view across `jobsByRun.values()`.
- AC12: `runnerStore.pools` is not `$state`; calling `runnerStore.loadPools` is a compile error.
- AC13: After dispatching a Queued Job with labels `['ubuntu-latest']`, `runnerStore.pools` contains one entry with `queued=1, running=0`.
- AC14: After dispatching InProgress for the same job with runner `{ groupId: 0n, groupName: 'Default' }`: `running=1, queued=0, isElastic=true, groupName='Default'`. After Completed: pool gone.
- AC15: Duplicate Job event dispatch produces idempotent `runnerStore.pools` (deep-equal across two dispatches).
- AC16: `LabelSet` parity: a Job with labels `['a','a','b']` and a Job with labels `['a','b']` resolve to the same pool entry (dedup correctness).
- AC17: `pool-indicators.test.ts` e2e passes against job-event-driven fixtures with selectors matching the `groupName ?? labels.join(', ')` rule.
- AC18: `pnpm test` (unit + browser) + `pnpm test:e2e` pass.

---

## Documents to Update

| Document | Change |
|----------|--------|
| `docs/architecture/backend-server.md` | Update snapshot schema (`lastSeq`, no `poolStats`); SeqEvent schema (no `poolStatsAfter`); note `pool_stats()` deleted; document pre-increment counter |
| `docs/architecture/frontend-app.md` | Update RunnerStore section (derived pools, no `loadPools`); update wire-contract section; document `RunStore.jobs` flat derived view |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Mark Phase 3a + 3b complete; record pre-increment shift; link PR |
| `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` | Implementation Status: Decision 1 → "complete (Phase 3a, PR #XX)"; record the in-memory pre-increment shift |
| `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` | Implementation Status → "complete (Phase 3b, PR #XX)" |
| `backend/crates/atc-server/CLAUDE.md` | Update wire-contract description; remove `poolStatsAfter` reference; note pre-increment counter |
| `backend/crates/atc-core/CLAUDE.md` | Note `pool_stats()` removed in Phase 3b |
| `frontend/CLAUDE.md` | Remove `dispatcher.ts` "applies `SeqEvent.poolStatsAfter` sidecar" wording (line ~27); remove `e2e/lib/ws-mock.ts` "with `poolStatsAfter` sidecar"; add `RunStore.jobs` flat derived to Store Additions section |

`scripts/doc-mapping.sh` already maps `backend/crates/*/src/*` → `backend-server.md` and `frontend/src/*` → `frontend-app.md`. No new mappings needed.

---

## Glossary

| Term | Definition |
|------|-----------|
| `lastSeq` | Highest committed sequence number at snapshot time. `0` is the unambiguous sentinel for "no events committed yet" (because seq starts at 1 with pre-increment). Computed as `*seq_guard` in 3a; will become `COALESCE(MAX(outbox.seq), 0)` in 3c — same value in both modes. |
| `seq_guard` | The in-memory `Mutex<u64>` counter in `AppState`. **Pre-increment** as of Phase 3a: counter starts at 0; webhook handler increments first, then reads, so the first broadcast event has `seq=1`. |
| Buffer invariant | A buffered WS event can only exist if the server committed and broadcast it. With pre-increment, any committed event has `seq ≥ 1`, so `lastSeq=0` unambiguously means "no commits". The filter `seq > 0` correctly replays the very first event when the snapshot was empty at fetch time. |
| Empty-snapshot/first-event race | Reachable on cold start: snapshot taken while `seq_guard=0`; webhook commits `seq=1` over WS *after* the mutex is released but *before* the client processes the snapshot. With pre-increment + `seq > lastSeq`, the buffered seq=1 is correctly replayed. (Post-increment would have discarded it — see Architecture rationale.) |
| `LabelSet` | Backend type in `atc-core` that sorts and dedupes labels. The frontend replica uses `[...new Set(labels)].sort()` for the same semantics; the map key is `JSON.stringify(sortedLabels)` to avoid commas-in-labels collisions. |
| `computePoolStats` | Module-level pure function in `runners.svelte.ts` that maps `Job[]` to `RunnerPoolStats[]`, replicating `StateStore::pool_stats()`. Exported separately from the store for direct testability. |
| `RunStore.jobs` | Flat `$derived.by<Job[]>` view across `runStore.jobsByRun.values()` — the single source the pool derivation iterates. Reusable by future consumers that need ungrouped jobs. |
| Pool derivation chain | `runStore.jobsByRun` (`SvelteMap`) → `runStore.jobs` (`$derived.by` flat) → `runnerStore.pools` (`$derived.by`) → `TopBar` reads `runnerStore.pools` → `RunnerBar` props. |
