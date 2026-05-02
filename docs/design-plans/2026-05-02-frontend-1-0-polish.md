# Frontend 1.0 Polish Design

## Summary

Sub-Phase 6b layers six independent concerns onto the existing frontend without restructuring the component tree and without changing the architectural patterns established across Sub-Phases 1–6a. The new `LiveRegion` is a global rune-class store in `lib/aria/live-region.svelte.ts`, structurally peer to the existing five stores (RunStore, RunnerStore, ConnectionStore, UIStore, PaletteStore) but consumed by a single component (`AriaLiveRegion.svelte`). The earlier "five-store ceiling" framing is dropped: the architectural discipline is store-vs-component-state-vs-prop, not numeric. The responsive grid change is a Tailwind v4 class cascade swap on `KanbanBoard.svelte` plus surgical edits inside `TopBar.svelte` (wrap + separator hide at `<md`) and `RunnerPool.svelte` (label truncation at `<md`) — no new layout primitives, no container queries (the detail panel uses Bits UI Sheet overlay-style, so kanban width tracks the viewport directly). Documentation cleanup lands first, in a no-functional-change phase that rewrites seven structural "wall display" occurrences and revises the SP6b contract in `ui-decomposition/README.md` before any component code ships.

The most architecturally novel piece is the ARIA live region. `EventDispatcher` gains a `setOnFlush(cb)` post-flush callback hook: after applying batched events to stores, the dispatcher invokes the callback with the flushed `ReadonlyArray<SeqEvent>` from that tick. `LiveRegion.observeFlush` consumes the events directly — counting `RunEvent::Requested` and `RunEvent::Completed` transitions per run (intermediate `RunEvent::InProgress` hops are not announced) and routing output through either a per-run message path (≤3 transitions per flush) or a `BurstAccumulator` (>3 transitions, 200 ms debounce spanning the entire burst window). The DOM surface is a single `role="status"` `aria-live="polite"` `aria-atomic="true"` element whose `textContent` is replaced on each announcement; `aria-busy` flips `true` during the burst-debounce window so screen readers defer until the summary is composed. Transition classification is driven by ts-rs-generated `RunStatus` and `RunConclusion` types: a `Record<RunConclusion, string>` verb dictionary gives compile-time exhaustiveness, and the `RunStatus` switch uses `const _: never = next` as a compile-time exhaustiveness guard. The remaining concerns — reduced-motion gate on the one ungated `CommandPalette` slide, `:focus-visible` rules on four custom interactive elements, and a global `.atc-scrollbar` class in `app.css` — are surgical point fixes with no cross-cutting impact.

## Definition of Done

Sub-Phase 6b is the final frontend sub-phase before the bundled 1.0 release. It ships when, on the merged feature branch:

1. **EmptyState component exists** as a real, pure Svelte component, replacing the inline "No workflows yet." string in `KanbanBoard`. Visual treatment is the schematic preview locked in `## Architecture` (three labeled column groups with monospace placeholder rows + "Watching for runs." caption). Tri-state predicate (connecting / connected-empty / populated) preserved.

2. **Kanban degrades responsively below 1024px** with no horizontal page scroll at any width ≥320px. Approach is locked: two-step Tailwind v4 cascade (`grid-cols-1 sm:grid-cols-2 xl:grid-cols-3`) on the kanban; wrap + separator hide inside `TopBar.svelte`; label truncation inside `RunnerPool.svelte`. No container queries (deferred). `ui-decomposition/README.md` SP6b contract updated in this PR to drop mobile tabs and adopt 640/1280 bands as the 1.0 contract.

3. **Reduced-motion audit complete:** the one ungated animation (CommandPalette theme submenu slide on `CommandPalette.svelte:218`) is gated; every animation has at least one test asserting reduced-motion behavior; audit findings recorded in this design doc.

4. **Scrollbar styling applied** to kanban columns and panel body. Cross-browser via `::-webkit-scrollbar` + `scrollbar-width`/`scrollbar-color`. Palette continues to hide scrollbars.

5. **Focus rings: every interactive element has a visible `:focus-visible` rule.** Specifically: `command-item`'s `outline: hidden` is replaced with a visible focus indicator; `PoolFilterPill` clear button, `PanelActions` close button, and the Go-to-run link gain explicit `:focus-visible` rules. Token uniformity (3px ring vs 2px outline) explicitly documented as out-of-scope.

6. **ARIA live region announces run-level transitions** with messages of the form "Run {displayTitle} for {org}/{repo} on {branch} ({event}) {queued|completed-verb}". Hybrid coalescing: per-run announcements below the burst threshold, summary form ("N runs queued, M completed (X succeeded, Y failed, ...)") above threshold. Single `role="status" aria-live="polite" aria-atomic="true"` element at App.svelte level (sibling to AppShell); messages derived from the dispatcher's flushed `SeqEvent[]` via `EventDispatcher.setOnFlush`. `LiveRegion` is a new module-scope rune-class store consumed only by `AriaLiveRegion.svelte`.

7. **Performance verification under a 1000-event burst:** RAF-coalescing assertion gates CI as a hard fail (instrumented `EventDispatcher` flush count under burst load). Frame-timing trace runs as an informational/non-blocking artifact.

8. **"Wall display" framing removed** from 7 structural occurrences across `.impeccable.md`, `docs/ideation/ui-decomposition/component-patterns.md`, `docs/ideation/ui-decomposition/README.md`, `docs/design-plans/2026-04-25-interactivity.md`, and `docs/ideation/design-research.md`. Concourse competitive-research mentions stay (they describe Concourse's product, not ATC's).

9. **All prior sub-phase E2E tests still pass** (full regression).

10. **Architecture docs and the SP6b section of `docs/ideation/ui-decomposition/README.md` updated** to reflect what shipped.

**Out of scope:**

