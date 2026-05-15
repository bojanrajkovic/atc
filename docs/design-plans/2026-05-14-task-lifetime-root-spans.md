# Drop task-lifetime root spans on listener and drain tasks

Issue: [#170](https://github.com/bojanrajkovic/atc/issues/170) (refactor)
Final design-plan path on feature branch: `docs/design-plans/2026-05-14-task-lifetime-root-spans.md`

## Context

PR #163 folded eviction into the persistence stores and, during implementation review, recognised that the originally-planned `eviction.task` task-lifetime root span was an OTel anti-pattern. The eviction-side spans were reshaped so each sweep emits as its own root span (`eviction.sweep`) with no task-lifetime parent. The postscript at `docs/design-plans/2026-05-13-eviction-fold-into-in-memory-store.md:7` captures the rationale verbatim and flagged the symmetric problem on `listener.task` and `drain.task` as a follow-up — that follow-up is #170.

Today, `backend/crates/atc-server/src/listener.rs:86` wraps the spawned listener future with `info_span!("listener.task")` via `.instrument(...)`, and `listener.rs:203` does the same with `info_span!("drain.task")`. Both spans persist for the lifetime of the spawned task — i.e., until process shutdown — which has four operational consequences spelled out in the issue: SDK memory retention, one trace ID per process, loss on SIGKILL/OOM, and misleading parent linkage. Per-tick child spans (`listener.recv`, `drain.pass`, `drain.broadcast`) already use `#[tracing::instrument]` directly and will become independent roots cleanly once the wrappers are removed.

This change is the symmetric application of the #163 decision: drop the task-lifetime wrappers, let per-tick spans emit as roots, preserve `drain.broadcast` as a child of `drain.pass` (single function invocation), and update the three architecture docs that name the old span shape.

## Definition of Done

1. `listener.task` and `drain.task` are no longer constructed anywhere in `backend/crates/atc-server/`.
2. `listener.recv` emits as a root span per NOTIFY received.
3. `drain.pass` emits as a root span per pass; `drain.broadcast` remains a child of `drain.pass` (preserved by the `.in_scope()` nesting at `listener.rs:512-530`).
4. The drain-side span test inverts: `drain.pass` has no parent; `drain.broadcast` parent is `drain.pass`. A listener-side test asserts `listener.recv` is a root.
5. `backend/crates/atc-server/CLAUDE.md`, `docs/architecture/metrics.md`, and `docs/architecture/backend-server.md` no longer reference the removed span names; `metrics.md` carries a short anti-pattern explanation for future authors.
6. `just test`, `just lint`, and the pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) all pass on the feature branch.

## Locked Decisions

- **`listener.recv` becomes a root span**, symmetric with `drain.pass` and `eviction.sweep`. *Source:* user clarification, 2026-05-14 planning session.
- **No ADR is authored.** The rationale lives in the 2026-05-13 design-plan postscript and is expanded inline in `docs/architecture/metrics.md`. *Source:* user clarification, 2026-05-14 planning session.
- **`drain.broadcast` remains a child of `drain.pass`** via the existing `broadcast_span.in_scope(...)` at `listener.rs:512-530`. *Source:* Issue #170 — "drain.pass would still parent drain.broadcast because it spans a single function invocation."
- **Code + tests + docs land in a single PR.** *Source:* `scripts/doc-mapping.sh:39-46` maps `listener.rs` to both `backend-server.md` and `metrics.md`; the pre-push gate blocks otherwise.
- **Doc-staleness gate is the source of truth** for which architecture docs must change. No new `doc-mapping.sh` entries are added. *Source:* `scripts/doc-mapping.sh:39-46`.

## Architecture

### Current shape (anti-pattern)

In `backend/crates/atc-server/src/listener.rs`:
- Line 86: `let task_span = info_span!("listener.task"); tokio::spawn(async move { ... }.instrument(task_span));`
- Line 203: `let task_span = info_span!("drain.task"); tokio::spawn(async move { ... }.instrument(task_span));`

