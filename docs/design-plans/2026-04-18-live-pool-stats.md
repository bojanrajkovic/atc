# Live Pool Stats Design

## Summary

This feature closes the gap between the backend's runner pool state and the frontend's live display. Currently, the TopBar pool indicators are seeded at WebSocket-connect time from a snapshot but never update in-session as jobs flow through their lifecycle. The fix is a **sidecar field** — `pool_stats_after` — added to `SeqEvent`, the server-layer envelope that already wraps every broadcast event with a sequence number. For each successful Job event, the server computes the full `RunnerPoolStats[]` immediately after applying the mutation, while still holding the seq mutex, and attaches it to the outbound `SeqEvent`. Run events and failed transitions carry `None`. Because the derivation happens inside the existing critical section, every subscriber receives a self-consistent snapshot of "pool state as it was after this exact mutation," with no re-derivation required on the client.

On the frontend, the `EventDispatcher`'s routing method changes to accept the full `SeqEvent` instead of just the primitive `WebhookEvent`. After routing the event to `RunStore` as before, the dispatcher checks the sidecar: if `poolStatsAfter` is non-null, it calls `runnerStore.loadPools` with that payload, wholesale-replacing the pool array. Because the RAF-batched flush processes events in order and each call wholesale-replaces, the last event in any batch always wins — no partial-update edge cases. The snapshot path at WS-connect is unchanged. Bundled in the same delivery is a fix for issue #32: the TopBar's `$derived` now counts `groupName` occurrences before rendering and only appends a `· <labels>` suffix when two or more pools share the same group name, so pools backed by distinct runner label sets (e.g., `ubuntu-latest` vs. `ubuntu-24.04`) display as distinct entries rather than two identically-named indicators.

## Definition of Done

1. **Backend broadcasts derived pool stats.** `SeqEvent` gains a `pool_stats_after: Option<Vec<RunnerPoolStats>>` sidecar. Successful Job events populate it with the new pool state; Run events and failed transitions leave it `None`. Pool stats are sorted by `labels` lexicographically, producing a canonical wire order clients can compare with exact equality. The backend remains the single source of truth for the derivation — no logic is re-implemented on the frontend.

2. **Frontend updates pool indicators live.** The dispatcher threads the sidecar through to `runnerStore.loadPools` so the TopBar RunnerBar reflects queue/run-count changes as jobs flow through Queued → InProgress → Completed, without needing a WS reconnect or page reload. `StateSnapshot.poolStats` still seeds initial state at WS-connect.

3. **Pool-label disambiguation fixes bojanrajkovic/atc#32.** TopBar's display `$derived` shows just `groupName` when only one pool carries that name, and appends `· <labels>` when two or more pools share a `groupName`. Null `groupName` renders as joined labels (unchanged).

4. **Documentation reflects reality.** `docs/architecture/frontend-app.md` RunnersStore paragraph is rewritten to match the actual implementation + sidecar source. `docs/architecture/backend-server.md` gains the `SeqEvent.pool_stats_after` contract (including sort order and empty-string normalization). Any affected CLAUDE.md files are audited for staleness and updated if needed.