- Mobile-targeted layouts (no tab navigation, no <640px-specific design). Tabbed mobile single-column nav is dropped from the 1.0 contract; `ui-decomposition/README.md` SP6b section is updated in this PR to reflect this.
- Touch-device manual verification from Sub-Phase 5
- Log fetching for the detail panel (issue #36)
- Focus-token uniformity across shadcn-vs-custom components
- EmptyState variants for filtered states (e.g., "no runs match this pool filter")
- History affordance in the live region (no append-log; `role="status"` replaces text on each announcement)

## Acceptance Criteria

### frontend-1-0-polish.AC1: EmptyState component

- **frontend-1-0-polish.AC1.1 Success:** When `connectionStore.status === 'connected'` and `totalRuns === 0`, `<EmptyState />` renders with the default caption "Watching for runs."
- **frontend-1-0-polish.AC1.2 Success:** When `<EmptyState message="..." />` is given a custom message, the rendered caption uses that string instead of the default.
- **frontend-1-0-polish.AC1.3 Success:** The schematic preview renders three labeled column groups (Queued, Running, Completed), each containing three rows of placeholder dots.
- **frontend-1-0-polish.AC1.4 Edge:** When `connectionStore.status !== 'connected'`, the existing "Connecting…" hydration placeholder renders — NOT `<EmptyState />`.

### frontend-1-0-polish.AC2: Responsive layout

- **frontend-1-0-polish.AC2.1 Success:** At viewport width ≥1280px, the kanban grid renders three columns (computed `gridTemplateColumns` resolves to three tracks).
- **frontend-1-0-polish.AC2.2 Success:** At viewport width 640–1279px, the kanban grid renders two columns (Completed wraps onto row 2).
- **frontend-1-0-polish.AC2.3 Success:** At viewport width <640px, the kanban grid renders a single-column stack.
- **frontend-1-0-polish.AC2.4 Success:** At viewport width <768px, the TopBar wraps to two rows: logo + connection indicator on row 1, RunnerBar + settings popover on row 2.
- **frontend-1-0-polish.AC2.5 Failure:** At any viewport width ≥320px, `document.documentElement.scrollWidth <= clientWidth` (no horizontal page scroll).

### frontend-1-0-polish.AC3: Reduced-motion audit

- **frontend-1-0-polish.AC3.1 Success:** With `prefers-reduced-motion: reduce`, `CommandPalette`'s theme submenu transitions with `duration: 0` (open/close is instantaneous).
- **frontend-1-0-polish.AC3.2 Success:** Every animation in the inventory table in `## Architecture` has at least one automated test that asserts its behavior under `prefers-reduced-motion: reduce`.
- **frontend-1-0-polish.AC3.3 Failure:** With `prefers-reduced-motion: reduce` enabled, no kanban transition, halo effect, or palette animation has a duration > 0; existing reduced-motion tests in `kanban-transitions.test.ts` continue to pass.

### frontend-1-0-polish.AC4: Scrollbar styling

- **frontend-1-0-polish.AC4.1 Success:** The `KanbanColumn` scroll container element has the `atc-scrollbar` class applied.
- **frontend-1-0-polish.AC4.2 Success:** The `RunDetailPanel` body container element has the `atc-scrollbar` class applied.
- **frontend-1-0-polish.AC4.3 Edge:** `CommandPalette`'s list retains its existing `no-scrollbar` class; the `atc-scrollbar` class is NOT applied to it.

### frontend-1-0-polish.AC5: Focus rings

- **frontend-1-0-polish.AC5.1 Success:** `command-item` no longer has `outline: hidden`; on `:focus-visible`, computed `outline-style: solid` and `outline-width: 2px`.
- **frontend-1-0-polish.AC5.2 Success:** `PoolFilterPill` clear button, `PanelActions` close button, and the `PanelActions` Go-to-run link each render `outline-style: solid` and `outline-width: 2px` on `:focus-visible`.
- **frontend-1-0-polish.AC5.3 Success:** Tab-cycling through every interactive surface in the dashboard reaches a state where every focused element has either `outline-width >= 2px` or `box-shadow !== 'none'`; no element is focusable with no visible indicator.

### frontend-1-0-polish.AC6: ARIA live region

A **transition** is a single `RunEvent::Requested` or `RunEvent::Completed` event in the flushed `SeqEvent[]`. `RunEvent::InProgress` events do not produce announcements (they are intermediate hops; not user-relevant for SR). Same-flush `Queued→InProgress→Completed` for one run produces two announcements (Queued + Completed). Snapshot loads bypass the dispatcher entirely (see AC6.7) and never generate announcements.

- **frontend-1-0-polish.AC6.1 Success:** A single `<div role="status" aria-live="polite" aria-atomic="true" aria-busy="false" aria-label="Workflow run updates" class="sr-only">` mounts in the DOM at App.svelte level (sibling to AppShell). Initial `aria-busy="false"` (explicit value, not bare attribute).
- **frontend-1-0-polish.AC6.2 Success:** When ≤3 transitions occur in a single dispatcher flush, the live region's `textContent` is replaced with the per-run messages joined by `". "` in the form `"Run {displayTitle} for {org}/{repo} on {branch} ({event}) {verb}"` (`verb` is `"queued"` for `RunEvent::Requested` events; for `RunEvent::Completed`, the conclusion-specific verb from `VERB_BY_CONCLUSION`); `aria-busy` stays `"false"`. `aria-atomic="true"` ensures SR re-announces the full new string each replacement.
- **frontend-1-0-polish.AC6.3 Success:** When >3 transitions occur in a flush, `aria-busy` flips to `"true"`; the BurstAccumulator opens with a 200ms debounce; transitions from the opening flush AND every subsequent flush within the debounce window contribute to accumulated counts (regardless of per-flush count — even a 2-transition flush during an open window adds to counts); on debounce close, `textContent` is replaced with a single summary message of the form `"N runs queued, M completed (X succeeded, Y failed, Z cancelled, W timed out)."` covering all transitions across the entire burst window; `aria-busy` flips back to `"false"`. The summary verb-by-conclusion breakdown is keyed by the same `VERB_BY_CONCLUSION` table as per-run; absent counts are elided (e.g., if no cancellations, the parenthetical drops the "Z cancelled" segment).
- **frontend-1-0-polish.AC6.4 Success:** When `WorkflowRun.branch` is null, the per-run message elides the "on {branch}" segment instead of rendering "on null".
- **frontend-1-0-polish.AC6.5 Success:** Adding a new `RunConclusion` variant in `atc-core` and regenerating ts-rs types fails the frontend `tsc` step until `VERB_BY_CONCLUSION` adds a verb for the new variant (compile-time exhaustiveness via `Record<RunConclusion, string>`). A vitest type-level test asserts the equivalence using a tsd-style helper: `type _Check = Expect<Equal<keyof typeof VERB_BY_CONCLUSION, RunConclusion>>` (where `Expect`/`Equal` are the standard tsd-helper utilities).
- **frontend-1-0-polish.AC6.6 Resilience:** `classifyEvent` throws on invariant violation (e.g., a `Completed` `RunEvent` arrives with `conclusion === null`, or an off-shape input slips through type narrowing). `LiveRegion.observeFlush` wraps the per-update classify call in try/catch: the violation is logged via `console.error` with the offending event payload and that update is skipped from announcement. Remaining well-formed transitions in the flush still announce normally; one bad event does not contaminate the batch.
- **frontend-1-0-polish.AC6.7 Edge:** Snapshots loaded by `ConnectionManager` go directly into stores via `runStore.loadSnapshot` (`connection.ts:106`), bypassing the dispatcher. However, `connection.ts:111-117` then dispatches buffered post-snapshot events (`preConnectBuffer` entries with `seq >= snapshot.seq`) through `eventDispatcher.dispatch + flush` — which would normally fire `setOnFlush` and announce them. To preserve true reconnect silence, `ConnectionManager` defers the `dispatcher.setOnFlush(liveRegion.observeFlush.bind(liveRegion))` wiring call until AFTER the post-snapshot buffered drain completes (i.e., after line 117's `eventDispatcher.flush()`). Initial connect: snapshot loaded silently → buffered events drained silently → setOnFlush wired → subsequent live events announce. Reconnect: same sequence — `setOnFlush` is unwired on disconnect (or replaced with a no-op) and re-wired only after the next snapshot+buffer drain. Result: zero announcements during snapshot or buffered-replay drain; state changes that occurred during downtime are visible in the kanban but not announced.

### frontend-1-0-polish.AC7: Performance verification

- **frontend-1-0-polish.AC7.1 Success:** A vitest browser-mode test uses `vi.useFakeTimers()` and a manually-driven RAF queue: enqueues 1000 mock events, advances RAF in N controlled ticks, asserts `flushSpy.mock.calls.length === N` (deterministic, not bounded), asserts `runStore.totalRuns === 1000` (or equivalent reflecting all events landed in store state), and asserts no events are dropped. Failure is a CI hard fail. Real timers are explicitly avoided to eliminate wall-clock flake.
- **frontend-1-0-polish.AC7.2 Success:** A Playwright frame-budget test runs `page.tracing.start({ categories: ['rendering'] })`, fires 1000 WS events through `e2e/lib/ws-mock`, parses BeginFrame deltas from the trace JSON, saves the trace as `frame-budget-trace.json` (CI artifact), logs a frame-budget summary; the test always passes (informational).

### frontend-1-0-polish.AC8: Wall-display framing removed

- **frontend-1-0-polish.AC8.1 Success:** Each of the seven structural occurrences (`.impeccable.md:6`; `component-patterns.md:34, 36, 186`; `ui-decomposition/README.md:217, 379`; `design-research.md:119, 134`) is rewritten or deleted per the table in `## Architecture`.
- **frontend-1-0-polish.AC8.2 Success:** `docs/design-plans/2026-04-25-interactivity.md` lines 30 and 375 each carry a "Revised by `docs/design-plans/2026-05-02-frontend-1-0-polish.md`" annotation; the original prose remains intact.
- **frontend-1-0-polish.AC8.3 Edge:** Concourse competitive-research mentions in `docs/ideation/research/concourse-ci-ui-design.md` (lines 28, 70, 88, 92, 101, 158, 219) are NOT modified.

### frontend-1-0-polish.AC9: Prior sub-phase E2E regression

- **frontend-1-0-polish.AC9.1 Success:** The full pre-existing Playwright E2E suite (kanban, theme, run-cards, palette, panel, run-card-interactivity, pool-filter, dialog-stacking, kanban-keyboard-nav) passes after all SP6b changes are applied.

### frontend-1-0-polish.AC10: Architecture docs updated

- **frontend-1-0-polish.AC10.1 Success:** `docs/architecture/frontend-app.md` includes sections for the EmptyState component, the AriaLiveRegion module, the responsive breakpoint contract, and the animation inventory matrix.
- **frontend-1-0-polish.AC10.2 Success:** The Sub-Phase 6b section in `docs/ideation/ui-decomposition/README.md` is marked "✅ COMPLETE" and follows the "What was built:" structure used by Sub-Phases 1–6a, listing the actual deliverables that shipped.

## Glossary

- **RAF (requestAnimationFrame)**: Browser API that schedules a callback before the next paint frame. `EventDispatcher` batches incoming WebSocket events and flushes them once per RAF tick to avoid mid-frame store mutations.
- **Bits UI**: Headless Svelte component library providing portal management, focus trapping, and dialog stacking primitives. Used for the `Sheet` (detail panel) and `Command` (palette) primitives inherited from Sub-Phase 5.
- **shadcn-svelte**: A port of the shadcn component collection to Svelte, vendored directly into the project rather than installed as a dependency. Provides `CommandPalette` and `RunDetailPanel` base components modified in this phase.
- **OKLCH**: Perceptually uniform cylindrical color space. The ATC design system uses OKLCH tokens throughout `app.css` for all theme hues, including the scrollbar thumb colors added in this phase.
- **Tailwind v4**: The version of Tailwind CSS used in this project, which uses a CSS-first configuration model and supports mobile-first cascade utilities (`sm:`, `xl:`) used for the responsive grid change.
- **Svelte 5 runes**: Svelte 5's reactive primitive system (`$state`, `$derived`, `$effect`). The `LiveRegion` class uses `$state` fields directly; `prefersReducedMotion.current` is a rune-based reactive media query from `svelte/motion`.
- **ts-rs**: A Rust crate that generates TypeScript type definitions from Rust types. In ATC, `RunStatus` and `RunConclusion` (used in the ARIA live region's exhaustive switch) are generated from `atc-core` via `just types`.
- **RunStatus**: The ts-rs-generated TypeScript union type representing a workflow run's current column state (Queued, InProgress, Completed). The `classifyEvent` function switches exhaustively over this type.
- **RunConclusion**: The ts-rs-generated TypeScript union type representing the terminal outcome of a completed run (success, failure, cancelled, timed_out, etc.). `VERB_BY_CONCLUSION` is keyed by this type for compile-time exhaustiveness.
- **WorkflowRun**: The core domain type representing a GitHub Actions workflow run. The ARIA live region reads `WorkflowRun.branch`, `WorkflowRun.displayTitle`, and conclusion fields when composing announcement messages.
- **EventDispatcher**: The frontend class in `frontend/src/lib/dispatcher.ts` that receives batched WebSocket events and flushes them to stores once per RAF. This phase adds a `setOnFlush(cb: (events: ReadonlyArray<SeqEvent>) => void)` hook that invokes `cb` with the flushed event list after store mutation.
- **ConnectionManager**: The class in `frontend/src/lib/connection.ts` that owns the WebSocket lifecycle and wires `EventDispatcher` to the stores. This phase adds deferred `setOnFlush` wiring: attached after the post-snapshot buffered drain completes (after `connection.ts:117`'s `eventDispatcher.flush()`); detached on disconnect (`dispatcher.setOnFlush(null)`); re-attached after each reconnect's snapshot+drain. Snapshots loaded over WS go directly into stores via `runStore.loadSnapshot` (`connection.ts:106`) and bypass the dispatcher entirely.
- **BurstAccumulator**: The internal state object within `LiveRegion` that aggregates transition counts across multiple RAF flushes when one flush's transition count exceeds the burst threshold (3). Once open, every subsequent flush within the debounce window contributes its transitions to counts regardless of that flush's per-tick count. Debounces over 200 ms; counts span the entire burst window.
- **prefers-reduced-motion**: CSS media query and OS accessibility setting that signals the user prefers minimal animation. Svelte exposes it as `prefersReducedMotion.current` from `svelte/motion`. This phase gates the one remaining ungated animation (`CommandPalette` theme submenu slide) and adds tests for every animation in the inventory table.
- **`role="status"`**: ARIA role for a live region that announces a single status string; implies `aria-live="polite"` by default. The `AriaLiveRegion` component uses this role plus explicit `aria-live="polite"` and `aria-atomic="true"`. Chosen over `role="log"` because there is no history affordance in the design — the kanban itself is the visual record, and `textContent` replacement is the natural primitive.
- **`aria-atomic`**: ARIA attribute set to `"true"` so screen readers re-announce the entire `textContent` on each replacement (rather than diffing the change). Required for the textContent-replace announcement model under `role="status"`.
- **`aria-busy`**: ARIA attribute toggled to `"true"` during the BurstAccumulator's debounce window so SR defers announcement of intermediate text changes until the busy state clears, then announces the final summary string. Returns to `"false"` on debounce close.
- **`aria-live`**: ARIA attribute that designates an element as a live region. Set to `"polite"` so screen readers finish the current utterance before announcing transitions.
- **Viewport breakpoints**: Fixed-width thresholds used for the responsive grid. This design uses Tailwind v4's `sm:` (≥640px) and `xl:` (≥1280px) rather than container queries, because the detail panel uses overlay-style positioning and does not affect kanban width.
- **Container queries**: A CSS feature that sizes layout relative to a containing element's width rather than the viewport. Explicitly evaluated and deferred for this phase; the responsive contract uses viewport breakpoints. Documented as a future migration path.
- **EmptyState (schematic preview)**: The visual treatment of the `EmptyState.svelte` component when connected with zero runs: three faint dashed column groups labeled Queued / Running / Completed, each containing three rows of monospace placeholder dots, with a "Watching for runs." caption below.
- **ws-mock harness**: The E2E test utility at `e2e/lib/ws-mock.ts` (`makeRunEvent`, `makeJobSeqEvent`, `sendWS`) that injects synthetic WebSocket events into the browser page during Playwright tests. Used in the performance verification and ARIA live region E2E tests.
- **LiveRegion**: The project's module in `frontend/src/lib/aria/live-region.svelte.ts` — a rune-class store that owns `message` and `busy` state and exposes an `observeFlush(events: ReadonlyArray<SeqEvent>)` method. Structurally peer to the existing five stores (global mutable reactive state) but consumed by a single component (`AriaLiveRegion.svelte`).
- **Sub-Phase 6b**: The final frontend sub-phase before the 1.0 release bundle. Covers EmptyState, responsive layout, ARIA live region, performance verification, polish audits, and documentation cleanup. The preceding standalone deliverable was Sub-Phase 6a (kanban keyboard navigation).
- **Documents to Update table**: A mandatory section in every ATC design plan enumerating every file that must be modified alongside the implementation, per project documentation conventions. Ensures architecture docs stay in sync with code changes.

## Architecture

Sub-Phase 6b layers six concerns onto the existing frontend without restructuring the component tree:

1. **EmptyState extraction.** A new pure component `EmptyState.svelte` replaces the inline `"No workflows yet."` string in `KanbanBoard.svelte`. The schematic-preview treatment (three faint dashed columns with monospace placeholder rows + "Watching for runs." caption) sits inside the existing tri-state predicate (`connectionStore.status === 'connected' && totalRuns === 0`).

2. **Responsive grid.** `KanbanBoard.svelte`'s hardcoded `grid-cols-3` becomes a Tailwind v4 mobile-first cascade: `grid-cols-1 sm:grid-cols-2 xl:grid-cols-3`. The detail panel uses Bits UI Sheet overlay-style (does not shrink the kanban), so viewport breakpoints are sufficient — container queries would adapt to nothing the kanban cares about today. TopBar wrap behavior lives inside `TopBar.svelte` itself (the parent AppShell composes `<TopBar />` opaquely; the wrap reorder cannot be driven from outside without `:global()` cascade). The current TopBar markup (`TopBar.svelte:105-126`) groups `ConnectionIndicator` + `SettingsPopover` inside a single right-side wrapper `<div>`, so simple `flex-wrap` + `order-*` cannot land logo+connection on row 1 and RunnerBar+settings on row 2 — these need to be siblings, not children of one wrapper. The implementation requires a small markup rewrite of `TopBar.svelte`: split the right cluster into separate sibling elements (Logo, RunnerBar wrapper, ConnectionIndicator, SettingsPopover all as direct children of the header `<div class="flex flex-wrap">`), give the RunnerBar wrapper an explicit `basis-full md:basis-auto` so it claims the full row at `<md`, hide separators with `hidden md:block`, and use `order-1 md:order-none`-style utilities so the visual order at `<md` is logo + connection (row 1) then RunnerBar + settings (row 2). Pool label truncation lives inside `RunnerPool.svelte` (the leaf that owns the `<span>` wrapping the pool label): `truncate` + `max-w-[12ch]` applied at `<md` via Tailwind responsive variants.

3. **ARIA live region.** A new `frontend/src/lib/aria/live-region.svelte.ts` module owns a `LiveRegion` rune-class store. The companion `AriaLiveRegion.svelte` component mounts at `App.svelte` level as a sibling to `<AppShell>`, rendering a single `<div role="status" aria-live="polite" aria-atomic="true" aria-busy={liveRegion.busy ? 'true' : 'false'} aria-label="Workflow run updates" class="sr-only">{liveRegion.message}</div>`. `EventDispatcher` (in `lib/dispatcher.ts`) gains a post-flush callback hook (`setOnFlush(cb)`) — after applying batched events to stores, the dispatcher invokes `cb(events)` with the flushed `ReadonlyArray<SeqEvent>` from that tick, but only when `events.length > 0` (empty drains do not invoke the callback). `flush()` cancels any pending RAF before draining so `dispatch(); flush();` produces exactly one non-empty callback rather than a real call followed by a phantom RAF empty call. `LiveRegion.observeFlush(events)` walks the event list directly: it picks out `RunEvent::Requested` and `RunEvent::Completed` events (skipping `RunEvent::InProgress`), classifies each via a ts-rs-driven exhaustive switch over `RunConclusion`, and emits either per-run messages (≤3 transitions per flush) or accumulates into a `BurstAccumulator` for summary form (>3 transitions, debounced over 200ms with counts spanning the entire burst window). Snapshots loaded by `ConnectionManager` (`connection.ts:106` `runStore.loadSnapshot`) bypass the dispatcher entirely. The post-snapshot buffered drain at `connection.ts:111-117` does flow through `eventDispatcher.dispatch + flush`, but `ConnectionManager` defers the `setOnFlush` wiring until after that drain completes — so neither snapshot data nor buffered-replay events announce. After wiring, all subsequent live events do announce (per AC6.2/AC6.3).

4. **Performance verification.** Two-tier metric. Tier 1 is a vitest browser-mode test that uses `vi.useFakeTimers()` and a manually-driven RAF queue to fire 1000 mock events through `EventDispatcher` deterministically. Asserts `flushSpy.mock.calls.length === N` for the chosen number of controlled RAF ticks (no wall-clock dependency, no slack), and asserts every event landed in store state. Hard CI gate. Tier 2 is a Playwright test that records a `page.tracing` artifact during a synthesized 1000-event burst (using the new `sendWSBatch` helper with its synchronization fence); saves the trace as a CI artifact and logs a structured frame-budget summary (`p50_ms`, `p95_ms`, `dropped_frames`); never fails (informational).

5. **Polish audits.** Reduced-motion: one ungated animation (`CommandPalette.svelte:218`'s theme submenu slide) gets the `prefersReducedMotion.current` gate matching `kanban-transitions.ts`. Existing reduced-motion test coverage is also strengthened in this phase: `KanbanColumn.browser.test.ts:281` is rewritten so the `prefersReducedMotion` mock binds before module import (the only viable approach in vitest browser-mode — Playwright's `emulateMedia` is not available there), and `theme.test.ts:267` (which IS Playwright) is rewritten to assert computed `animation-duration: 0s` on a halo'd element under `emulateMedia({ reducedMotion: 'reduce' })` rather than only checking that the global CSS reset rule exists. Focus rings: four custom interactive elements (`command-item`, `PoolFilterPill` clear, `PanelActions` close, `PanelActions` Go-to-run) gain explicit `:focus-visible` rules adopting `RunCard.svelte:240`'s 2px+2px outline pattern. Scrollbars: a global `.atc-scrollbar` class in `app.css` provides cross-browser thin-thumb styling using existing OKLCH tokens — thumb is `var(--border)` with alpha modulation, track is transparent (no new tokens). `scrollbar-width`/`scrollbar-color` for Firefox, `::-webkit-scrollbar` with `border + background-clip: padding-box` for Chromium/Safari per the Rauno Freiberg pattern. Applied to `KanbanColumn` and `RunDetailPanel` body.

6. **"Wall display" framing cleanup.** Seven structural occurrences across `.impeccable.md`, `component-patterns.md`, `ui-decomposition/README.md`, `interactivity.md`, and `design-research.md` are surgically rewritten to keep each sentence's actual point while replacing the wall-display justification with the real driver (operator at workstation, motion contrast, etc.). The shipped `2026-04-25-interactivity.md` design plan receives a one-line "Revised by" annotation rather than silent rewrites — the supersession is traceable, the original prose stays intact.

### Module and component boundaries

```
frontend/src/lib/
  aria/                       (new)
    live-region.svelte.ts     LiveRegion rune-class store + BurstAccumulator
    format-run-transition.ts  Pure function: (run, transitionKind) → message string
    transition-kinds.ts       TransitionKind type + classifyEvent + VERB_BY_CONCLUSION
  components/
    EmptyState.svelte         (new) pure component
    AriaLiveRegion.svelte     (new) connected component (reads liveRegion store)
    KanbanBoard.svelte        (modified) inline "No workflows yet." → <EmptyState />, grid cascade
    KanbanColumn.svelte       (modified) gains .atc-scrollbar
    TopBar.svelte             (modified) <md flex-wrap + order-*; hide separators at <md
    RunnerPool.svelte         (modified) truncate + max-w-[12ch] at <md
    CommandPalette.svelte     (modified) reduced-motion gate on submenu slide
    PoolFilterPill.svelte     (modified) :focus-visible rule on clear button
    PanelActions.svelte       (modified) :focus-visible rules on close + Go-to-run
    RunDetailPanel.svelte     (modified) gains .atc-scrollbar on body
    ui/command/command-item.svelte (modified) outline-hidden → focus-visible:outline-*
  dispatcher.ts               (modified) gains setOnFlush(cb) post-flush callback hook
  connection.ts               (modified) wires dispatcher.setOnFlush(liveRegion.observeFlush)
src/main.ts                   (modified) exposes window.eventDispatcher for E2E harness
src/App.svelte                (modified) mount <AriaLiveRegion /> sibling to <AppShell />
src/app.css                   (modified) .atc-scrollbar global class
e2e/lib/ws-mock.ts             (modified) routes synthetic events through window.eventDispatcher
```

### Live region message contract

```typescript
// transition-kinds.ts
import type { RunConclusion } from '$lib/types/generated';

// A transition is the minimal classified form of a per-run announcement event.
// Requested (queued) and Completed only — InProgress hops are filtered before classify.
export type TransitionKind =
  | { kind: 'queued' }
  | { kind: 'completed'; conclusion: RunConclusion };

// Throws on invariant violation (e.g., Completed event with conclusion === null).
// Caller (LiveRegion.observeFlush) is expected to wrap in try/catch and log+skip.
export function classifyEvent(event: SeqEvent): TransitionKind | null;
//   returns null for events that are not announcement-relevant (InProgress, Job-* events, etc.)

export const VERB_BY_CONCLUSION: Record<RunConclusion, string>;

// Compile-time exhaustiveness sentinel; tsd-style helper:
type _CheckExhaustive = Expect<Equal<keyof typeof VERB_BY_CONCLUSION, RunConclusion>>;
```

```typescript
// live-region.svelte.ts
import type { SeqEvent } from '$lib/types/generated';

export class LiveRegion {
  message: string = $state('');
  busy: boolean = $state(false);
  observeFlush(events: ReadonlyArray<SeqEvent>): void;
}
export const liveRegion: LiveRegion;

export interface BurstAccumulator {
  active: boolean;
  startedAtMs: number;
  // Counts keyed by discriminant: 'queued' or 'completed:<conclusion>'.
  // The string key form preserves per-conclusion granularity so the summary
  // can render "(X succeeded, Y failed, Z cancelled, W timed out)".
  counts: Record<string, number>;
}
```

```typescript
// dispatcher.ts (modified)
import type { SeqEvent } from '$lib/types/generated';

export class EventDispatcher {
  setOnFlush(cb: (events: ReadonlyArray<SeqEvent>) => void): void;
}
```

### Animation inventory (reduced-motion audit)

| File:line | Type | Trigger | Gate status | Test coverage |
|---|---|---|---|---|
| `app.css:101-112` | CSS keyframes (halo) | InProgress card | GATED (global CSS reset) | `e2e/theme.test.ts:AC1.6` |
| `lib/animations/kanban-transitions.ts:14-31` | Crossfade send/receive | Cross-column move | GATED (`prefersReducedMotion.current`) | `kanban-transitions.test.ts` |
| `lib/animations/kanban-transitions.ts:20-23` | Fly fallback | New card arrival | GATED | `kanban-transitions.test.ts` |
| `lib/animations/kanban-transitions.ts:27-29` | Fade fallback | Card removal | GATED | `kanban-transitions.test.ts` |
| `KanbanColumn.svelte:49` | `animate:flip` | Reorder within column | GATED (uses `DURATION_MOVE`) | `KanbanColumn.browser.test.ts` |
| `CommandPalette.svelte:218` | `transition:slide` | Theme submenu | **UNGATED → fix in this phase** | new test in this phase |

After Sub-Phase 6b: every animation in the table is GATED with at least one reduced-motion-asserting test.

### Responsive contract

| Viewport | Kanban | TopBar |
|---|---|---|
| `≥1280px` (xl) | 3 columns | Single row |
| `768-1279px` | 2 columns | Single row |
| `640-767px` | 2 columns | Wrapped (2 rows) |
| `<640px` | 1 column stack | Wrapped |

No horizontal scroll at any width ≥320px. The design plan flags the `<768px` TopBar wrap as a verification-during-implementation point — the wrap may be too aggressive and may shift later.

## Existing Patterns

This design follows patterns established across Sub-Phases 1-6a:

- **Pure-component-by-default discipline** (per `docs/ideation/ui-decomposition/README.md`). Every new `.svelte` file is `props in, DOM out` with no store reads. `EmptyState.svelte` is pure. `AriaLiveRegion.svelte` is connected (subscribes to the `liveRegion` singleton) but renders a single fixed-shape DOM node.
- **Test-per-component sibling layout** (`.svelte` + `.test.ts`). `EmptyState.svelte` ↔ `EmptyState.test.ts`. `AriaLiveRegion.svelte` ↔ `AriaLiveRegion.test.ts`.
- **Accessibility-first selectors in tests.** `getByRole('status', { name: /workflow run updates/i })`, `getByLabelText('Workflow run updates')` rather than `getByTestId`.
- **Exported props interface.** `export interface EmptyStateProps { message?: string }`.
- **Store-as-rune-class pattern.** `LiveRegion` follows the same shape as the existing five stores (`runs.svelte.ts`, `runners.svelte.ts`, `connection.svelte.ts`, `ui.svelte.ts`, `palette.svelte.ts`): a class with `$state` fields, a module-level singleton export, consumed by reading the singleton's reactive fields. Whether to call this "the sixth store" is not architecturally meaningful; the discipline is store-vs-component-state-vs-prop, applied consistently. `LiveRegion` qualifies as a store by structure but is consumed by exactly one component.
- **`prefersReducedMotion.current` from `svelte/motion`.** Matches the existing pattern in `lib/animations/kanban-transitions.ts`. The `CommandPalette` slide gate uses the same import path and reactivity shape.
- **ts-rs-generated types as source of truth** (per `feedback_exhaustive_switches_at_boundaries`). `RunStatus` and `RunConclusion` are generated from the Rust `atc-core` crate via `just types`. Verb dictionary uses `Record<RunConclusion, string>` for compile-time exhaustiveness; the classification switch uses `const _: never = next` as a compile-time exhaustiveness guard. Runtime safety at off-shape wire boundaries comes from `classifyEvent` throwing, with caller-side try/catch in `LiveRegion.observeFlush`.
- **2px+2px outline focus-ring pattern** for new custom focus indicators (per `RunCard.svelte:240`). New focus rules adopt this; shadcn-derived components retain their 3px box-shadow ring (token uniformity is OOS).
- **Documents to Update table** per project guidance #6 (`.ed3d/design-plan-guidance.md`). Listed below in Additional Considerations.
- **Crossfade-pair-with-fly-fallback animation pattern** is unchanged. The existing `kanban-transitions.ts` module gates on `prefersReducedMotion.current` once and exports zeroed durations under reduced motion.
- **WS-driven projection-of-state pattern.** Live region announcements are computed from the dispatcher's flushed `SeqEvent[]` at each RAF boundary — not from store mutation side effects, and not from snapshot diffs. Stores remain pure derived projections of server state; the live region is a parallel projection driven by the same event stream.

No new patterns are introduced. The design adds infrastructure (post-flush hook on `EventDispatcher`, ts-rs-driven exhaustive switch, schematic preview empty state) within the existing architectural boundaries.

## Implementation Phases

Five phases, ordered by risk and reviewability. The bulk of the implementation effort is Phase 4 (ARIA live region).

<!-- START_PHASE_1 -->
### Phase 1: Documentation cleanup and scaffolding

**Goal:** Remove the "wall display" framing across the codebase and revise the SP6b contract in the ideation README before any functional code lands.

**Components:**
- `.impeccable.md` line 6 — wall-display reframe to operator-at-workstation
- `docs/ideation/ui-decomposition/component-patterns.md` lines 34, 36, 186 — three rewrites/deletions
- `docs/ideation/ui-decomposition/README.md` — (a) lines 217, 379 wall-display rewrites; (b) SP6b section contract revision: drop mobile-tabbed single-column nav, replace tablet-condensed/mobile-tabs band scheme with the locked 640/1280 cascade documented in this design plan, mark SP6b "in progress"
- `docs/ideation/design-research.md` lines 119, 134 — two rewrites
- `docs/design-plans/2026-04-25-interactivity.md` lines 30, 375 — "Revised by" annotations; original prose preserved

**Dependencies:** None (first phase).

**Done when:** All seven structural wall-display occurrences are rewritten or deleted per the table in `## Architecture`. The SP6b section in `ui-decomposition/README.md` reflects the 640/1280 cascade and explicit deferral of mobile tabs. Concourse competitive-research mentions in `design-research.md` (lines 28, 70, 88, 92, 101, 158, 219) are explicitly preserved. Build and existing tests still pass.

**Note on `scripts/doc-mapping.sh`:** Verified that the existing `frontend/src/*` wildcard mapping already routes new frontend source files to `docs/architecture/frontend-app.md`; no new explicit entry is required. If a narrower override is later needed for the new `aria/` subdirectory, it can be added in Phase 4 alongside the code.

**ACs covered:** `frontend-1-0-polish.AC8.1`, `frontend-1-0-polish.AC8.2`, `frontend-1-0-polish.AC8.3`.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: EmptyState component and responsive layout

**Goal:** Extract the inline empty-state string into a pure component with the schematic-preview treatment; introduce responsive breakpoints across the kanban grid and TopBar.

**Components:**
- `frontend/src/lib/components/EmptyState.svelte` (new) — pure component with `message?: string` prop, schematic preview rendering
- `frontend/src/lib/components/EmptyState.test.ts` (new) — schematic structure, caption rendering, prop override
- `frontend/src/lib/components/KanbanBoard.svelte` (modified) — replaces inline `"No workflows yet."` with `<EmptyState />`; grid class becomes `grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4`; container gains `min-width: 0`
- `frontend/src/lib/components/TopBar.svelte` (modified) — small markup rewrite: split the existing right-side wrapper `<div class="flex items-center gap-3">` so `ConnectionIndicator` and `SettingsPopover` become direct children of the header `<div>` rather than children of one wrapper. Add `flex-wrap gap-y-2` to the header, give the RunnerBar wrapper `basis-full md:basis-auto` so it claims a full row at `<md`, hide `<Separator />` instances with `hidden md:block`, and apply `order-*` utilities so visual order at `<md` is Logo + ConnectionIndicator (row 1) then RunnerBar + SettingsPopover (row 2). Verify pool overflow behavior with 5+ pools
- `frontend/src/lib/components/RunnerPool.svelte` (modified) — pool label `<span>` gains `truncate` + `max-w-[12ch] md:max-w-none`
- `frontend/e2e/empty-state.test.ts` (new) — appears with zero runs, disappears on first run arrival
- `frontend/e2e/responsive.test.ts` (new) — 1280×800 (3 cols), 900×800 (2 cols), 480×800 (1 col); no horizontal overflow at any width
- `docs/architecture/frontend-app.md` (modified) — add EmptyState component section; add responsive breakpoint contract section

**Dependencies:** Phase 1 (documentation aligned with new direction).

**Done when:** `EmptyState` renders in `connected + 0 runs` state. Hydration placeholder ("Connecting…") branch unchanged. Kanban grid responds to viewport at 1280/900/480px without horizontal scroll. TopBar wraps cleanly at `<768px` with separators hidden and clusters reordered. Pool labels truncate to 12ch under `<md`. All new tests pass; all prior E2E tests still pass.

**ACs covered:** `frontend-1-0-polish.AC1.1`, `frontend-1-0-polish.AC1.2`, `frontend-1-0-polish.AC1.3`, `frontend-1-0-polish.AC1.4`, `frontend-1-0-polish.AC2.1`, `frontend-1-0-polish.AC2.2`, `frontend-1-0-polish.AC2.3`, `frontend-1-0-polish.AC2.4`, `frontend-1-0-polish.AC2.5`.

**Verification note:** During implementation, validate the TopBar wrap feel at `<768px` by hand. The user flagged this as potentially too aggressive. Adjust the breakpoint or strategy if it feels wrong before locking the test thresholds.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Polish audits — reduced motion, focus rings, scrollbars

**Goal:** Close the three audit gaps: gate the one ungated animation, add `:focus-visible` rules to four custom interactive elements, apply cross-browser scrollbar styling.

**Components:**
- `frontend/src/lib/components/CommandPalette.svelte` (modified) — import `prefersReducedMotion`; `transition:slide|local={{ duration: submenuDuration }}` derived
- `frontend/src/lib/components/CommandPalette.reduced-motion.test.ts` (new) — vitest browser-mode; assert duration goes to 0 with `prefers-reduced-motion: reduce` mocked (mock binds before module import)
- `frontend/src/lib/components/KanbanColumn.browser.test.ts` (modified) — rewrite the reduced-motion block (currently around line 281) so the `prefersReducedMotion` mock binds before module import (vitest browser-mode does not have access to Playwright's `emulateMedia`, so the mock-before-import approach is the only viable path here); remove the self-documented "may not affect the loaded module" caveat by making the assertion actually verify zeroed duration
- `frontend/e2e/theme.test.ts` (modified, Playwright) — rewrite `AC1.6` to assert computed `animation-duration: 0s` on a halo'd element under `emulateMedia({ reducedMotion: 'reduce' })` (not just verify the global CSS reset rule exists); extend with submenu-opens-without-delay assertion for the new `CommandPalette` slide gate
- `frontend/src/lib/components/ui/command/command-item.svelte` (modified) — replace `outline-hidden` with `focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2`
- `frontend/src/lib/components/PoolFilterPill.svelte` (modified) — `:focus-visible` rule on clear button
- `frontend/src/lib/components/PanelActions.svelte` (modified) — `:focus-visible` rules on close button and Go-to-run link
- `frontend/src/lib/components/PoolFilterPill.test.ts`, `PanelActions.test.ts` (modified) — assert `outline-style: solid` and `outline-width: 2px` after `el.focus()` + `getComputedStyle`
- `frontend/e2e/focus-rings.test.ts` (new) — Tab-cycles every interactive surface; asserts `outline-width >= 2px || box-shadow !== 'none'` at each stop
- `frontend/src/app.css` (modified) — `.atc-scrollbar` class with cross-browser thumb styling: thumb uses `var(--border)` (alpha-modulated for thumb visibility), track is transparent. `scrollbar-width: thin` + `scrollbar-color: <thumb> transparent` for Firefox; `::-webkit-scrollbar` with `border + background-clip: padding-box` for Chromium/Safari
- `frontend/src/lib/components/KanbanColumn.svelte` (modified) — gains `.atc-scrollbar`
- `frontend/src/lib/components/RunDetailPanel.svelte` (modified) — body container gains `.atc-scrollbar`
- `frontend/src/lib/components/KanbanColumn.test.ts` (modified) — assert class is applied
- `docs/architecture/frontend-app.md` (modified) — add animation inventory matrix; document scrollbar token flow

**Dependencies:** Phase 2 (responsive grid in place; scrollbar is most visible on narrow widths).

**Done when:** `CommandPalette` submenu animation respects reduced motion. All four custom focus targets render visible focus indicators on `:focus-visible`. `KanbanColumn` and `RunDetailPanel` body show themed scrollbars in Chromium/Safari and Firefox. All animation rows in the inventory matrix have at least one reduced-motion-asserting test.

**ACs covered:** `frontend-1-0-polish.AC3.1`, `frontend-1-0-polish.AC3.2`, `frontend-1-0-polish.AC3.3`, `frontend-1-0-polish.AC4.1`, `frontend-1-0-polish.AC4.2`, `frontend-1-0-polish.AC4.3`, `frontend-1-0-polish.AC5.1`, `frontend-1-0-polish.AC5.2`, `frontend-1-0-polish.AC5.3`.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: ARIA live region

**Goal:** Announce run-level column transitions to assistive technology with hybrid coalescing under burst load.

**Components:**
- `frontend/src/lib/aria/transition-kinds.ts` (new) — `TransitionKind` discriminated union (`{kind:'queued'} | {kind:'completed'; conclusion}`); `classifyEvent(event: SeqEvent): TransitionKind | null` returns null for non-announcement events (InProgress, Job-* events) and throws on invariant violation (e.g., Completed `RunEvent` with `conclusion === null`); `VERB_BY_CONCLUSION: Record<RunConclusion, string>` for compile-time exhaustiveness; tsd-style type-level helper `type _CheckExhaustive = Expect<Equal<keyof typeof VERB_BY_CONCLUSION, RunConclusion>>`
- `frontend/src/lib/aria/transition-kinds.test.ts` (new) — exhaustive verb table; type-level test using vitest's `expectTypeOf` or a `tsd`-style helper; classification cases including null-conclusion-with-Completed throws; non-announcement events return null without throwing
- `frontend/src/lib/aria/format-run-transition.ts` (new) — pure builder: `(run: WorkflowRun, transitionKind: TransitionKind) => string` with null-branch elision; conclusion-specific verbs from `VERB_BY_CONCLUSION`
- `frontend/src/lib/aria/format-run-transition.test.ts` (new) — message format per transition kind, branch elision, conclusion-specific verbs
- `frontend/src/lib/aria/live-region.svelte.ts` (new) — `LiveRegion` rune-class with `observeFlush(events: ReadonlyArray<SeqEvent>): void`; per-event try/catch around `classifyEvent` (logs+skips on throw, continues with remaining events); `BurstAccumulator` opens when >3 announcement-relevant transitions surface in a single flush, debounces 200ms, accumulates counts across every flush within the window (regardless of per-flush count); per-run vs summary message construction with `aria-busy` toggling; `liveRegion` module-level singleton
- `frontend/src/lib/aria/live-region.test.ts` (new) — observeFlush event walking (Queued and Completed counted; InProgress skipped; same-run Queued+Completed in one flush produces 2 announcements); threshold-based per-run-vs-summary switch; debounce behavior with `vi.useFakeTimers`; multi-flush burst aggregation (a 4-event flush opens burst; a subsequent 2-event flush within debounce contributes to counts); aria-busy true→false transition; reset on debounce close; per-event error containment (one bad event doesn't kill the rest of the batch's announcement)
- `frontend/src/lib/components/AriaLiveRegion.svelte` (new) — connected component reading `liveRegion`; renders `<div role="status" aria-live="polite" aria-atomic="true" aria-busy={liveRegion.busy ? 'true' : 'false'} aria-label="Workflow run updates" class="sr-only">{liveRegion.message}</div>`
- `frontend/src/lib/components/AriaLiveRegion.test.ts` (new) — role, aria-live, aria-atomic, aria-busy reflection (initial "false"; flips to "true" when liveRegion.busy is true), sr-only class
- `frontend/src/lib/dispatcher.ts` (modified) — `setOnFlush(cb: (events: ReadonlyArray<SeqEvent>) => void): void` method; `processBuffer` invokes `cb(events)` only when `events.length > 0` (empty drains do not trigger the callback); `flush()` cancels any pending RAF (`cancelAnimationFrame(this.rafId)` + `this.rafId = null`) before draining so a `dispatch(); flush();` pair produces exactly one non-empty callback rather than a real callback followed by a phantom empty-array callback when the queued RAF later fires. Idempotent: setting twice replaces the prior callback; clearing via `setOnFlush(null)` (or equivalent) detaches the callback so reconnect sequences can suppress announcements during buffered drain (see AC6.7). Calling without ever setting is a no-op
- `frontend/src/lib/dispatcher.test.ts` (modified) — assert `setOnFlush` callback is invoked once per RAF flush with the flushed event list; assert no invocation when `setOnFlush` was never called; assert callback receives only the events from the current flush (not cumulative); assert `dispatch(); flush();` produces exactly one non-empty callback (no phantom empty call from the cancelled RAF); assert detaching the callback (e.g., `setOnFlush(null)`) suppresses subsequent invocations
- `frontend/src/lib/connection.ts` (modified) — wires `dispatcher.setOnFlush((events) => liveRegion.observeFlush(events))` AFTER the post-snapshot buffered drain at line 117 completes (deferred wiring; not at construction time). On disconnect / reconnect, the wiring is detached (`dispatcher.setOnFlush(null)`) so the next snapshot+buffered-drain sequence runs silently; re-wired after the new drain completes. Snapshot loads continue to use `runStore.loadSnapshot` directly and bypass the dispatcher path
- `frontend/src/main.ts` (modified) — exposes `window.eventDispatcher` inside the existing `if (import.meta.env.DEV) { ... }` bridge alongside `window.__stores` (DEV-only; no-op in production builds). The exposure is via the same untyped pattern as `__stores` to keep the dev-bridge ergonomics consistent
- `frontend/src/vite-env.d.ts` (modified) — extend the Window augmentation: `eventDispatcher?: typeof import('$lib/dispatcher').eventDispatcher` (optional because production builds do not expose it)
- `frontend/e2e/lib/ws-mock.ts` (modified) — `sendWS` switches from direct store calls (`runStore.applyRunEvent`, `runStore.applyJobEvent`, `runnerStore.loadPools`) to `window.eventDispatcher.dispatch(seqEvent); window.eventDispatcher.flush()`. The `dispatch + flush` pair preserves the existing sync-after-await semantics so the nine pre-existing E2E files (`kanban`, `run-cards`, `pool-indicators`, `pool-filter`, `palette`, `run-detail-panel`, `run-card-interactivity`, `kanban-keyboard-nav`, `stacking`) need no source changes. `poolStatsAfter` handling moves out of ws-mock into the dispatcher's existing `routeEvent` switch (`dispatcher.ts:49-51` already does this), removing the duplicate write path. New helper `sendWSBatch(page, msgs[])` calls `dispatch(...)` for each event WITHOUT calling `flush()`, then awaits a synchronization fence — explicitly: `await page.waitForFunction(() => window.eventDispatcher.bufferLength === 0)` (a new read-only `bufferLength` getter on the dispatcher exposes drain state) followed by `await page.evaluate(() => new Promise(r => requestAnimationFrame(() => r(undefined))))` to ensure the post-flush callback has had at least one tick to run. This way tests have a deterministic fence for "the burst has been processed and any aria-busy debounce has had time to start." Used by `aria-live.test.ts` (Phase 4) and `frame-budget.test.ts` (Phase 5) for burst testing
- `frontend/src/App.svelte` (modified) — mounts `<AriaLiveRegion />` as a sibling to `<AppShell />`
- `frontend/e2e/aria-live.test.ts` (new) — inject WS events at known cadences via `e2e/lib/ws-mock` (now routed through the real dispatcher); assert `aria-busy` flips `false→true→false` during a synthetic burst; assert `textContent` matches expected per-run messages below threshold and summary message above; assert summary counts span multi-flush bursts (not just the final tick); assert role="status" + aria-atomic="true" attributes
- `docs/architecture/frontend-app.md` (modified) — add AriaLiveRegion module section; document the `setOnFlush` callback contract; document the snapshot-bypass / reconnect-silent policy

**Dependencies:** Phase 1 (docs aligned). Independent of Phases 2 and 3 functionally — Phase 4 could land before either. Ordered after Phase 3 as a sequencing/review preference (keeps the largest, highest-risk phase reviewable in isolation), not a true dependency.

**Done when:** Below 3 transitions per flush: per-run announcements with full message format. Above 3: summary form covering all transitions across the burst window, debounced 200ms. `RunConclusion` exhaustiveness enforced at compile time. Off-shape events throw in `classifyEvent`; `LiveRegion.observeFlush` logs + skips per-event without contaminating the batch. ws-mock harness routes through real dispatcher (so `aria-busy` and RAF batching are observable in E2E). All new tests pass. **Regression gate:** the full pre-existing E2E suite (9 files) passes after the ws-mock refactor lands but before `AriaLiveRegion` is wired in — verifies that the `dispatch + flush` migration is behavior-preserving in isolation.

**ACs covered:** `frontend-1-0-polish.AC6.1`, `frontend-1-0-polish.AC6.2`, `frontend-1-0-polish.AC6.3`, `frontend-1-0-polish.AC6.4`, `frontend-1-0-polish.AC6.5`, `frontend-1-0-polish.AC6.6`, `frontend-1-0-polish.AC6.7`.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Performance verification and final regression

**Goal:** Add the two-tier performance guard and run the full E2E regression to confirm nothing earlier in the bundle regressed.

**Components:**
- `frontend/src/lib/dispatcher.perf.test.ts` (new, vitest browser-mode) — uses `vi.useFakeTimers()` and a manually-driven RAF queue. Enqueues 1000 mock events; advances RAF in N controlled ticks (test author chooses N to model realistic batching, e.g., 10 ticks of 100 events each). Asserts `flushSpy.mock.calls.length === N` (deterministic, not bounded). Asserts `runStore.totalRuns === 1000` reflects every event landed in store state. Asserts `processedCount === 1000`. Real timers are not used; wall-clock flake is eliminated by construction
- `frontend/e2e/frame-budget.test.ts` (new, Playwright) — `page.tracing.start({ categories: ['rendering'] })`; fires 1000 WS events through `e2e/lib/ws-mock` (now routed through the real dispatcher per Phase 4); parses BeginFrame deltas from trace JSON; saves trace as CI artifact (`frame-budget-trace.json`); logs frame-budget summary in a structured format (e.g., JSON line with `p50_ms`, `p95_ms`, `dropped_frames`) so future tightening is mechanical; test always passes (informational)
- `justfile` (modified) — `test-perf` recipe runs Tier 1 + Tier 2 locally; existing `test` recipe includes Tier 1 (vitest); existing `test-e2e` recipe includes Tier 2
- `.github/workflows/ci.yml` (modified, if needed) — upload `frame-budget-trace.json` as a CI artifact (the existing artifact-upload step may already cover this; verify)
- `docs/architecture/frontend-app.md` (modified) — finalize cross-cutting bits not landed in earlier phases (the EmptyState/responsive sections land in Phase 2; the AriaLiveRegion module in Phase 4; this phase only adds the perf-verification methodology section)
- `frontend/CLAUDE.md` (modified) — update Sub-Phase status; reference new live-region module
- `CLAUDE.md` (root, modified) — update top-level frontend status to reflect 1.0 readiness
- `docs/ideation/ui-decomposition/README.md` (modified) — Sub-Phase 6b section header marked "✅ COMPLETE" with what shipped (replacing the "in progress" mark from Phase 1)

**Dependencies:** Phases 1-4. This is the closing phase.

**Done when:** Tier 1 perf test passes deterministically in CI (no wall-clock dependency). Tier 2 produces a trace artifact uploaded by CI. Full prior-sub-phase E2E regression passes. All architecture docs and `CLAUDE.md` files updated. The `ui-decomposition/README.md` SP6b section reads as a "what shipped" record matching the format used for SP1-SP6a.

**Note on doc phasing:** The pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) is branch-scoped (diffs `merge-base origin/main..HEAD`), not per-commit. As long as the branch is pushed once with all phase changes present, the gate passes. EmptyState/responsive doc deltas land in Phase 2; AriaLiveRegion in Phase 4; perf methodology in Phase 5 — all consistent with the gate's branch-level evaluation.

**ACs covered:** `frontend-1-0-polish.AC7.1`, `frontend-1-0-polish.AC7.2`, `frontend-1-0-polish.AC9.1`, `frontend-1-0-polish.AC10.1`, `frontend-1-0-polish.AC10.2`.
<!-- END_PHASE_5 -->

## Additional Considerations

### Documents to Update

| Document | Reason |
|---|---|
| `.impeccable.md` | Wall-display reframe (line 6) |
| `docs/ideation/ui-decomposition/component-patterns.md` | 3 wall-display rewrites/deletions |
| `docs/ideation/ui-decomposition/README.md` | 2 wall-display rewrites; SP6b contract revision (drop mobile tabs, adopt 640/1280 cascade); SP6b section marked "in progress" in Phase 1, "✅ COMPLETE" in Phase 5 |
| `docs/ideation/design-research.md` | 2 wall-display rewrites |
| `docs/design-plans/2026-04-25-interactivity.md` | "Revised by" annotations on lines 30 and 375 |
| `docs/architecture/frontend-app.md` | Add EmptyState component (Phase 2), responsive breakpoint contract (Phase 2), animation inventory matrix (Phase 3), AriaLiveRegion module + setOnFlush callback contract + snapshot-bypass policy (Phase 4), perf-verification methodology (Phase 5) |
| `frontend/CLAUDE.md` | Update Sub-Phase status; reference new live-region module |
| `CLAUDE.md` (root) | Update top-level frontend status to reflect 1.0 readiness |
| `frontend/src/main.ts` | Expose `window.eventDispatcher` for E2E harness (Phase 4) |
| `frontend/src/vite-env.d.ts` | Window augmentation for `eventDispatcher` (Phase 4) |
| `frontend/e2e/lib/ws-mock.ts` | Route synthetic events through `window.eventDispatcher` so E2E exercises real RAF batching and aria-busy behavior (Phase 4) |

### Edge cases

- **`RunConclusion` is null when a `RunEvent::Completed` arrives.** Should not happen at the wire level (the backend always populates conclusion on completion, and ts-rs types reflect this). If it does (off-shape input), `classifyEvent` throws; `LiveRegion.observeFlush` per-event try/catch logs the violation via `console.error` and skips the offending update. Other transitions in the same flush continue to announce.
- **Initial snapshot vs incremental updates.** The `ConnectionManager` loads initial WS snapshots directly into stores via `runStore.loadSnapshot` (`connection.ts:106`), bypassing the dispatcher entirely. The post-snapshot buffered drain (`connection.ts:111-117`) DOES dispatch buffered events through `eventDispatcher.dispatch + flush`, but the `setOnFlush` callback is not yet wired at that point — `ConnectionManager` defers wiring until after the buffered drain completes. Net result: no announcements fire during snapshot load OR buffered-replay drain. Only events arriving live (after the wiring) announce.
- **Reconnect.** Same sequence as initial connect: on disconnect, `ConnectionManager` detaches the wiring (`dispatcher.setOnFlush(null)`); on reconnect, it loads the new snapshot, drains any buffered events silently, then re-attaches the wiring. State changes during downtime are visible in the kanban but not announced. The reconnect policy is silent by construction. (Future enhancement could add a "Reconnected. N runs changed during downtime." announcement; explicitly out of 1.0 scope.)
- **TTL eviction.** When a completed run is evicted by TTL, it disappears from the store but produces no `RunEvent`; therefore `observeFlush` never sees an "eviction" event and announces nothing. Correct behavior.
- **Same-flush Queued→Completed for one run.** If a single flush contains both a `RunEvent::Requested` and a subsequent `RunEvent::Completed` for the same run (rare but possible under burst load), `LiveRegion.observeFlush` walks the events in order and announces both transitions: "Run X queued. Run X succeeded." (or summary form if total transition count exceeds threshold).
- **Non-overlapping bursts.** If a burst closes (debounce fires, summary announced, `aria-busy` flips back to `false`) and then 5+ transitions arrive in a fresh flush, a new burst opens. Each burst summary is independent; counts do not carry across.
- **Prefers-reduced-motion changes mid-session.** The `prefersReducedMotion.current` rune is reactive — flipping the OS setting mid-session updates the gate without a reload. The `CommandPalette` submenu duration, the kanban transitions, and the halo CSS reset all respond.
- **Container width at exact breakpoints.** Tailwind v4's `sm:` (≥640px) and `xl:` (≥1280px) are inclusive at the lower bound. At exactly 640px the kanban renders 2 columns; at exactly 1280px it renders 3.

### Future extensibility

- **Container queries** become a viable upgrade if a future sidebar or split-view feature shrinks the kanban's effective width independently of the viewport. The migration is mechanical: wrap the kanban in a `@container` and swap `sm:` / `xl:` for `@sm:` / `@xl:`. No architectural change.
- **Filter-empty EmptyState.** When pool filtering is active and excludes all runs, the kanban currently renders empty columns (no `<EmptyState />`). A future enhancement could switch to a filter-aware empty state ("No runs match the current pool filter"). Out of scope here.
- **Detailed transition messages.** The `formatRunTransition` builder is a pure function; future enhancements (e.g., adding job count or duration to the message) are mechanical edits without architectural impact.
- **Additional `RunConclusion` variants.** Adding a new conclusion in `atc-core` triggers ts-rs regeneration, which fails the frontend type-check until `VERB_BY_CONCLUSION` adds the new key. The contract enforces this without runtime tests.

