# ADR 0009 — Display TTL vs data retention

Date: 2026-05-22
Status: Accepted

## Context

`GET /v1/state` returns every non-evicted run and job from the active `PersistentStore`. On the PG path the row count grows monotonically — the outbox sweep (ADR 0007) trims the durable broadcast log, but `runs` and `jobs` rows persist indefinitely. As the dashboard runs for days the snapshot payload grows unboundedly, and completed work that nobody is looking at anymore continues to occupy kanban "Completed" columns. The in-memory store has had `is_evictable` (ADR 0006 area) since launch, but that is a memory-bound for dev mode, not a UI visibility decision.

We need a way to hide aged completed work from the UI without committing to a data-row eviction policy. Display lifetime and data lifetime are distinct concerns:

- **Display TTL** — how long completed work appears in the kanban / flat job list. Operator-relevant for "I don't need to see yesterday's deploys"; UX-relevant for keeping the surface scannable.
- **Data retention** — how long the underlying row exists. Operator-relevant for compliance, debugging, audit; out of scope for this work.

Conflating them — say, deleting rows past 1 h — would force every operator who wants a 1 h kanban view to also accept losing their compliance log.

## Decision

Introduce a **display-layer gate**: completed runs and jobs whose terminal timestamp is older than a configured TTL are filtered out of `/v1/state` (server-side SQL `WHERE`) and aged out of the live UI (client-side `$derived` against `uiStore.nowMs`). The underlying rows remain in place. Data-row retention is a future and orthogonal decision.

### Mechanism

1. **Domain model.** `WorkflowRun` gains `completed_at: Option<DateTime<Utc>>`, populated on the `Completed` transition by `apply_run_event` with preserve-first semantics (`envelope.completed_at.or(existing.completed_at)`). The run FSM is forward-only, so once a `Some(t)` is recorded, a later replay cannot move it backward. The parallel field already exists on `Job`.

2. **GitHub translation.** GitHub's `workflow_run` payload has no dedicated `completed_at` (unlike `workflow_job`); when `action == "completed"`, `workflow_run.updated_at` is GitHub's record of the row's last write, which is the run's completion moment by contract. The translation layer treats `updated_at` as the best-available proxy for non-completed actions, `completed_at` stays `None`.

3. **PG schema.** A new migration adds `runs.completed_at TIMESTAMPTZ NULL` and a composite `(status, completed_at)` index parallel to `jobs_status_completed_at_idx`. Existing `Completed` rows are backfilled once from `updated_at`.

4. **Trait surface.** `PersistentStore::read_snapshot` accepts a `cutoff: Option<DateTime<Utc>>`. `None` disables filtering (test-only). The PG `WHERE` clause and the in-memory pre-collect filter share the same shape:

   ```sql
   cutoff IS NULL OR status != 'Completed' OR completed_at IS NULL OR completed_at >= cutoff
   ```

   `completed_at IS NULL` is permissive so a row whose backfill has not yet landed, or whose first event has not yet arrived, stays visible until the next event populates it.

5. **Cutoff composition.** `routes::state_handler` reads `AppState.clock` and `AppState.display_ttl` (both wired from `Config` at startup), computes `cutoff = clock.now() - display_ttl`, and passes it to `read_snapshot`. The store does not know the configured TTL — that violates the "trait owns event-derived state, not config" invariant (ADR 0008).

6. **Wire surface.** `StateSnapshot` gains `display_ttl_seconds: u32` with `#[serde(default)]`. A pre-feature replica during a rolling deploy emits no field; the frontend treats `0` as "no filter armed" so the rollout window is non-disruptive. The frontend uses this value to drive a `$derived` filter on `RunStore.completedRuns` and the flat `jobs` view against `uiStore.nowMs`, so completed rows age out reactively without an event arriving.