`listener.task` and `drain.task` carry no attributes. They exist solely as containers. Children (`listener.recv` at `listener.rs:118-122`, `drain.pass` at `listener.rs:404-411`, `drain.broadcast` at `listener.rs:512-530`) carry the operational attributes.

### Target shape (mirrors `eviction.sweep`)

Reference: `backend/crates/atc-server/src/persist/in_memory.rs:198-206` — `#[tracing::instrument(name = "eviction.sweep", ...)] pub async fn evict_expired(&self)`. The async fn is decorated directly; the spawn site is unadorned. Each call emits a root span on completion.

The listener/drain change is structurally smaller because `listener.recv` and `drain.pass` *already* use `#[tracing::instrument]` on the handler function. Removing the `.instrument(task_span)` wrapper at the spawn site is sufficient — the per-tick attribute fans and `Span::current().record(...)` calls inside the handlers are unchanged. `drain.broadcast` remains nested under `drain.pass` because the `.in_scope()` block executes inside the `drain.pass`-instrumented function and therefore inherits its `Span::current()` parent.

### Rejected alternatives (briefly)

- **Replace task-lifetime span with a finite-lifetime span at spawn time**: there is no meaningful unit of work at the spawn boundary to span over; the per-handler `#[instrument]` already provides per-tick roots.
- **Keep the wrapper but configure the SDK to flush in-flight spans periodically**: the OTel Rust exporter only emits a span on close, and forcing partial export would diverge from the framework's invariant. This is an SDK-internal fight, not an instrumentation fix.

## Implementation Phases

Phases are TDD-ordered. Each is a checkpoint, not a padding target. The implementation context reads this file before writing code and follows `docs/implementation-guidance.md`.

### Phase 1 — Branch and plan commit

- Create branch `refactor/task-lifetime-root-spans` from `main`.
- Copy this plan from `~/.claude/plans/docs-planning-workflow-md-take-a-look-crispy-snowglobe.md` to `docs/design-plans/2026-05-14-task-lifetime-root-spans.md`.
- Commit on the feature branch with `docs: add design plan for task-lifetime root span removal` (Conventional Commits per `CONTRIBUTING.md` § Commit Conventions).
- Run `just setup` in the new worktree if applicable (lefthook is per-worktree per CLAUDE.md invariants).

### Phase 2 — Write failing tests

In `backend/crates/atc-server/tests/integration/tracing_webhook_spans_test.rs:130-220`:

- Rename the test `drain_pass_span_is_child_of_drain_task` to `drain_pass_is_root_with_drain_broadcast_child`.
- Delete the assertions that `drain.pass.parent_span_id` is non-zero (was line 176–179).
- Delete the assertions that `drain.task` exists as a root and shares `trace_id` with `drain.pass` (was line 210–220).
- Add an assertion that `drain.pass.parent_span_id` is `None`/zero (root) using the inline pattern shown below.
- Keep the existing assertions that `drain.broadcast.parent` is `drain.pass` (was line 188–195) — this relationship is preserved.

Add `listener_recv_is_root` to **`tracing_webhook_spans_test.rs`** (do NOT add a sibling file — `tests/integration/main.rs:14-45` enumerates the test module list, so a new sibling needs a module-list edit and a separate cargo-nextest filter, which the Phase 2 RED-step command would miss). Use the inline root-span assertion pattern from `tests/integration/eviction_spans_test.rs:202-208`:

```rust
assert!(
    span.parent_span_id.to_bytes().iter().all(|b| *b == 0),
    "<descriptive failure message>"
);
```

Apply the same pattern to the renamed `drain_pass_is_root_with_drain_broadcast_child` test for the `drain.pass` root-ness assertion. There is no shared root-span helper to reuse — the inline `parent_span_id.to_bytes()` check is the project convention.

