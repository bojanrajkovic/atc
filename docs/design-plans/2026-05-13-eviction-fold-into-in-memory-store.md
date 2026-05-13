---
issue: 163
slug: eviction-fold-into-in-memory-store
status: draft
---

## Context

Issue [#163](https://github.com/coderinserepeat/atc/issues/163) was filed as a precondition for [#67](https://github.com/coderinserepeat/atc/issues/67) (PG outbox retention design): "eviction task machinery should live inside the persistence store, not parallel to it." It also asked whether the stores should split into separate crates for complexity management.

Most of the literal text of #163 was already delivered by ADR-0006 (`docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md`): `InMemoryStore::start` now spawns the eviction task and holds the `JoinHandle`; `InMemoryStore::shutdown` joins it; the orchestration goes through `PersistentStore::shutdown`. What ADR-0006 explicitly left intact (§ Future work, line 198: "`eviction.rs::spawn_eviction_task` is unchanged. Its sole caller moves…") is the residual parallel module — `backend/crates/atc-server/src/persist/eviction.rs` still sits as a sibling to `persist/in_memory.rs`, exporting a free function whose only production caller is `InMemoryStore::start` (`in_memory.rs:111`).

This plan closes that residual along two axes:

1. **Fold** `spawn_eviction_task` into `InMemoryStore` as an associated function; delete the sibling module; update the two architecture docs that name the module.
2. **Resolve a stale instrumentation claim that the fold surfaces.** `atc-server/CLAUDE.md:56` claims `spawn_eviction_task` "constructs a task-lifetime root at spawn time and attaches via `.instrument(span)`" alongside `spawn_listener_task` / `spawn_drain_task` — but the actual code at `persist/eviction.rs:24-41` has no `info_span!` and no `.instrument(...)` wrap. `metrics.md` § "Span inventory" (line 333) and `backend-server.md` § Tracing (line 211) only enumerate `listener.task` / `listener.recv` / `drain.task` / `drain.pass` / `drain.broadcast` — eviction is missing entirely. The CLAUDE.md claim is wrong and the inventory has a hole.

   We resolve this in the direction the (currently false) CLAUDE.md text already pointed: add real `eviction.task` (task-lifetime root) and `eviction.sweep` (per-tick child) spans following the listener/drain reference implementation, then document them in `metrics.md` § "Span inventory" and `backend-server.md` § Tracing. Operationally this gives observability into eviction cadence and per-sweep work, matching what `listener.task` / `drain.task` already provide for the PG broadcast pipeline.

The crate-split question raised in #163 is deferred. Long-term direction is a three-crate split (`atc-persist` trait crate + `atc-store-pg` + `atc-store-mem`), but it requires relocating shared wire types (`SeqEvent`, `StateSnapshot`) out of `atc-server::state`, which is a substantially larger refactor. Bundling it with this fold would bury a ~50-line behavior-preserving change under several hundred lines of relocation noise. A follow-up issue captures that direction so it stays visible.

## Definition of Done

1. `backend/crates/atc-server/src/persist/eviction.rs` is deleted.
2. `InMemoryStore` exposes an associated function (e.g. `Self::spawn_eviction(self: Arc<Self>, interval, cancel) -> JoinHandle<()>`) that replaces the free `spawn_eviction_task`. `InMemoryStore::start` calls it.
3. The spawned future is wrapped with an `info_span!("eviction.task")` task-lifetime root via `.instrument(...)` (matching `spawn_listener_task` / `spawn_drain_task`). Each per-tick sweep emits a child `eviction.sweep` span with fields `jobs.evicted` (u64), `runs.evicted` (u64), and `elapsed.micros` (u128) — the same data already logged at `info` by `evict_expired` (`in_memory.rs:226-231`).
4. All existing eviction tests (`in_memory_store_tests.rs` lines 820-1065 and `store_lifecycle_tests.rs` lines 48-116) pass unchanged.
5. A new integration test asserts that after a `TestClock`-driven sweep, both an `eviction.task` root span and at least one `eviction.sweep` child span with the expected counts/fields are recorded in the in-memory span exporter. Test follows the `#[serial_test::serial]` + `OnceLock` exporter discipline documented in `atc-server/CLAUDE.md` § Testing.
6. `backend/crates/atc-server/CLAUDE.md` no longer references `eviction.rs` as a sibling module; the Spans bullet (line 56) reflects the now-real `eviction.task` / `eviction.sweep` instrumentation.
7. `docs/architecture/backend-server.md` (line 515) describes the eviction task as an internal implementation detail of `InMemoryStore`; the Tracing section (around line 211) adds `eviction.task` / `eviction.sweep` to the enumerated boundaries.
8. `docs/architecture/metrics.md` § "Span inventory" (line 333) adds entries for `eviction.task` and `eviction.sweep` following the existing two-column table format used for `listener.task` / `drain.task`.
9. A follow-up GitHub issue is filed (title: "refactor: extract atc-persist + atc-store-{pg,mem} crates") capturing the long-term crate-split direction with the rationale from this plan.

## Locked Decisions

The following are not open for re-evaluation during implementation. ADR-anchored decisions cite the ADR; planning-session decisions are anchored to this design plan's Context section (which is committed to `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md` in Phase 1 and becomes the file-path-citable artifact).

- **Each store owns its background-task lifecycle; eviction is store-private.** `PersistentStore::evict()` would be a no-op on `PgStore`, and #67's PG outbox retention is a different operation that shares only the "periodic sweep" shape. Each store owns its own activity cycle; eviction stays an implementation detail.
  - *Source*: `docs/architecture-decisions/0006-stores-own-background-task-lifecycle.md` § Decision (line 53), which locks "Each store's `start()` constructs the store and spawns the background tasks it owns" — this plan extends that decision by collapsing the parallel `eviction.rs` module into the store itself.
- **PG outbox retention (#67) is fully out of scope.** No stub, no placeholder. #67 remains a separate design + implementation effort with its own ADR/design plan.
  - *Source*: this design plan, Context § (axis enumeration) — set during the planning session's PG-outbox clarification.
- **No crate split in this PR.** Captured as a follow-up issue instead.
  - *Source*: this design plan, Context § final paragraph — set during the planning session's sequencing clarification.
- **The replacement is an associated function on `InMemoryStore`**, not an inline `tokio::spawn` block in `start()`.
  - *Source*: this design plan, Architecture § "The fold" — set during the planning session's inline-style clarification.
- **Eviction instrumentation lands now, not in a separate PR.** The stale CLAUDE.md claim is resolved by adding real `eviction.task` / `eviction.sweep` spans rather than by deleting the claim.
  - *Source*: this design plan, Context § axis 2 — set during the planning session's instrumentation-scope clarification.

## Architecture

### The fold

`spawn_eviction_task` today (`backend/crates/atc-server/src/persist/eviction.rs:24-41`) takes `Arc<InMemoryStore>` and returns `JoinHandle<()>`. Its sole production caller is `InMemoryStore::start` at `backend/crates/atc-server/src/persist/in_memory.rs:111`.

Post-fold, the helper becomes an associated function on `InMemoryStore`, with the previously-promised-but-missing instrumentation actually added. The reference implementations are `spawn_listener_task` (`backend/crates/atc-server/src/listener.rs:74-114`) and `spawn_drain_task` (`backend/crates/atc-server/src/listener.rs:188-…`):

```rust
impl InMemoryStore {
    fn spawn_eviction(
        self: Arc<Self>,
        interval: Duration,
        cancel: CancellationToken,
    ) -> JoinHandle<()> {
        // Construct the task-lifetime root span at spawn time. `tokio::spawn`
        // does NOT propagate the calling task's parent span — wrap the future
        // via `.instrument(span)` so per-sweep children attach here.
        let task_span = info_span!("eviction.task");
        tokio::spawn(
            async move {
                let mut ticker = tokio::time::interval(interval);
                // First tick completes immediately — consume it to align the cadence
                // so the first real sweep runs after `interval`, not at startup.
                ticker.tick().await;
                loop {
                    // `biased;` so cancellation is honored before the next tick
                    // fires — eviction never delays cooperative shutdown.
                    tokio::select! {
                        biased;
                        () = cancel.cancelled() => break,
                        _ = ticker.tick() => self.evict_expired().await,
                    }
                }
            }
            .instrument(task_span),
        )
    }
}
```

`InMemoryStore::start` changes from `super::eviction::spawn_eviction_task(Arc::clone(&store), …)` to `Arc::clone(&store).spawn_eviction(…)`. The biased-select cooperative-cancel shape from issue #60 is preserved verbatim — this is the supervision pattern; do not deviate. The first-tick consumption and cancel-first ordering comments from the deleted `eviction.rs:21-23` move into the new method as the comments shown above.

`InMemoryStore::evict_expired` (`in_memory.rs:162-232`) gains a `#[tracing::instrument(name = "eviction.sweep", skip_all, fields(jobs.evicted = tracing::field::Empty, runs.evicted = tracing::field::Empty, elapsed.micros = tracing::field::Empty))]` attribute. Inside the body, after the sweep completes (around the existing `tracing::info!` at line 226-231), call `tracing::Span::current().record("jobs.evicted", …)` / `…record("runs.evicted", …)` / `…record("elapsed.micros", …)` with the same values currently logged. The existing `info!` stays — the span fields supplement structured tracing, the log line stays human-grep-able. (This is the same pattern used in `handle_listener_notification` at `listener.rs:118-122` and `:144`.)

### Rejected alternatives

1. **Add `evict()` / `start_eviction()` to `PersistentStore` trait.** Rejected: would be a no-op on `PgStore`; the operations don't share a meaningful contract; eviction is an internal cycle, not a callable surface.
2. **Inline the `tokio::spawn` block directly into `InMemoryStore::start()`.** Rejected per user preference: the associated function keeps `start()` legible at a glance and matches the supervision pattern's "spawn helper is its own named unit" convention.
3. **Bundle the three-crate split into this PR.** Rejected: relocates shared wire types (`SeqEvent`/`StateSnapshot`), reshapes `Cargo.toml`, churns ts-rs export paths and integration-test layout. Sequenced into a follow-up issue.
4. **Stub a PG outbox-retention module** to lock in the symmetric shape. Rejected: #67 remains untouched. The pattern this PR establishes (private associated function on the store) is forward-compatible — when #67 lands, the natural shape is a `PgStore::spawn_outbox_retention` associated function called inside `PgStore::start`, regardless of how its body is implemented.
5. **Delete the stale CLAUDE.md instrumentation claim instead of fulfilling it.** Rejected: the operational value of `eviction.task` / `eviction.sweep` spans is real (eviction cadence and per-sweep work are otherwise invisible), the metrics.md span inventory has a hole that should be closed, and the listener/drain reference implementations are ~10 lines each to mirror.

### Forward-compatibility with #67

When #67 implements PG outbox retention, it will follow the placement pattern established here: a private associated function on `PgStore` called inside `PgStore::start`, with the `JoinHandle` joined by `PgStore::shutdown`. The body of #67's task — supervision shape, coordination mechanism (advisory lock, watermark-coordinated DELETE, partition rotation), span structure — is its own design problem and is not constrained by this plan. The supervision shape used in `spawn_eviction` (biased select + ticker) is the right tool for a single-replica TTL sweep over in-memory state; PG outbox retention may need different scaffolding (e.g., advisory-lock-gated coordination across replicas), and this plan makes no claim about that.

## Implementation Phases

This plan mixes a behavior-preserving refactor (the fold) with new observable behavior (eviction spans). The fold leans on existing tests as the contract; the new spans get TDD treatment — write the failing span-assertion test first, then add the instrumentation to make it pass.

### Phase 1 — Branch + plan commit

Create the feature branch `refactor/issue-163-eviction-fold` from `main`. Copy this plan from `~/.claude/plans/let-s-plan-to-address-cryptic-cocke.md` to `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md`. Commit on the branch as `docs: add design plan for issue 163`.

### Phase 2 — Establish baseline on the feature branch

After Phase 1 (now on the feature branch, plan commit landed, no source code edits yet), run `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server -E 'test(in_memory_store_tests) | test(store_lifecycle_tests)'`. Purpose: distinguish pre-existing red (if any) from regressions introduced by Phase 3/4. The fold must not change the outcome of these tests.

### Phase 3 — Write the span-assertion test (red)

Add a new integration test, e.g. `backend/crates/atc-server/tests/integration/eviction_spans_test.rs` (or extend `in_memory_store_tests.rs` with a new module — match the existing organization conventions you find in the directory). The test:

1. Marked `#[serial_test::serial]` (per `atc-server/CLAUDE.md` § Testing — the `InMemorySpanExporter` is process-global).
2. Initializes the OTel harness in `tests/integration/common/mod.rs` if not already.
3. Builds an `InMemoryStore` with a `TestClock`, applies enough events to seat a completed job past TTL.
4. Advances the `TestClock` past TTL and calls `store.evict_expired().await` directly (the spawned task isn't required to exercise the span — the `#[instrument]` attribute on `evict_expired` fires on each direct call too; the test then separately verifies `spawn_eviction` produces a root span by collecting span exports under a short `tokio::time::pause`-driven tick).
5. Force-flushes the meter/tracer provider and reads finished spans.
6. Asserts: at least one span with name `eviction.task` (root) exists; at least one span with name `eviction.sweep` exists with attributes `jobs.evicted`, `runs.evicted`, `elapsed.micros` set to values consistent with the seed.

Run the test — it MUST fail on red (`eviction.task` / `eviction.sweep` don't exist yet).

### Phase 4 — Fold + instrument (green)

1. Add `spawn_eviction` as an associated function on `InMemoryStore` in `backend/crates/atc-server/src/persist/in_memory.rs`, including the `info_span!("eviction.task")` + `.instrument(...)` wrapping shown in Architecture § "The fold". Carry forward the first-tick and cancel-first rationale comments from the deleted `eviction.rs:21-23` into the new method.
2. Add `#[tracing::instrument(name = "eviction.sweep", skip_all, fields(jobs.evicted = tracing::field::Empty, runs.evicted = tracing::field::Empty, elapsed.micros = tracing::field::Empty))]` to `InMemoryStore::evict_expired` (`in_memory.rs:162`). After the sweep work completes, record the three fields on `tracing::Span::current()` using the same values currently passed to the `tracing::info!` at line 226-231.
3. Update `InMemoryStore::start` (`in_memory.rs:111`) to call `Arc::clone(&store).spawn_eviction(eviction_period, shutdown)` instead of `super::eviction::spawn_eviction_task(...)`.
4. Delete `backend/crates/atc-server/src/persist/eviction.rs`.
5. Remove `pub mod eviction;` from `backend/crates/atc-server/src/persist/mod.rs:21`.
6. Compile-check: `cd /Users/brajkovic/Projects/atc/backend && cargo check -p atc-server`.
7. Run the Phase 3 test — it MUST now pass.
8. Run the Phase 2 baseline nextest filter — still must pass.

### Phase 5 — Doc updates

1. `backend/crates/atc-server/CLAUDE.md`: in the `persist` row of the modules table (line 31), remove the parenthetical describing `eviction.rs` and roll its substance into the `in_memory.rs` parenthetical (note the eviction task is now an `InMemoryStore` associated function, not a sibling module). In the "Spans" bullet (line 56), rewrite the `spawn_eviction_task` reference to `InMemoryStore::spawn_eviction`; the existing claim that it constructs a task-lifetime root span and attaches via `.instrument(span)` now becomes true. Add `eviction.task` and `eviction.sweep` to the span-names roll-call earlier in the same bullet (alongside `persist.apply.run_event` etc.). Refresh "Last verified" date.
2. `docs/architecture/backend-server.md`: at line 515, drop the "`eviction` sub-module exporting `spawn_eviction_task`…" trailing clause. Replace with a sentence noting that `InMemoryStore::spawn_eviction` is the in-memory-mode background sweep, owned by the store. In the Tracing section (around line 211, where `listener.task` / `drain.task` / etc. are enumerated), add `eviction.task` and `eviction.sweep` to the boundary list. Refresh "Last verified" date.
3. `docs/architecture/metrics.md` § "Span inventory" (line 333+): add two rows to the table for `eviction.task` and `eviction.sweep`, following the two-column format used by `listener.task` and `drain.task`. For `eviction.task`: source pointer `backend/crates/atc-server/src/persist/in_memory.rs` (`InMemoryStore::spawn_eviction`, spawned from `InMemoryStore::start` per ADR-0006); attributes "none (long-lived)". For `eviction.sweep`: source pointer `InMemoryStore::evict_expired`, per-tick child of `eviction.task`; attributes `jobs.evicted` (u64), `runs.evicted` (u64), `elapsed.micros` (u128). Also update line 53-54 (the prose roll-call before the inventory table) to add `eviction.task`, `eviction.sweep` alongside `listener.task` etc.
4. No ADR edits. ADR-0006 is historical; its mention of `eviction.rs::spawn_eviction_task` (lines 19, 198) accurately reflects the world at the time of that decision. Do not retrofit ADRs to match current code.
5. No `scripts/doc-mapping.sh` edits needed: `eviction.rs` was already covered by the catch-all `backend/crates/atc-server/src/*` → `backend-server.md` mapping (line 58); after deletion the glob still routes the remaining files correctly. `in_memory.rs` changes also route to `backend-server.md`. Note: `in_memory.rs` is NOT in the dual-map list at line 39 today, but adding metrics.md to its mapping is unnecessary because the doc-staleness gate triggers on the SOURCE file being edited and the metrics.md update lands in the same commit anyway.

### Phase 6 — Lint, test, doc-staleness gate

```bash
cd /Users/brajkovic/Projects/atc/backend && cargo clippy -p atc-server -- -D warnings
cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server
cd /Users/brajkovic/Projects/atc && just lint
```

The pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) should be satisfied by the Phase 5 doc edits.

### Phase 7 — File the crate-split follow-up issue

```bash
gh issue create \
  --title "refactor: extract atc-persist + atc-store-{pg,mem} crates" \
  --body-file /tmp/claude-<session-id>/issue-body.md
```

The issue body should capture the long-term direction articulated in this plan's Context section: three-crate split (trait crate + two store crates), with the precondition that `SeqEvent` / `StateSnapshot` migrate out of `atc-server::state` (target: `atc-core` or a thin shared wire-types crate). Cite this design plan and #163 as the rationale source.

### Phase 8 — PR

Open a PR against `main`. PR title: `refactor: fold eviction task into InMemoryStore`. Body summarizes the fold + the now-real eviction instrumentation, links the follow-up issue. Closes #163.

## Acceptance Criteria

- **AC1.** `cd /Users/brajkovic/Projects/atc && git ls-files backend/crates/atc-server/src/persist/ | grep -c '/eviction\.rs$'` returns `0`.
- **AC2 (current-state cleanup).** All three of these commands return zero matches (run from `/Users/brajkovic/Projects/atc`):
  - `git grep -nE 'spawn_eviction_task|persist::eviction|persist/eviction' -- 'backend/'`
  - `git grep -nE 'spawn_eviction_task|persist::eviction|persist/eviction' -- 'docs/architecture/'`
  - `git grep -nE 'spawn_eviction_task|persist::eviction|persist/eviction' -- 'backend/crates/atc-server/CLAUDE.md'`
  This is a strict zero-hit assertion across every current-state location the refactor touches.
- **AC2b (historical exemption — informational).** `git grep -nE 'spawn_eviction_task|persist::eviction|persist/eviction' -- 'docs/architecture-decisions/' 'docs/design-plans/'` is expected to return non-zero matches and is NOT a failure mode. ADR-0006 and prior design plans are historical artifacts that must remain unchanged.
- **AC3.** `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server` passes with no edits to existing tests. The new span-assertion test (Phase 3) is the only test added.
- **AC4.** `cd /Users/brajkovic/Projects/atc/backend && cargo clippy -p atc-server -- -D warnings` is clean.
- **AC5.** Existing eviction behavior is preserved end-to-end: a `TestClock`-driven completed job past TTL is removed from `InMemoryStore` (covered by existing `completed_job_past_ttl_evicted` at `in_memory_store_tests.rs:870`); the graceful shutdown still joins the eviction task within `SHUTDOWN_TIMEOUT_EVICTION` (covered by existing `in_memory_start_subscribe_observes_apply` at `store_lifecycle_tests.rs:48`).
- **AC6.** The new Phase 3 integration test passes: an `eviction.task` root span is recorded, and at least one `eviction.sweep` child span carries `jobs.evicted`, `runs.evicted`, and `elapsed.micros` fields with values consistent with the TestClock-driven seed.
- **AC7.** `gh issue list --search 'extract atc-persist'` returns an open issue referencing this design plan.
- **AC8.** The pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) succeeds when the implementation branch is pushed.

**Failure cases for the key ACs:**

- AC1 fails if the file is moved/renamed instead of deleted, or if a new `eviction.rs` is created elsewhere in `persist/`.
- AC2 fails if any source code, current-state architecture doc, or domain `CLAUDE.md` still references the free-function name or sibling-module path. (ADRs and design-plans are explicitly exempt via AC2b.)
- AC3 fails if behavior changed: e.g., if the biased-select cancel-first ordering was inverted, eviction would race with shutdown and lifecycle tests would deadlock-on-shutdown or surface lagged broadcasts.
- AC6 fails if `eviction.task` is omitted, if `eviction.sweep` is emitted without the field set, or if the per-sweep span attaches as a fresh root (indicates `.instrument(...)` was dropped on the `spawn_eviction` future).

## Documents to Update

| File | Change |
|------|--------|
| `backend/crates/atc-server/src/persist/in_memory.rs` | Add `InMemoryStore::spawn_eviction` associated function (with `info_span!("eviction.task")` + `.instrument(...)`); add `#[tracing::instrument(name = "eviction.sweep", …)]` to `evict_expired`; record sweep fields; update `start()` call site |
| `backend/crates/atc-server/src/persist/mod.rs` | Remove `pub mod eviction;` line 21 |
| `backend/crates/atc-server/src/persist/eviction.rs` | **DELETE** |
| `backend/crates/atc-server/tests/integration/eviction_spans_test.rs` (or extension to an existing file) | **NEW** — span-assertion test (Phase 3 / AC6) |
| `backend/crates/atc-server/CLAUDE.md` | Update modules table `persist` row to drop `eviction.rs` reference; rewrite "Spans" bullet to reflect the now-real `eviction.task` / `eviction.sweep` instrumentation (`spawn_eviction_task` → `InMemoryStore::spawn_eviction`; add `eviction.task` / `eviction.sweep` to the span-name roll-call); refresh "Last verified" |
| `docs/architecture/backend-server.md` | Drop `eviction` sub-module clause in line 515; describe eviction as InMemoryStore implementation detail; add `eviction.task` and `eviction.sweep` to the Tracing-section boundary list (around line 211); refresh "Last verified" |
| `docs/architecture/metrics.md` | Add `eviction.task` and `eviction.sweep` rows to § "Span inventory" (around line 366 onward, following `listener.task` / `drain.task` format); add the two span names to the prose roll-call at lines 53-54; refresh "Last verified" if the doc carries one |
| `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md` | **NEW** — copy of this plan, committed in Phase 1 |
| GitHub issue (new) | "refactor: extract atc-persist + atc-store-{pg,mem} crates" filed in Phase 7 |

ADRs (`0005`, `0006`) are **NOT** updated. They are historical decision records and accurately describe the state at the time of authorship.

Prior design plans in `docs/design-plans/` are **NOT** updated for the same reason — they are point-in-time artifacts.

`scripts/doc-mapping.sh` is **NOT** updated. The catch-all `backend/crates/atc-server/src/*` glob already routes `in_memory.rs` edits to `backend-server.md`; the metrics.md update lands in the same commit anyway. Adding `in_memory.rs` to the dual-map list at line 39 would be a forward-compat improvement but is out of scope for this PR.

## Out of Scope

- **PG outbox retention (#67).** No code, no stubs, no module shells. Tracked separately.
- **Three-crate split (`atc-persist` + `atc-store-pg` + `atc-store-mem`).** Tracked by the follow-up issue filed in Phase 7. Requires relocating `SeqEvent`/`StateSnapshot` and rewiring ts-rs export paths — a multi-PR effort that warrants its own design plan.
- **Generalizing eviction into a trait method.** Locked Decisions § "Each store owns its background-task lifecycle; eviction is store-private."
- **ADR retrofits.** Historical decision records stay as written.
- **Renaming `evict_expired` or its sub-operations.** No API renames; the fold is purely about module/visibility relocation.
- **Adding `in_memory.rs` to the `doc-mapping.sh` dual-map list at line 39.** Forward-compat improvement that would benefit future PRs which edit `in_memory.rs` without touching `metrics.md`, but not required by this PR (the metrics.md update is in the same commit).

## Plan Review Gates

Per `docs/planning-workflow.md` § 6:

- **Self-consistency check.** AC2 is a strict zero-hit assertion across `backend/`, `docs/architecture/`, and `backend/crates/atc-server/CLAUDE.md`. AC2b is a separate informational sweep across `docs/architecture-decisions/` and `docs/design-plans/` that is EXPECTED to return hits — those are historical artifacts. The plan file itself (this document, post-Phase-1 commit at `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md`) contains the literal string `spawn_eviction_task` in code samples and AC text; AC2 explicitly excludes `docs/design-plans/` from its zero-hit scope so this self-reference is not self-defeating.
- **External codex review.** Completed before exiting plan mode. Initial review identified two blockers (AC2 scope/consistency and the stale `.instrument(span)` claim) and three important concerns (Phase 2 wording, Locked Decisions citations, #67 forward-compat overspecification). All five resolved in this revision. The instrumentation blocker is resolved by adding the eviction spans rather than deleting the stale claim — operational visibility for eviction cadence and per-sweep work was deemed worth the scope expansion. The crate-split deferral remains the answer to issue #163's second prompt; follow-up issue captured in Phase 7.