**Out of scope:** operator capacity config (bojanrajkovic/atc#16), multi-replica state externalization (bojanrajkovic/atc#7), per-pool delta transport, incremental pool counters on the backend.

## Acceptance Criteria

### live-pool-stats.AC1: Backend broadcasts derived pool stats via SeqEvent sidecar
- **live-pool-stats.AC1.1 Success:** A successful Job event produces a `SeqEvent` whose `pool_stats_after` is `Some(vec)`, and the vector equals `store.pool_stats()` evaluated immediately after the event applies (both sorted per AC1.7).
- **live-pool-stats.AC1.2 Success:** Successive Job events for the same job (e.g., Queued → InProgress → Completed) produce sidecars that reflect post-mutation state at each step: queued count decrements, running count increments, then the pool entry is omitted from the vector when no active jobs remain for that label-set.
- **live-pool-stats.AC1.3 Success:** A Run event produces a `SeqEvent` whose `pool_stats_after` is `None`.
- **live-pool-stats.AC1.4 Success:** `GET /v1/state` returns `poolStats` computed atomically with `runs` and `jobs` under one lock acquisition, sorted per AC1.7 (regression guard for the existing snapshot contract, extended with the new sort contract).
- **live-pool-stats.AC1.5 Failure:** A Job event that returns a store transition error (e.g., Completed → Queued) results in no broadcast: no seq bump, no `SeqEvent`, no sidecar.
- **live-pool-stats.AC1.6 Success:** `SeqEvent` JSON renders `poolStatsAfter` as a `RunnerPoolStats[]` value when populated and as `null` when not; ts-rs emits the field as `poolStatsAfter: RunnerPoolStats[] | null` in `SeqEvent.ts`.
- **live-pool-stats.AC1.7 Success:** Both `pool_stats_after` (in broadcast `SeqEvent`s) and `poolStats` (in REST `StateSnapshot`s) are sorted by `labels` lexicographically. Clients can perform exact `Vec<RunnerPoolStats>` equality comparisons without re-sorting.
- **live-pool-stats.AC1.8 Success:** When a GitHub webhook payload carries `runner_group_name: ""`, `atc-github`'s translation normalizes it to `None` so the resulting `RunnerInfo.group_name` is `null`. Empty strings never reach the frontend disambiguation logic as a shared group name.

### live-pool-stats.AC2: Frontend applies sidecar and updates live
- **live-pool-stats.AC2.1 Success:** When the WS client receives a `SeqEvent` with populated `poolStatsAfter`, the dispatcher calls `runnerStore.loadPools` with exactly that payload after routing the primitive event.
- **live-pool-stats.AC2.2 Success:** When the WS client receives a `SeqEvent` with null `poolStatsAfter`, the dispatcher does not call `runnerStore.loadPools` (no accidental clear).
- **live-pool-stats.AC2.3 Success:** At WS-connect, `runnerStore.pools` is seeded from `StateSnapshot.poolStats` via the existing `runnerStore.loadPools(snapshot.poolStats)` call in `ConnectionManager` (no regression of the snapshot-seeding path).
- **live-pool-stats.AC2.4 Success:** Pre-connect buffered events with `seq >= snapshot.seq` are flushed through the dispatcher after snapshot load; their sidecars are applied, bringing `runnerStore.pools` into alignment with the latest broadcast.
- **live-pool-stats.AC2.5 Edge:** After a batched RAF flush containing multiple Job events, `runnerStore.pools` equals the last event's `poolStatsAfter` payload — the array is replaced wholesale on each populated sidecar in dispatch order.
- **live-pool-stats.AC2.6 Success:** Browser-mode unit test with the real `EventDispatcher` and `runnerStore` modules: a sequence of synthetic `SeqEvent`s dispatched via `eventDispatcher.dispatch` and processed via `eventDispatcher.flush()` results in `runnerStore.pools` equaling the last event's `poolStatsAfter` payload.

### live-pool-stats.AC3: TopBar disambiguates pool labels only when groupName is shared
- **live-pool-stats.AC3.1 Success:** Two pools with `groupName: "GitHub Actions"` and distinct label sets (`["ubuntu-latest"]` and `["ubuntu-24.04"]`) render as `"GitHub Actions · ubuntu-latest"` and `"GitHub Actions · ubuntu-24.04"`.
- **live-pool-stats.AC3.2 Success:** A single pool with a non-null `groupName` (e.g., `"GitHub Actions"`) renders as just `"GitHub Actions"` (no suffix).
- **live-pool-stats.AC3.3 Success:** A pool with `groupName: null` renders as the joined labels (e.g., `"self-hosted, linux"`) — unchanged from prior behavior.
- **live-pool-stats.AC3.4 Edge:** Three or more pools sharing a `groupName` each render with the `· <labels>` suffix.
- **live-pool-stats.AC3.5 Edge:** Mixed case — one unambiguous `"GitHub Actions"` elastic pool alongside two `"self-hosted-linux-group"` pools with distinct labels — renders the unambiguous pool without a suffix and both shared-group pools with suffixes.

### live-pool-stats.AC4: Documentation aligns with implementation
- **live-pool-stats.AC4.1 Success:** `docs/architecture/frontend-app.md` describes `RunnersStore` as holding `RunnerPoolStats[]` with `loadPools` / `clear` methods, seeded from `StateSnapshot.poolStats` at WS-connect and updated per event via the `SeqEvent.poolStatsAfter` sidecar. The "RunnerEvent" and "map of `Runner` objects" language is removed.
- **live-pool-stats.AC4.2 Success:** `docs/architecture/backend-server.md` documents `SeqEvent.pool_stats_after` as a contract: semantics (Some for Job events, None for Run events, computed under the seq mutex after apply), wire format, and interaction with the snapshot's `poolStats` field.
- **live-pool-stats.AC4.3 Success:** Each architecture doc's "Last verified: YYYY-MM-DD" date is refreshed in the same commit that changes that doc's content (`backend-server.md` in Phase 1; `frontend-app.md` in Phase 2).
- **live-pool-stats.AC4.4 Success:** CLAUDE.md files affected by Phase 1 and Phase 2 changes are audited for staleness and updated if needed (via `project-claude-librarian` or equivalent manual review).

## Glossary

- **SeqEvent**: The server-layer broadcast envelope consisting of a monotonic sequence number, a `WebhookEvent`, and (new in this design) the optional `pool_stats_after` sidecar. Defined in `atc-server/src/state.rs`.
- **sidecar**: A design pattern where auxiliary derived data is attached alongside the primary payload of a message rather than sent as a separate message. Here, the pool stats vector is the sidecar to the `WebhookEvent` inside `SeqEvent`.
- **RunnerPoolStats**: The domain type representing the instantaneous state of a runner pool — label-set, queue depth, running count, and optional total capacity. Defined in `atc-core`.
- **WebhookEvent**: The discriminated union type (`Run | Job`) produced by `atc-github` when parsing an incoming GitHub webhook. It is the primitive event that `SeqEvent` wraps.
- **JobEventEnvelope**: The domain type that carries a parsed GitHub job webhook payload before it is translated into a `WebhookEvent::Job` variant. Represents the boundary between `atc-github` parsing and `atc-server` handling.
- **StateStore**: The `atc-core` in-memory store holding runs, jobs, and derived pool state, protected by an `RwLock`. The single source of truth the backend derives pool stats from.
- **seq mutex**: A `Mutex<u64>` in `atc-server` whose critical section spans mutation + sequence assignment + broadcast, ensuring no subscriber can observe a seq number that does not correspond to a committed state change.
- **TTL eviction**: A background task in `atc-core` that removes completed jobs from the store after a configurable time-to-live. Referenced in the concurrency discussion because it can interleave with `pool_stats()` reads.
- **EventDispatcher**: The frontend module (`dispatcher.ts`) that receives `SeqEvent` messages from the WebSocket client, buffers them through a `requestAnimationFrame` flush, and routes each event to the appropriate store.
- **RAF batching**: Grouping multiple incoming events so they are all processed in a single `requestAnimationFrame` callback, preventing intermediate renders. Relevant to AC2.5, where the last event's sidecar is the authoritative final pool state.
- **RunnerStore**: The Svelte store holding `RunnerPoolStats[]` for the TopBar. Uses wholesale-replace (`loadPools`) because it has a single consumer reading the entire collection. Distinct from `RunStore`.
- **RunStore**: The Svelte store holding individual workflow run and job state. Uses `SvelteMap` for per-key reactivity because many components read specific runs by ID. Intentionally different shape from `RunnerStore`.
- **loadPools**: The `RunnerStore` method that replaces the entire `pools` array in one call. Idempotent; calling it multiple times with the same data produces the same result.
- **$state / $derived**: Svelte 5 rune declarations. `$state` declares reactive state; `$derived` declares a value computed from reactive state that re-runs automatically when dependencies change.
- **StateSnapshot**: The REST response from `GET /v1/state` and the initial WS payload, containing `runs`, `jobs`, and `poolStats` read atomically under a single lock acquisition. Seeds the frontend stores at connect time.
- **ts-rs**: A Rust crate that generates TypeScript type definitions from Rust structs at build time (via `just types`). Used to keep `SeqEvent.ts` in sync with the Rust struct.
- **ws-mock harness**: A test utility in `frontend/e2e/lib/ws-mock.ts` that intercepts the WebSocket connection in E2E tests and injects scripted events, allowing the test suite to simulate a live webhook stream without a running backend.
- **elastic pool**: A GitHub Actions runner pool whose label-set maps to a hosted runner image (e.g., `ubuntu-latest`, `ubuntu-24.04`). Multiple elastic pools sharing a `groupName` like "GitHub Actions" are the specific case that triggers the #32 disambiguation bug.
- **groupName**: A field on `RunnerPoolStats` that names the logical group a pool belongs to (e.g., `"GitHub Actions"` for hosted runners, `null` for ungrouped self-hosted runners). The TopBar uses it as the primary display label.
- **project-claude-librarian**: A project-specific sub-agent that reviews `CLAUDE.md` files for staleness after implementation phases and surfaces any needed updates.

## Architecture

Pool stats stay derived on the backend; clients never re-implement the derivation. The update path is a **sidecar field on `SeqEvent`** — each successful Job event broadcast carries the full `Vec<RunnerPoolStats>` computed immediately after the event applies. Run events leave the sidecar `None`; failed transitions produce no broadcast at all. One event, one seq, one self-consistent snapshot of state-after-this-mutation.

```rust
// backend/crates/atc-server/src/state.rs
#[derive(Serialize, Deserialize, Clone, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SeqEvent {
    pub seq: u64,
    pub event: WebhookEvent,                              // unchanged
    pub pool_stats_after: Option<Vec<RunnerPoolStats>>,   // new
}
```

`atc-core` and `atc-github` are untouched: `WebhookEvent`, `JobEventEnvelope`, and the webhook parsing layer stay pure domain/GitHub entities. The sidecar lives on the server-layer envelope where the derivation also lives.

**Broadcast flow.** The existing webhook handler already holds the seq mutex across `apply_X_event` + seq assignment + broadcast. We extend the critical section by one call: after a successful Job event applies, call `StateStore::pool_stats()` and attach the result to the emitted `SeqEvent`. `pool_stats()` acquires the store's read lock; the TTL eviction task can interleave but only removes Completed jobs (which the derivation filters out), so concurrent eviction does not affect correctness. Failed transitions skip broadcast entirely — no seq bump, no sidecar — matching the existing "clients must never receive events not reflected in the store" contract.

**Snapshot reconciliation.** `StateStore::snapshot()` and `StateSnapshot.poolStats` are unchanged. At WS-connect, the client still seeds `runnerStore.pools` from the snapshot's `poolStats`. Every subsequent `SeqEvent` with `poolStatsAfter: Some(...)` replaces the pool array wholesale via the existing `runnerStore.loadPools` method. `loadPools` is idempotent — correctness survives RAF batching because the last event's sidecar always wins.

**Frontend consumer.** `EventDispatcher.routeEvent` signature changes from `(event: WebhookEvent)` to `(seqEvent: SeqEvent)`. After routing the primitive to `runStore`, the dispatcher checks `poolStatsAfter`; if non-null, it calls `runnerStore.loadPools`. `RunnerStore` keeps its current shape (`$state<RunnerPoolStats[]>` with `loadPools` / `clear`). No store refactor, no per-key reactivity change.

**Wire-format determinism.** Both `StateStore::pool_stats()` and `StateStore::snapshot()` sort their `Vec<RunnerPoolStats>` output by `labels` lexicographically before returning. Without this, the underlying `HashMap` iteration order is arbitrary — tests would flake on exact equality and the TopBar would visually jitter as pools reshuffle between broadcasts. Sorting centralizes the contract at the two producer sites.

**Empty-`groupName` normalization.** `atc-github`'s webhook translation normalizes `runner_group_name: ""` to `None` before constructing `RunnerInfo`. Disambiguation logic on the frontend therefore never sees empty strings — an empty string upstream becomes `null` and is treated as "no group name," not as "a shared empty-group name."

**TopBar label disambiguation.** Issue #32 is fixed in the single TopBar `$derived` that maps `runnerStore.pools` into the `RunnerPoolDisplay[]` prop. Collision detection iterates the pool array once to count `groupName` occurrences; during mapping, a pool whose `groupName` is shared with another is rendered as `"<groupName> · <labels>"`, while an unambiguous or null-`groupName` pool keeps its concise form.

## Existing Patterns

Investigation found the supporting infrastructure already exists; this design is purely additive.

**Broadcast architecture** — `atc-server` already uses `tokio::sync::broadcast` (bounded, capacity 256) with a `Mutex<u64>` seq cursor held across mutation + seq assignment + broadcast (`backend/crates/atc-server/src/routes.rs:141-190`, per `backend/crates/atc-server/CLAUDE.md`). The sidecar adds one `pool_stats()` call inside the same critical section.

**Layering** — `atc-core` owns domain types (including `JobEventEnvelope`, `RunnerPoolStats`). `atc-github` parses webhooks into those envelopes; its `WebhookEvent::Run | Job` discriminated union is the wire format of the primitive stream. `atc-server` composes `SeqEvent = (seq, WebhookEvent)` and owns the broadcast and state-derivation surface. Adding `pool_stats_after` to `SeqEvent` keeps the derived sidecar at the server layer where derivation lives.

**Atomic reads** — `StateStore::snapshot()` (`backend/crates/atc-core/src/store.rs:383`) already reads `runs + jobs + pool_stats` atomically under one RwLock acquisition while holding the seq mutex. The REST snapshot contract this design inherits is unchanged.

**Store update patterns** — `RunnerStore.loadPools` (`frontend/src/lib/stores/runners.svelte.ts:6`) is wholesale replace on `$state<RunnerPoolStats[]>`. `RunStore` uses `SvelteMap` for per-key reactivity because many consumers read specific runs by id; `RunnerStore` has one consumer reading the whole collection, so array + wholesale-replace is the right shape and stays unchanged. See `feedback_cow_semantics.md` for the distinction.

**Dispatcher pattern** — `frontend/src/lib/dispatcher.ts` already buffers events through RAF and routes by `event.type`. Threading the sidecar is one signature widening plus one conditional call.

**Testing patterns** — backend uses oneshot `tower::ServiceExt` tests for route-level behavior and full-stack ephemeral listeners for WebSocket flows (`backend/crates/atc-server/CLAUDE.md`). Frontend uses Vitest projects: jsdom for component composition and reactivity, browser-mode (Playwright chromium) for computed-style assertions (`frontend/CLAUDE.md`). Tests are organized by AC for Rust (`feedback_test_organization_by_ac.md`) and kept cohesive for TypeScript (`feedback_no_split_ts_test_files.md`).

**Divergence — frontend architecture doc.** `docs/architecture/frontend-app.md:224-227` has described `RunnersStore` as "holds a map of `Runner` objects ... receives and applies `RunnerEvent` mutations" since PR #22, but no `RunnerEvent` type has ever existed in code and the store has always been a `RunnerPoolStats[]` holder. The paragraph was templated by symmetry from the `RunsStore` paragraph one line above; it describes a design that was never implemented. This plan corrects the paragraph to match reality plus the new sidecar path.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Backend — SeqEvent sidecar and broadcast emission

**Goal:** Extend `SeqEvent` with a `pool_stats_after` sidecar and populate it on successful Job events inside the existing webhook handler's critical section.

**Components:**
- `backend/crates/atc-server/src/state.rs` — extend `SeqEvent` with `pool_stats_after: Option<Vec<RunnerPoolStats>>` (ts-rs + serde derives preserved)
- `backend/crates/atc-server/src/routes.rs` — after a successful `apply_job_event`, compute `store.pool_stats().await` under the seq mutex; build `SeqEvent` with `Some(vec)` for Job events, `None` for Run events; failed transitions produce no broadcast
- `backend/crates/atc-core/src/store.rs` — both `snapshot()` and `pool_stats()` sort their `Vec<RunnerPoolStats>` output by `labels` lexicographically before returning, so broadcast sidecars and REST snapshots share a canonical order (covers AC1.7)
- `backend/crates/atc-github/src/webhook/translate.rs` — normalize empty-string `runner_group_name` to `None` during `RunnerInfo` translation so downstream `group_name` is `null`, never `""` (covers AC1.8)
- `frontend/src/lib/types/generated/SeqEvent.ts` — regenerated via `just types`
- `backend/crates/atc-server/tests/` — route-level and ephemeral-listener tests asserting sidecar shape across Run / Job / failed-transition cases, including successive-event evolution for a single job across its lifecycle (covers AC1.1–AC1.6)
- `backend/crates/atc-core/` store tests — assert sort order in `snapshot()` and `pool_stats()` outputs (covers AC1.7)
- `backend/crates/atc-github/` translate tests — assert empty-string `runner_group_name` is normalized to `None` (covers AC1.8)
- `docs/architecture/backend-server.md` — document `SeqEvent.pool_stats_after` contract (including sort order and the empty-string normalization) alongside existing broadcast semantics; refresh "Last verified"

**Dependencies:** None (existing broadcast infrastructure is in place).

**Done when:** Tests pass for `live-pool-stats.AC1.*`; `just types` freshness check passes in CI; `just test` passes; `docs/architecture/backend-server.md` describes the new contract.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Frontend — Dispatcher wiring and live pool updates

**Goal:** Thread the sidecar through the WebSocket client so pool indicators update in-session without requiring a reconnect or refresh.

**Components:**
- `frontend/src/lib/dispatcher.ts` — widen `routeEvent` signature to `(seqEvent: SeqEvent)`; after routing the primitive event, call `runnerStore.loadPools(seqEvent.poolStatsAfter)` when non-null. `processBuffer` now passes the full `SeqEvent` into `routeEvent`.
- `frontend/src/lib/dispatcher.test.ts` (extend existing) — new coverage for sidecar handling: null → `loadPools` not called; populated → called with exact payload; Run events always null; batched RAF flush with multiple Job events → last sidecar wins
- `frontend/e2e/` — end-to-end coverage via the shared `ws-mock` harness (`frontend/e2e/lib/ws-mock.ts`): simulated Job event with `poolStatsAfter` drives TopBar pool-indicator updates without reload
- `docs/architecture/frontend-app.md` — rewrite the `RunnersStore` paragraph (currently lines 224-227) to describe actual implementation: `pools: RunnerPoolStats[]` with `loadPools` / `clear`, seeded from `StateSnapshot.poolStats` at WS-connect, updated per-event via `SeqEvent.poolStatsAfter` sidecar; refresh "Last verified"

**Dependencies:** Phase 1 (`SeqEvent` shape and regenerated TypeScript types).

**Done when:** Tests pass for `live-pool-stats.AC2.*`; in a live dev session against the real webhook stream, TopBar pool indicators populate and update as CI events flow; `docs/architecture/frontend-app.md` matches implementation.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Frontend — TopBar label disambiguation (closes #32)

**Goal:** Disambiguate pool display labels only when two or more pools share a `groupName`, so distinct label-set elastic pools render distinctly.

**Components:**
- `frontend/src/lib/components/TopBar.svelte` — rewrite the `pools` `$derived` so it first counts `groupName` occurrences across `runnerStore.pools`, then for each pool emits `groupName` alone when the count is 1 (or `groupName` is null-fallback to joined labels), and `"<groupName> · <labels>"` when the count is ≥2
- `frontend/src/lib/components/TopBar.browser.test.ts` (extend existing) — new coverage for `live-pool-stats.AC3.*`: ambiguous collision (two pools sharing `groupName`), unambiguous single pool, null-`groupName` fallback to joined labels, three-way collision, mixed ambiguous + unambiguous. Colocated in the existing browser-mode suite per `feedback_no_split_ts_test_files.md`.

**Dependencies:** None (independent of the sidecar work; could run in parallel with Phase 1 or Phase 2).

**Done when:** Tests pass for `live-pool-stats.AC3.*`; in a live dev session against the ATC repo (which exercises both `ubuntu-latest` and `ubuntu-24.04` elastic pools), the TopBar renders two distinct labels rather than two identical "GitHub Actions" indicators.
<!-- END_PHASE_3 -->

## Additional Considerations

**Concurrency.** `pool_stats()` acquires the store's read lock. It is called inside the webhook handler's seq-mutex critical section, immediately after `apply_job_event` returns (releasing the store write lock). The TTL eviction task can interleave between them but only removes Completed jobs, which `pool_stats()` filters out — so concurrent eviction does not affect derivation correctness.

**Cost.** `pool_stats()` is O(jobs in store). Negligible at current scale (tens of jobs per repo during CI). Incremental counter optimization is deferred; revisit only if the store grows to thousands of active jobs or if broadcast-time CPU shows up in profiles.

**Forward compatibility with capacity config (bojanrajkovic/atc#16).** `RunnerPoolStats.total: Option<u32>` already exists. When the operator capacity config feature lands, `pool_stats()` populates `total` from config — the sidecar wire format requires no change and clients already handle the optional.

**Forward compatibility with multi-replica (bojanrajkovic/atc#7).** The sidecar design works regardless of where state lives. In a multi-replica world, derivation runs at broadcast time against whichever replica owns the mutation; externalized state turns `pool_stats()` into a read from a shared store instead of local memory, but the sidecar contract is unaffected.

**Commit and PR conventions.** Per project convention, the design plan lands on branch `feat/live-pool-stats`, not `main`. The PR title must reflect the full deliverable (both the broadcast work and the #32 fix), not just the design doc commit — e.g., `feat: broadcast live runner pool stats and disambiguate runner bar labels`. The PR body must include `Closes bojanrajkovic/atc#32` so the squash commit auto-closes the issue. The test plan goes in the first PR comment, not in the body (per `feedback_test_plans.md`). Audit affected CLAUDE.md files for staleness at the end of each implementation phase (via `project-claude-librarian` or equivalent manual review).

**Documents to Update** (per project design-plan guidance #6):

| Path | Change | Phase |
|------|--------|-------|
| `docs/architecture/backend-server.md` | Document `SeqEvent.pool_stats_after` contract and broadcast semantics. Refresh "Last verified". | Phase 1 |
| `docs/architecture/frontend-app.md` | Rewrite `RunnersStore` paragraph (lines 224-227) to describe actual implementation and the two update paths (snapshot seed + per-event sidecar). Refresh "Last verified". | Phase 2 |
| `backend/crates/atc-server/CLAUDE.md` | Audited for staleness at end of Phase 1 (via `project-claude-librarian` or equivalent review). Add a sidecar-broadcast note only if the crate-level contracts section requires one. | Phase 1 |
| `frontend/CLAUDE.md` | Audited for staleness at end of Phase 2 (via `project-claude-librarian` or equivalent review). Add a runner-store update-path note only if the Key Files section needs it. | Phase 2 |
| `backend/crates/atc-core/CLAUDE.md` | Audited for staleness at end of Phase 1 (sort-order contract added to `snapshot()` / `pool_stats()`). | Phase 1 |
| `backend/crates/atc-github/CLAUDE.md` | Audited for staleness at end of Phase 1 (empty-`runner_group_name` normalization added to translation). | Phase 1 |