Update the module docstring at `tests/integration/eviction_spans_test.rs:1-8` in lockstep: today it says "Unlike `listener.task` / `drain.task`, eviction deliberately has no task-lifetime parent…" — after this change that comparison is stale. Rewrite it to describe per-tick roots as the shared convention with `listener.recv` and `drain.pass`. This edit is required for AC2 to pass; see Documents to Update.

Run `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server tracing_webhook_spans_test`. Confirm the new/renamed tests fail against unmodified `listener.rs`. This is the RED step.

### Phase 3 — Drop the wrappers

In `backend/crates/atc-server/src/listener.rs`:

- Around line 86: delete `let task_span = info_span!("listener.task");` and change `tokio::spawn(async move { ... }.instrument(task_span))` to `tokio::spawn(async move { ... })`. Remove the now-unused `tracing::Instrument` import if it is no longer referenced elsewhere in the file (verify with grep before deleting).
- Around line 203: same treatment for `drain.task`.

Run `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server`. Confirm the previously-failing tests now pass and no others broke. This is the GREEN step.

### Phase 4 — Update domain CLAUDE.md

Edit `backend/crates/atc-server/CLAUDE.md:57` — the span inventory roll-call. Remove `listener.task` and `drain.task` from the inventory; keep `listener.recv`, `drain.pass`, `drain.broadcast`. Update any surrounding prose that describes the `.instrument(task_span)` pattern. Update the file's `Last verified:` date.

### Phase 5 — Update metrics.md

Edit `docs/architecture/metrics.md`. The investigator reported references at lines 60–61, 78, 124–133, 232–237, 247–305, 312, 322, 332–342, 390–399. Treat the table at 390–399 as the authoritative span inventory:

- Delete the `listener.task` and `drain.task` rows from the span-inventory table.
- Remove or rephrase prose sections that describe the `.instrument(task_span)` wrapping pattern.
- Add a short subsection (≤ 8 lines) titled "Task-lifetime root spans are an anti-pattern" explaining: (a) `tracing-opentelemetry` only exports a span on close; (b) a span attached via `.instrument()` to a `tokio::spawn`-ed future closes only when the task ends; (c) for long-lived tasks this means the span never exports under normal operation and is lost entirely on SIGKILL/OOM; (d) per-tick `#[tracing::instrument]` on the handler function is the established alternative. Cite `eviction.sweep`, `listener.recv`, `drain.pass` as the three reference implementations. Link to the 2026-05-13 design plan postscript.
- Update the file's `Last verified:` date.

### Phase 6 — Update backend-server.md

Edit `docs/architecture/backend-server.md`. The investigator reported references at lines 156, 172, 210–213, 238, 243, 284, 381, 398–428, 490–506, 513, 527. Touch each:

- Drain/listener architecture prose paragraphs (156, 172, 238, 243, 284, 381): rewrite to describe per-tick roots, not task-lifetime containers.
- Boundary instrumentation section (210–213) and tokio-spawn discipline paragraph (213): replace `.instrument(task_span)` guidance with the per-tick handler-instrument pattern, citing `eviction.sweep` as the canonical example.
- Readiness probe / gap-healing / task shutdown sections (398–428, 490–506, 513, 527): scrub any operational claim that depends on `listener.task` or `drain.task` being a queryable span name. The readiness probe and gap-healing logic do not depend on span emission; verify by reading the cited paragraphs.
- Update the file's `Last verified:` date.

### Phase 7 — Full verification and PR

Run, from `/Users/brajkovic/Projects/atc/backend`:
- `cargo nextest run` (full backend suite)
- `cargo clippy --workspace --all-targets -- -D warnings` (via `just lint` if equivalent)

From `/Users/brajkovic/Projects/atc`:
- `just lint`
- `just test`
- `git push` and observe the pre-push hook running `scripts/check-docs-lefthook.sh`; confirm zero doc-staleness violations.

Open the PR with title `refactor(server): drop task-lifetime root spans on listener and drain (#170)` and link to the issue in the body. Single squash-merge PR per `CONTRIBUTING.md` § Pull Requests.