7. **Frontend predicate.** Three escape hatches keep a row visible: `displayTtlSeconds === 0`, status not `Completed`, or `completedAt` missing/`null`. Only when all three miss do we compare `now - completedAt > ttl` (strict `>`, matching the server SQL's `completed_at >= cutoff`).

### Locked design decisions

1. **Two-layer filter (server + client), not server-only.** Server filtering bounds the snapshot payload at request time; client filtering keeps the displayed state reactive between snapshot fetches. Without the client filter, a stale tab against a 1 h TTL would keep showing aging-out runs until the next reconnect re-fetched `/v1/state`. Without the server filter, the snapshot payload grows unbounded.

2. **WS event stream is not server-filtered.** Live events for an aged-out completed row stay in the broadcast stream — the client side's reactive filter handles them. Server-filtering the WS stream would require keeping the cutoff state per-subscriber and re-evaluating on every nowMs tick, which is more machinery than the dev-UX gap justifies.

3. **Restart-only, no hot reload of `ATC_DISPLAY_TTL`.** ATC already hot-reloads `runner_pools` via the config-file watcher. `display_ttl` does *not* ride that channel in v1. Rationale: restart-only matches `outbox_retention`'s treatment, keeps v1 surface narrow, and avoids a second source of truth on the wire (snapshot + WS frame). Operators who edit the live file get a `ScalarSnapshot::diff` warn-log telling them to roll the pod; the value applies on the next pod roll. Promoting `display_ttl` to hot-reloadable is a one-frame, one-store-slot change if dev UX warrants it later.

4. **60 s startup floor.** Below 60 s the display gate would hide rows mid-view as `uiStore.nowMs` advances, which is hostile UX. The floor is enforced at `Config::load` rather than inside `PgStore::start_inner` (where `OUTBOX_RETENTION_FLOOR` lives) because display TTL applies in both PG and in-memory modes, so server-level validation is the only place that catches both.

5. **Default 1 h.** Long enough that operators glancing at the kanban between meetings see their last hour of activity; short enough to keep the surface scannable without configuration. Independent of `ATC_OUTBOX_RETENTION` (default 7 d) — display TTL is UX, outbox retention is durability.

6. **In-memory mode contract is deliberately narrowed.** `InMemoryStore` keeps its existing hardcoded 1 h completed-eviction TTL. For `ATC_DISPLAY_TTL <= 1h` the two are aligned; for `ATC_DISPLAY_TTL > 1h`, eviction wins — rows older than 1 h have been removed from the HashMap and cannot be surfaced. This is acceptable because in-memory mode is dev-only (single-replica, lossy on restart). Linking the eviction TTL to `display_ttl` (e.g., `max(default_completed_ttl, display_ttl)`) is a small follow-up if dev UX needs longer retention.

7. **Migration locks are deployment-posture-dependent.** The migration runs three lock-acquiring statements in a single transaction: `ALTER TABLE ADD COLUMN NULL` (brief `AccessExclusiveLock`), `UPDATE … WHERE status='Completed'` (`RowExclusiveLock` for the touched rows), and `CREATE INDEX` (non-concurrent — `ShareLock` blocking writes during the build). For this repo's homelab posture (small `runs` table, brief redeploy window) the lock duration is sub-second and acceptable. For larger deployments the rollout-safe alternative is three separate migrations (`ALTER TABLE` alone, batched non-transactional backfill, `CREATE INDEX CONCURRENTLY` outside a transaction). The repo's CI gate verifies column contents, not lock duration.

8. **Clock model: independent server and client clocks.** Each side reads its own wall clock and applies the shared `display_ttl_seconds`. The inconsistency band at the boundary equals the wall-clock skew between server and browser — sub-second under NTP, dwarfed by typical TTL values. No alignment effort is made. The predicate-parity unit tests (run on both sides for the same `(now, completed_at, ttl)` tuple) guarantee the only remaining failure mode is wall-clock skew.

## Consequences

### Positive

- **Snapshot payload bounded by the configured TTL window.** With the default 1 h TTL, `/v1/state` carries at most the last hour of completed work plus all active runs and jobs — predictable for the frontend's first render.
- **Display gating decoupled from data retention.** Operators can keep 30 days of audit trail in `runs` / `jobs` and still get a 1 h kanban view; a future data-row retention policy can land without touching the display layer.
- **Frontend reactivity without polling.** The existing `uiStore.nowMs` ticker (one shared `setInterval`, 1 s cadence) drives the filter — no per-card timers, no fetch on every tick, and reconnects re-pick up the latest configured TTL from the snapshot.
- **Two-predicate parity is testable.** Server and client filters operate on the same `(now, completed_at, ttl)` inputs; the borderline-tuples unit test on both sides catches drift before it ships.

### Negative

- **One more env var to document.** `ATC_DISPLAY_TTL` joins `ATC_OUTBOX_RETENTION` as a humantime-tunable, restart-only knob. Helm chart values, deployment runbook, and the metrics doc each grow one entry.
- **In-memory mode UX is asymmetric above 1 h.** Anyone running `just dev` with `ATC_DISPLAY_TTL=4h` will see completed rows disappear at 1 h (eviction) rather than 4 h (their setting). Documented as DoD 8 of the design plan and again in `atc-store-mem/CLAUDE.md`. A follow-up that links the two values is small if it becomes operator-visible.
- **WS event stream still carries aged-out events.** A client that has been connected for hours and just received a `Completed` event for a run completed an hour earlier will store the row in its in-memory map (briefly visible before the next `nowMs` tick re-evaluates the deriver and removes it). The reactivity model keeps this from being user-visible past one second; the alternative — server-side WS filtering — was rejected as disproportionate machinery.
- **Mixed-version snapshots during a rolling deploy.** A pre-feature replica's snapshot lacks `displayTtlSeconds` and may lack `completedAt` on individual rows. The frontend null-safety covers both cases (`undefined` and `null` keep the row visible), so the rollout window is non-disruptive but operators temporarily see ungated completed work until every replica has rolled.

### Future work captured separately

- **Hot-reload of `ATC_DISPLAY_TTL`.** Add a `ConfigEvent::DisplayTtl(u32)` variant on the existing config-events broadcast channel; have the WS handler forward it as a `WireFrame`; have the frontend reducer update `RunStore.displayTtlSeconds` on receipt. Same PR removes `display_ttl` from `ScalarSnapshot`. ~50 LOC, no schema change.
- **PG-mode data-row eviction.** Independent retention policy for `runs` and `jobs` rows. Almost certainly partition-by-time + drop-old-partition rather than DELETE-by-`completed_at`, given the scale considerations in ADR 0007.
- **Link in-memory `completed_ttl` to `display_ttl`.** `completed_ttl = max(default_completed_ttl, display_ttl)` so dev sessions with a long TTL don't see eviction-driven UX gaps.