## Acceptance Criteria

**AC1.** `cd /Users/brajkovic/Projects/atc/backend && cargo nextest run -p atc-server` exits 0 with all `tracing_webhook_spans_test` cases (including `drain_pass_is_root_with_drain_broadcast_child` and `listener_recv_is_root`) passing.

**AC2.** `cd /Users/brajkovic/Projects/atc && git grep -n -e 'listener\.task' -e 'drain\.task' -- backend/crates/atc-server/src/ backend/crates/atc-server/tests/ docs/architecture/ CLAUDE.md backend/crates/atc-server/CLAUDE.md CONTRIBUTING.md` returns zero hits. *(Scope intentionally excludes `docs/design-plans/`, where historical references in this plan and the 2026-05-13 plan are preserved by design.)*

**AC3.** `cd /Users/brajkovic/Projects/atc && rg -n 'info_span!\("listener\.task"|info_span!\("drain\.task"|\.instrument\(task_span\)' backend/` returns zero hits.

**AC4.** `cd /Users/brajkovic/Projects/atc/backend && cargo clippy -p atc-server -- -D warnings` exits 0 — `unused_imports` is a default-warn lint and `-D warnings` escalates it to error, catching a leftover `use tracing::Instrument` if the wrapper was the last user. *(`cargo build` alone would only warn; the clippy gate is the one that fails the build.)*

**AC5.** The doc-staleness gate passes on the feature branch. Verifiable by either running `lefthook run pre-push` and observing exit 0, OR invoking the script directly: `bash scripts/check-docs-lefthook.sh` exits 0. *(The script is silent on success per `scripts/check-docs-lefthook.sh:50-69`; do not grep for a success string — there isn't one.)*

**AC6.** `docs/architecture/metrics.md` contains a subsection whose body explains why task-lifetime root spans do not export. Verifiable by `git grep -nE 'anti-pattern|does not export' docs/architecture/metrics.md` returning at least one match. *(Both phrases are novel to that file pre-change — verified by `git grep -nE 'anti-pattern|does not export' docs/architecture/metrics.md` returning zero hits on `main` as of 2026-05-14.)*

**AC7.** `just lint` and `just test` both exit 0 on the feature branch HEAD.

**Failure cases the ACs catch:**
- Leftover `.instrument(task_span)` wrapper → AC1 (test fails) + AC3 (grep hits).
- Forgetting to remove `Instrument` import after dropping its only user → AC4 (clippy `-D warnings`) + AC7 (`just lint` runs the same gate).
- Missing edit to either architecture doc (`metrics.md` / `backend-server.md`) → AC5 (doc-staleness gate blocks).
- Missing edit to any of the in-scope files (CLAUDE.md, tests, source, architecture docs) that leaves a `listener.task` or `drain.task` string behind → AC2 (grep hits).
- Forgetting to scrub the `eviction_spans_test.rs` module docstring → AC2 (grep hits).
- Inverting the wrong assertion in the test rename → AC1 (test still fails or asserts wrong shape).
- `drain.broadcast` accidentally losing its `drain.pass` parent due to refactoring the `.in_scope()` block → AC1 (existing assertion at the old line 188–195 catches this).
- Forgetting the new anti-pattern subsection in `metrics.md` → AC6.

## Documents to Update

| Document | Change |
|---|---|
| `backend/crates/atc-server/CLAUDE.md` | Remove `listener.task` and `drain.task` from the spans bullet (the 5-name list inside the bullet that begins "Spans: boundary instrumentation lives in…"); rewrite the "PG-side spawned futures (…) construct a task-lifetime root at spawn time" sentence to describe per-handler `#[instrument]` roots, citing `eviction.sweep` as the established pattern; bump `Last verified:`. |
| `docs/architecture/metrics.md` | **Must edit:** the two rows for `listener.task` and `drain.task` in the span-inventory table (around 390–399); the "Background-task boundaries" bullet at 78 that says these need `.instrument(span)`; the [Tokio spawn gotcha] section at 124–133 that holds them up as the canonical task-lifetime example; the listener/drain span list lines at 60–61. Add a ≤8-line **anti-pattern subsection** explaining why long-lived task-lifetime roots do not export (SDK only emits on close; spawn-attached spans close only at process shutdown; lost on SIGKILL). Bump `Last verified:`. **Re-read but likely no change:** prose at 232–237, 247–305, 312, 322, 332–342 — confirm those describe operational metric/span behavior and don't restate the `.instrument(task_span)` recipe; edit only if they do. |
| `docs/architecture/backend-server.md` | **Must edit:** the boundary-instrumentation section at 210–213 (tokio-spawn discipline paragraph that names the wrappers); the drain/listener architecture prose at 156, 172, 238, 243, 284, 381 — narrow each to the sentence(s) that name the removed span shape; rewrite to describe per-tick roots citing `eviction.sweep`. Bump `Last verified:`. **Re-read but likely no change:** readiness probe at 398–428, gap-healing at 490–506, task shutdown at 513–527 — confirm none of those operational behaviors depend on `listener.task` / `drain.task` as a span name; edit only if a load-bearing sentence references the span shape. |
| `backend/crates/atc-server/tests/integration/tracing_webhook_spans_test.rs` | Rename `drain_pass_span_is_child_of_drain_task` to `drain_pass_is_root_with_drain_broadcast_child`; invert parent-span-id assertions for `drain.pass` (assert root). Delete the `drain.task` existence and trace-id-sharing assertions (currently at lines 210–220) and scrub the comments and failure-message text that reference `drain.task` (currently at lines 154, 178, 203). Preserve the `drain.broadcast` parent-is-`drain.pass` assertions (currently at lines 188–195). Add `listener_recv_is_root` using the inline `parent_span_id.to_bytes()` pattern. AC2 catches any leftover `drain.task` string. |
| `backend/crates/atc-server/tests/integration/eviction_spans_test.rs` | Rewrite the module docstring at lines 1–8 to remove the stale "Unlike `listener.task` / `drain.task`" comparison; describe per-tick roots as the shared convention with `listener.recv` and `drain.pass`. Required for AC2's grep to pass. |
| `backend/crates/atc-server/src/listener.rs` | Drop `info_span!("listener.task")` + `.instrument(task_span)` at the listener spawn site (around line 86); drop `info_span!("drain.task")` + `.instrument(task_span)` at the drain spawn site (around line 203). Remove `use tracing::Instrument` if the file has no other user — grep first; the file may still need the trait for other `.instrument(...)` calls. |

No `scripts/doc-mapping.sh` change. The existing mapping at lines 39–46 already routes `listener.rs` to both architecture docs.

`CONTRIBUTING.md:402` mentions `drain.broadcast` as a span-naming-convention example; `drain.broadcast` is preserved, so no change.

## Out of Scope

- **Eviction spans.** Already correct per #163; not touched.
- **Webhook ingestion or REST handler span shape.** Outside the issue scope; would need a separate audit.
- **ADR for the anti-pattern.** Decided no in the planning session (see Locked Decisions); rationale lives in `metrics.md` and the 2026-05-13 postscript.
- **Adding `drain.broadcast` attributes or renaming any preserved span.** Out of scope.
- **Operational dashboards.** The issue notes the span names are documented only in the inventory table; the investigator confirmed no operational consumer references them. If a downstream dashboard turns out to depend on `listener.task`/`drain.task`, a follow-up issue covers it.

## Glossary

- **Task-lifetime root span**: a span attached via `.instrument(span)` to a `tokio::spawn`-ed future, whose lifetime is therefore bounded by the spawned task. For long-lived tasks (listener loop, drain loop, eviction loop), this is the entire process lifetime.
- **Per-tick root**: a span whose lifetime is a single unit of work (one NOTIFY, one drain pass, one eviction sweep) and which exports immediately on completion.
