# Frontend 1.0 Polish Design

## Summary

Sub-Phase 6b layers six independent concerns onto the existing frontend without restructuring the component tree, without introducing new global stores, and without changing the architectural patterns established across Sub-Phases 1–6a. The five-store ceiling (RunStore, RunnerStore, ConnectionStore, UIStore, PaletteStore) is preserved: the `LiveRegion` rune-class is a module-scope singleton in `lib/aria/live-region.svelte.ts`, not a sixth store. Similarly, the responsive grid change is purely a Tailwind v4 class cascade swap on `KanbanBoard.svelte` — no new layout primitives, no container queries (the detail panel uses Bits UI Sheet overlay-style, so kanban width tracks the viewport directly). Documentation cleanup lands first, in a no-functional-change phase that rewrites seven structural "wall display" occurrences before any component code ships.

The most architecturally novel piece is the ARIA live region. `EventDispatcher` gains a `setOnFlush` post-flush callback hook: before each RAF flush the dispatcher snapshots the three column arrays' run IDs and statuses, applies batched events, then invokes the callback with the pre- and post-state pair. `LiveRegion.observeFlush` diffs those snapshots and routes output through either a per-run message path (≤3 transitions per RAF) or a `BurstAccumulator` (>3 transitions, 200 ms debounce spanning the entire burst window). Transition classification is driven by ts-rs-generated `RunStatus` and `RunConclusion` types: a `Record<RunConclusion, string>` verb dictionary gives compile-time exhaustiveness, and the `RunStatus` switch uses `const _: never = next` for runtime safety at off-shape wire boundaries. The remaining concerns — reduced-motion gate on the one ungated `CommandPalette` slide, `:focus-visible` rules on four custom interactive elements, and a global `.atc-scrollbar` class in `app.css` — are surgical point fixes with no cross-cutting impact.

## Definition of Done

Sub-Phase 6b is the final frontend sub-phase before the bundled 1.0 release. It ships when, on the merged feature branch:

1. **EmptyState component exists** as a real, pure Svelte component, replacing the inline "No workflows yet." string in `KanbanBoard`. Visual treatment to be designed during brainstorming. Tri-state predicate (connecting / connected-empty / populated) preserved.

2. **Kanban degrades responsively below 1024px** with no horizontal page scroll at any width ≥320px. Approach (two-step breakpoints vs container queries) chosen during brainstorming and prototyped. TopBar/RunnerBar adapt without overflow.

3. **Reduced-motion audit complete:** the one ungated animation (CommandPalette theme submenu slide on `CommandPalette.svelte:218`) is gated; every animation has at least one test asserting reduced-motion behavior; audit findings recorded in this design doc.

4. **Scrollbar styling applied** to kanban columns and panel body. Cross-browser via `::-webkit-scrollbar` + `scrollbar-width`/`scrollbar-color`. Palette continues to hide scrollbars.

5. **Focus rings: every interactive element has a visible `:focus-visible` rule.** Specifically: `command-item`'s `outline: hidden` is replaced with a visible focus indicator; `PoolFilterPill` clear button, `PanelActions` close button, and the Go-to-run link gain explicit `:focus-visible` rules. Token uniformity (3px ring vs 2px outline) explicitly documented as out-of-scope.

6. **ARIA live region announces run-level column transitions** with messages of the form "Run {displayTitle} for {org}/{repo} on {branch} ({trigger}) {queued|started|completed|failed|cancelled|timed-out}". Hybrid coalescing: per-run announcements below burst threshold, summary form ("N runs started, M completed") above threshold. Single live element near the kanban root; messages derived from existing `RunStore` data via `EventDispatcher`. No new store.

7. **Performance verification under a 1000-event burst:** RAF-coalescing assertion gates CI as a hard fail (instrumented `EventDispatcher` flush count under burst load). Frame-timing trace runs as an informational/non-blocking artifact.

8. **"Wall display" framing removed** from 7 structural occurrences across `.impeccable.md`, `docs/ideation/ui-decomposition/component-patterns.md`, `docs/ideation/ui-decomposition/README.md`, `docs/design-plans/2026-04-25-interactivity.md`, and `docs/ideation/design-research.md`. Concourse competitive-research mentions stay (they describe Concourse's product, not ATC's).

9. **All prior sub-phase E2E tests still pass** (full regression).

10. **Architecture docs and the SP6b section of `docs/ideation/ui-decomposition/README.md` updated** to reflect what shipped.

**Out of scope:**

- Mobile-targeted layouts (no tab navigation, no <640px-specific design)
- Touch-device manual verification from Sub-Phase 5
- Log fetching for the detail panel (issue #36)
- Focus-token uniformity across shadcn-vs-custom components
- EmptyState variants for filtered states (e.g., "no runs match this pool filter")

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

- **frontend-1-0-polish.AC6.1 Success:** A single `<div role="log" aria-live="polite" aria-busy aria-label="Workflow run updates" class="sr-only">` mounts in the DOM at App.svelte level (sibling to AppShell).
- **frontend-1-0-polish.AC6.2 Success:** When ≤3 column transitions occur in a single RAF flush, the live region's text content reads the per-run messages joined with `". "` in the form `"Run {displayTitle} for {org}/{repo} on {branch} ({event}) {verb}"`; `aria-busy` is `false`.
- **frontend-1-0-polish.AC6.3 Success:** When >3 column transitions occur in a RAF flush, `aria-busy` becomes `true`; the BurstAccumulator opens with a 200ms debounce; subsequent flushes during the window add their transitions into accumulated counts; on debounce close, the live region announces a single summary message of the form "N runs started, M completed, K failed." that covers ALL transitions during the entire burst window (not just the final flush); `aria-busy` becomes `false`.
- **frontend-1-0-polish.AC6.4 Success:** When `WorkflowRun.branch` is null, the per-run message elides the "on {branch}" segment instead of rendering "on null".
- **frontend-1-0-polish.AC6.5 Success:** Adding a new `RunConclusion` variant in `atc-core` and regenerating ts-rs types fails the frontend `tsc` step until `VERB_BY_CONCLUSION` adds a verb for the new variant (compile-time exhaustiveness via `Record<RunConclusion, string>`). A type-level test asserts `keyof typeof VERB_BY_CONCLUSION === RunConclusion`.
- **frontend-1-0-polish.AC6.6 Failure:** When a `Completed` event arrives with `conclusion === null`, `classifyTransition` throws an explicit error rather than silently classifying or falling through.
- **frontend-1-0-polish.AC6.7 Edge:** The initial WS snapshot drain (state hydration on first connect) does NOT generate live-region announcements; only true status transitions after the first snapshot do.

### frontend-1-0-polish.AC7: Performance verification

- **frontend-1-0-polish.AC7.1 Success:** A vitest browser-mode test fires 1000 mock events spread across `setTimeout(..., Math.floor(i/10))` (~100ms wall time) through `EventDispatcher`; asserts `flushSpy.mock.calls.length <= Math.ceil(elapsedMs / 16.67) + 2` and `processedCount === 1000`. Failure is a CI hard fail.
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
- **RunStatus**: The ts-rs-generated TypeScript union type representing a workflow run's current column state (Queued, InProgress, Completed). The `classifyTransition` function switches exhaustively over this type.
- **RunConclusion**: The ts-rs-generated TypeScript union type representing the terminal outcome of a completed run (success, failure, cancelled, timed_out, etc.). `VERB_BY_CONCLUSION` is keyed by this type for compile-time exhaustiveness.
- **WorkflowRun**: The core domain type representing a GitHub Actions workflow run. The ARIA live region reads `WorkflowRun.branch`, `WorkflowRun.displayTitle`, and conclusion fields when composing announcement messages.
- **EventDispatcher**: The frontend class in `lib/connection/event-dispatcher.ts` that receives batched WebSocket events and flushes them to the RunStore once per RAF. This phase adds a `setOnFlush` hook to it.
- **ConnectionManager**: The class in `lib/connection/connection-manager.ts` that owns the WebSocket lifecycle and wires `EventDispatcher` to the stores. This phase adds the one-time `setOnFlush` wiring call at construction time.
- **BurstAccumulator**: The internal state object within `LiveRegion` that aggregates transition counts across multiple RAF flushes when the per-flush count exceeds the burst threshold (3). Debounces over 200 ms; counts span the entire burst window.
- **prefers-reduced-motion**: CSS media query and OS accessibility setting that signals the user prefers minimal animation. Svelte exposes it as `prefersReducedMotion.current` from `svelte/motion`. This phase gates the one remaining ungated animation (`CommandPalette` theme submenu slide) and adds tests for every animation in the inventory table.
- **`role="log"`**: ARIA landmark role for a live region that accumulates new messages over time; implies `aria-live="polite"` by default. The `AriaLiveRegion` component uses this role explicitly alongside `aria-live="polite"`.
- **`aria-busy`**: ARIA attribute set to `true` during the BurstAccumulator's debounce window, signaling to screen readers that the region is mid-update and they should defer announcement until it resolves.
- **`aria-live`**: ARIA attribute that designates an element as a live region. Set to `"polite"` so screen readers finish the current utterance before announcing transitions.
- **Viewport breakpoints**: Fixed-width thresholds used for the responsive grid. This design uses Tailwind v4's `sm:` (≥640px) and `xl:` (≥1280px) rather than container queries, because the detail panel uses overlay-style positioning and does not affect kanban width.
- **Container queries**: A CSS feature that sizes layout relative to a containing element's width rather than the viewport. Explicitly evaluated and deferred for this phase; the responsive contract uses viewport breakpoints. Documented as a future migration path.
- **EmptyState (schematic preview)**: The visual treatment of the `EmptyState.svelte` component when connected with zero runs: three faint dashed column groups labeled Queued / Running / Completed, each containing three rows of monospace placeholder dots, with a "Watching for runs." caption below.
- **ws-mock harness**: The E2E test utility at `e2e/lib/ws-mock.ts` (`makeRunEvent`, `makeJobSeqEvent`, `sendWS`) that injects synthetic WebSocket events into the browser page during Playwright tests. Used in the performance verification and ARIA live region E2E tests.
- **LiveRegion**: The project's module in `lib/aria/live-region.svelte.ts` — a rune-class singleton (not a global store) that owns `message` and `busy` state and exposes an `observeFlush` method. Distinct from the generic ARIA `role="log"` concept.
- **Sub-Phase 6b**: The final frontend sub-phase before the 1.0 release bundle. Covers EmptyState, responsive layout, ARIA live region, performance verification, polish audits, and documentation cleanup. The preceding standalone deliverable was Sub-Phase 6a (kanban keyboard navigation).
- **Five-store ceiling**: The architectural discipline that limits global Svelte stores to exactly five: RunStore, RunnerStore, ConnectionStore, UIStore, PaletteStore. `LiveRegion` is deliberately implemented as a module-scope rune-class singleton rather than a sixth store to preserve this invariant.
- **Documents to Update table**: A mandatory section in every ATC design plan enumerating every file that must be modified alongside the implementation, per project documentation conventions. Ensures architecture docs stay in sync with code changes.

## Architecture

Sub-Phase 6b layers six concerns onto the existing frontend without restructuring the component tree:

1. **EmptyState extraction.** A new pure component `EmptyState.svelte` replaces the inline `"No workflows yet."` string in `KanbanBoard.svelte`. The schematic-preview treatment (three faint dashed columns with monospace placeholder rows + "Watching for runs." caption) sits inside the existing tri-state predicate (`connectionStore.status === 'connected' && totalRuns === 0`).

2. **Responsive grid.** `KanbanBoard.svelte`'s hardcoded `grid-cols-3` becomes a Tailwind v4 mobile-first cascade: `grid-cols-1 sm:grid-cols-2 xl:grid-cols-3`. The detail panel uses Bits UI Sheet overlay-style (does not shrink the kanban), so viewport breakpoints are sufficient — container queries would adapt to nothing the kanban cares about today. TopBar wraps at `<md` via `flex-wrap`; RunnerBar pool labels truncate to `12ch`.

3. **ARIA live region.** A new `lib/aria/live-region.svelte.ts` module owns a `LiveRegion` rune-class instance (module-scope; not a sixth global store). The companion `AriaLiveRegion.svelte` component mounts at `App.svelte` level as a sibling to `<AppShell>`, rendering a single `<div role="log" aria-live="polite" aria-busy={...} aria-label="Workflow run updates" class="sr-only">`. `EventDispatcher` gains a post-flush callback hook (`setOnFlush`) — before each RAF flush, the dispatcher captures a pre-state snapshot of the three column arrays' Run IDs and statuses; after applying batched events, it captures a post-state snapshot and invokes the callback. `LiveRegion.observeFlush` diffs the snapshots, classifies transitions via a ts-rs-driven exhaustive switch over `RunStatus`, and emits either per-run messages (≤3 transitions per RAF) or accumulates into a `BurstAccumulator` for summary form (>3 transitions, debounced over 200ms with counts spanning the entire burst window).

4. **Performance verification.** Two-tier metric. Tier 1 is a vitest browser-mode test that fires 1000 mock events through `EventDispatcher` over ~100ms wall time and asserts RAF flush count is bounded by elapsed frames + slack. Hard CI gate. Tier 2 is a Playwright test that records a `page.tracing` artifact during a synthesized 1000-event burst; saves the trace as a CI artifact and logs frame-budget summary; never fails (informational).

5. **Polish audits.** Reduced-motion: one ungated animation (`CommandPalette.svelte:218`'s theme submenu slide) gets the `prefersReducedMotion.current` gate matching `kanban-transitions.ts`. Focus rings: four custom interactive elements (`command-item`, `PoolFilterPill` clear, `PanelActions` close, `PanelActions` Go-to-run) gain explicit `:focus-visible` rules adopting `RunCard.svelte:240`'s 2px+2px outline pattern. Scrollbars: a global `.atc-scrollbar` class in `app.css` provides cross-browser thin-thumb styling (`scrollbar-width`/`scrollbar-color` for Firefox, `::-webkit-scrollbar` with `border + background-clip: padding-box` for Chromium/Safari per the Rauno Freiberg pattern), applied to `KanbanColumn` and `RunDetailPanel` body.

6. **"Wall display" framing cleanup.** Seven structural occurrences across `.impeccable.md`, `component-patterns.md`, `ui-decomposition/README.md`, `interactivity.md`, and `design-research.md` are surgically rewritten to keep each sentence's actual point while replacing the wall-display justification with the real driver (operator at workstation, motion contrast, etc.). The shipped `2026-04-25-interactivity.md` design plan receives a one-line "Revised by" annotation rather than silent rewrites — the supersession is traceable, the original prose stays intact.

### Module and component boundaries

```
frontend/src/lib/
  aria/                       (new)
    live-region.svelte.ts     LiveRegion rune-class + module singleton + BurstAccumulator
    format-run-transition.ts  Pure function: (run, prev, next) → message string
    transition-kinds.ts       TransitionKind type + classifyTransition + VERB_BY_CONCLUSION
  components/
    EmptyState.svelte         (new) pure component
    AriaLiveRegion.svelte     (new) connected component (subscribes to liveRegion)
    KanbanBoard.svelte        (modified) inline "No workflows yet." → <EmptyState />, grid cascade
    KanbanColumn.svelte       (modified) gains .atc-scrollbar
    CommandPalette.svelte     (modified) reduced-motion gate on submenu slide
    PoolFilterPill.svelte     (modified) :focus-visible rule on clear button
    PanelActions.svelte       (modified) :focus-visible rules on close + Go-to-run
    RunDetailPanel.svelte     (modified) gains .atc-scrollbar on body
    ui/command/command-item.svelte (modified) outline-hidden → focus-visible:outline-*
  connection/
    event-dispatcher.ts       (modified) gains setOnFlush() + pre/post snapshot capture
App.svelte                    (modified) mount <AriaLiveRegion /> sibling to <AppShell />
src/app.css                   (modified) .atc-scrollbar global class
```

### Live region message contract

```typescript
// transition-kinds.ts
import type { RunStatus, RunConclusion } from '$lib/types/generated';

export type TransitionKind =
  | 'queued'
  | 'started'
  | { kind: 'completed'; conclusion: RunConclusion };

export function classifyTransition(
  next: RunStatus,
  conclusion: RunConclusion | null,
): TransitionKind;

export const VERB_BY_CONCLUSION: Record<RunConclusion, string>;
```

```typescript
// live-region.svelte.ts
export class LiveRegion {
  message: string = $state('');
  busy: boolean = $state(false);
  observeFlush(prev: RunStateSnapshot, next: RunStateSnapshot): void;
}
export const liveRegion: LiveRegion;

export interface RunStateSnapshot {
  byId: Map<bigint, { status: RunStatus; conclusion: RunConclusion | null }>;
}

export interface BurstAccumulator {
  active: boolean;
  startedAtMs: number;
  counts: Record<string, number>;  // keyed by TransitionKind discriminant
}
```

```typescript
// event-dispatcher.ts (modified)
export class EventDispatcher {
  setOnFlush(cb: (prev: RunStateSnapshot, next: RunStateSnapshot) => void): void;
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
- **Accessibility-first selectors in tests.** `getByRole('log')`, `getByLabelText('Workflow run updates')` rather than `getByTestId`.
- **Exported props interface.** `export interface EmptyStateProps { message?: string }`.
- **Five-store ceiling preserved.** `LiveRegion` is a module-scope rune-class singleton (`lib/aria/live-region.svelte.ts`), not a global store. The five-store discipline (RunStore, RunnerStore, ConnectionStore, UIStore, PaletteStore) is unchanged.
- **`prefersReducedMotion.current` from `svelte/motion`.** Matches the existing pattern in `lib/animations/kanban-transitions.ts`. The `CommandPalette` slide gate uses the same import path and reactivity shape.
- **ts-rs-generated types as source of truth** (per `feedback_exhaustive_switches_at_boundaries`). `RunStatus` and `RunConclusion` are generated from the Rust `atc-core` crate via `just types`. Verb dictionary uses `Record<RunConclusion, string>` for compile-time exhaustiveness; the classification switch uses `const _: never = next` to catch missed `RunStatus` variants.
- **2px+2px outline focus-ring pattern** for new custom focus indicators (per `RunCard.svelte:240`). New focus rules adopt this; shadcn-derived components retain their 3px box-shadow ring (token uniformity is OOS).
- **Documents to Update table** per project guidance #6 (`.ed3d/design-plan-guidance.md`). Listed below in Additional Considerations.
- **Crossfade-pair-with-fly-fallback animation pattern** is unchanged. The existing `kanban-transitions.ts` module gates on `prefersReducedMotion.current` once and exports zeroed durations under reduced motion.
- **WS-driven projection-of-state pattern.** Live region announcements are computed from store snapshot diffs at the dispatcher's RAF flush boundary — not from store mutation side effects. Stores remain pure derived projections of server state.

No new patterns are introduced. The design adds infrastructure (post-flush hook on `EventDispatcher`, ts-rs-driven exhaustive switch, schematic preview empty state) within the existing architectural boundaries.

## Implementation Phases

Five phases, ordered by risk and reviewability. The bulk of the implementation effort is Phase 4 (ARIA live region).

<!-- START_PHASE_1 -->
### Phase 1: Documentation cleanup and scaffolding

**Goal:** Remove the "wall display" framing across the codebase and establish the design plan's "Documents to Update" scaffolding before any functional code lands.

**Components:**
- `.impeccable.md` line 6 — wall-display reframe to operator-at-workstation
- `docs/ideation/ui-decomposition/component-patterns.md` lines 34, 36, 186 — three rewrites/deletions
- `docs/ideation/ui-decomposition/README.md` lines 217, 379 — two rewrites; SP6b section header marked "in progress"
- `docs/ideation/design-research.md` lines 119, 134 — two rewrites
- `docs/design-plans/2026-04-25-interactivity.md` lines 30, 375 — "Revised by" annotations; original prose preserved
- `scripts/doc-mapping.sh` — add mappings for new files (`EmptyState.svelte`, `AriaLiveRegion.svelte`, `live-region.svelte.ts`) → `docs/architecture/frontend-app.md`

**Dependencies:** None (first phase).

**Done when:** All seven structural wall-display occurrences are rewritten or deleted per the table in `## Architecture`. Concourse competitive-research mentions in `design-research.md` (lines 28, 70, 88, 92, 101, 158, 219) are explicitly preserved. `scripts/doc-mapping.sh` includes the new file mappings. Build and existing tests still pass.

**ACs covered:** `frontend-1-0-polish.AC8.1`, `frontend-1-0-polish.AC8.2`, `frontend-1-0-polish.AC8.3`.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: EmptyState component and responsive layout

**Goal:** Extract the inline empty-state string into a pure component with the schematic-preview treatment; introduce responsive breakpoints across the kanban grid and TopBar.

**Components:**
- `frontend/src/lib/components/EmptyState.svelte` (new) — pure component with `message?: string` prop, schematic preview rendering
- `frontend/src/lib/components/EmptyState.test.ts` (new) — schematic structure, caption rendering, prop override
- `frontend/src/lib/components/KanbanBoard.svelte` (modified) — replaces inline `"No workflows yet."` with `<EmptyState />`; grid class becomes `grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4`; container gains `min-width: 0`
- `frontend/src/lib/components/AppShell.svelte` (modified) — TopBar slot gains `flex-wrap` + `gap-y-2`
- `frontend/src/lib/components/RunnerBar.svelte` (modified) — pool labels gain `truncate` + `max-w-[12ch]` at `<md`
- `frontend/e2e/empty-state.test.ts` (new) — appears with zero runs, disappears on first run arrival
- `frontend/e2e/responsive.test.ts` (new) — 1280×800 (3 cols), 900×800 (2 cols), 480×800 (1 col); no horizontal overflow

**Dependencies:** Phase 1 (documentation aligned with new direction).

**Done when:** `EmptyState` renders in `connected + 0 runs` state. Hydration placeholder ("Connecting…") branch unchanged. Kanban grid responds to viewport at 1280/900/480px without horizontal scroll. TopBar wraps cleanly at `<768px`. All new tests pass; all prior E2E tests still pass.

**ACs covered:** `frontend-1-0-polish.AC1.1`, `frontend-1-0-polish.AC1.2`, `frontend-1-0-polish.AC1.3`, `frontend-1-0-polish.AC1.4`, `frontend-1-0-polish.AC2.1`, `frontend-1-0-polish.AC2.2`, `frontend-1-0-polish.AC2.3`, `frontend-1-0-polish.AC2.4`, `frontend-1-0-polish.AC2.5`.

**Verification note:** During implementation, validate the TopBar wrap feel at `<768px` by hand. The user flagged this as potentially too aggressive. Adjust the breakpoint or strategy if it feels wrong before locking the test thresholds.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Polish audits — reduced motion, focus rings, scrollbars

**Goal:** Close the three audit gaps: gate the one ungated animation, add `:focus-visible` rules to four custom interactive elements, apply cross-browser scrollbar styling.

**Components:**
- `frontend/src/lib/components/CommandPalette.svelte` (modified) — import `prefersReducedMotion`; `transition:slide|local={{ duration: submenuDuration }}` derived
- `frontend/src/lib/components/CommandPalette.reduced-motion.test.ts` (new) — vitest browser-mode; assert duration goes to 0 with `prefers-reduced-motion: reduce` mocked
- `frontend/e2e/theme.test.ts` (modified) — extend `AC1.6` with submenu-opens-without-delay assertion
- `frontend/src/lib/components/ui/command/command-item.svelte` (modified) — replace `outline-hidden` with `focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2`
- `frontend/src/lib/components/PoolFilterPill.svelte` (modified) — `:focus-visible` rule on clear button
- `frontend/src/lib/components/PanelActions.svelte` (modified) — `:focus-visible` rules on close button and Go-to-run link
- `frontend/src/lib/components/PoolFilterPill.test.ts`, `PanelActions.test.ts` (modified) — assert `outline-style: solid` and `outline-width: 2px` after `el.focus()` + `getComputedStyle`
- `frontend/e2e/focus-rings.test.ts` (new) — Tab-cycles every interactive surface; asserts `outline-width >= 2px || box-shadow !== 'none'` at each stop
- `frontend/src/app.css` (modified) — `.atc-scrollbar` class with cross-browser thumb styling
- `frontend/src/lib/components/KanbanColumn.svelte` (modified) — gains `.atc-scrollbar`
- `frontend/src/lib/components/RunDetailPanel.svelte` (modified) — body container gains `.atc-scrollbar`
- `frontend/src/lib/components/KanbanColumn.test.ts` (modified) — assert class is applied

**Dependencies:** Phase 2 (responsive grid in place; scrollbar is most visible on narrow widths).

**Done when:** `CommandPalette` submenu animation respects reduced motion. All four custom focus targets render visible focus indicators on `:focus-visible`. `KanbanColumn` and `RunDetailPanel` body show themed scrollbars in Chromium/Safari and Firefox. All animation rows in the inventory matrix have at least one reduced-motion-asserting test.

**ACs covered:** `frontend-1-0-polish.AC3.1`, `frontend-1-0-polish.AC3.2`, `frontend-1-0-polish.AC3.3`, `frontend-1-0-polish.AC4.1`, `frontend-1-0-polish.AC4.2`, `frontend-1-0-polish.AC4.3`, `frontend-1-0-polish.AC5.1`, `frontend-1-0-polish.AC5.2`, `frontend-1-0-polish.AC5.3`.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: ARIA live region

**Goal:** Announce run-level column transitions to assistive technology with hybrid coalescing under burst load.

**Components:**
- `frontend/src/lib/aria/transition-kinds.ts` (new) — `TransitionKind` discriminated union; `classifyTransition` exhaustive switch over `RunStatus`; `VERB_BY_CONCLUSION` `Record<RunConclusion, string>`
- `frontend/src/lib/aria/transition-kinds.test.ts` (new) — exhaustive verb table; type-level test `AssertEqual<keyof typeof VERB_BY_CONCLUSION, RunConclusion>`; classification cases including null-conclusion-with-Completed throw
- `frontend/src/lib/aria/format-run-transition.ts` (new) — pure builder: `(run, transitionKind) => string` with null-branch elision
- `frontend/src/lib/aria/format-run-transition.test.ts` (new) — message format per transition kind, branch elision, conclusion-specific verbs
- `frontend/src/lib/aria/live-region.svelte.ts` (new) — `LiveRegion` rune-class with `observeFlush(prev, next)`; `BurstAccumulator` opens at >3 transitions/RAF, debounces 200ms, accumulates counts across the entire burst window; per-run vs summary message construction; `liveRegion` module-scope singleton
- `frontend/src/lib/aria/live-region.test.ts` (new) — observeFlush diffing for run additions/transitions/removals; threshold-based per-run-vs-summary switch; debounce behavior with vi.useFakeTimers; multi-flush burst aggregation (counts span the whole window); reset on debounce close
- `frontend/src/lib/components/AriaLiveRegion.svelte` (new) — connected component subscribing to `liveRegion`; renders `<div role="log" aria-live="polite" aria-busy aria-label class="sr-only">`
- `frontend/src/lib/components/AriaLiveRegion.test.ts` (new) — role, aria-live, aria-busy reflection, sr-only class
- `frontend/src/lib/connection/event-dispatcher.ts` (modified) — `setOnFlush(cb)` method; pre-flush `RunStateSnapshot` capture from RunStore; post-flush snapshot + callback invocation
- `frontend/src/lib/connection/event-dispatcher.test.ts` (modified) — assert pre/post snapshot capture invariants; assert callback is invoked on each flush; assert no callback when `setOnFlush` was never called
- `frontend/src/lib/connection/connection-manager.ts` (modified) — wires `dispatcher.setOnFlush(liveRegion.observeFlush.bind(liveRegion))` once at construction
- `frontend/src/App.svelte` (modified) — mounts `<AriaLiveRegion />` as a sibling to `<AppShell />`
- `frontend/e2e/aria-live.test.ts` (new) — inject WS events at known cadences via `e2e/lib/ws-mock`; assert `aria-busy` flips during a synthetic burst; assert `textContent` matches expected per-run messages below threshold and summary message above; assert summary counts span multi-flush bursts (not just the final tick)

**Dependencies:** Phase 1 (docs aligned). Independent of Phases 2 and 3 functionally — could land in parallel — but ordered after Phase 3 to keep Phase 4's review surface small.

**Done when:** Below 3 transitions per RAF: per-run announcements with full message format. Above 3: summary form covering all transitions across the burst window, debounced 200ms. `RunConclusion` exhaustiveness enforced at compile time. `RunStatus` exhaustiveness enforced at compile + runtime. Off-shape WS payloads throw at the dispatcher boundary. All new tests pass.

**ACs covered:** `frontend-1-0-polish.AC6.1`, `frontend-1-0-polish.AC6.2`, `frontend-1-0-polish.AC6.3`, `frontend-1-0-polish.AC6.4`, `frontend-1-0-polish.AC6.5`, `frontend-1-0-polish.AC6.6`, `frontend-1-0-polish.AC6.7`.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Performance verification and final regression

**Goal:** Add the two-tier performance guard and run the full E2E regression to confirm nothing earlier in the bundle regressed.

**Components:**
- `frontend/src/lib/connection/event-dispatcher.perf.test.ts` (new, vitest browser-mode) — fires 1000 mock events spread over `setTimeout(..., Math.floor(i / 10))`; asserts `flushSpy` called at most `Math.ceil(elapsedMs / 16.67) + 2` times; asserts `processedCount === 1000`
- `frontend/e2e/frame-budget.test.ts` (new, Playwright) — `page.tracing.start({ categories: ['rendering'] })`; fires 1000 WS events through `e2e/lib/ws-mock`; parses BeginFrame deltas from trace JSON; saves trace as CI artifact; logs frame-budget summary; test always passes (informational)
- `justfile` (modified) — `test-perf` recipe runs Tier 1 + Tier 2 locally; existing `test` recipe includes Tier 1 (vitest); existing `test-e2e` recipe includes Tier 2
- `.github/workflows/ci.yml` (modified, if needed) — upload `frame-budget-trace.json` as a CI artifact (the existing artifact-upload step may already cover this; verify)
- `docs/architecture/frontend-app.md` (modified) — add EmptyState component, AriaLiveRegion module, responsive breakpoint contract, animation inventory table
- `frontend/CLAUDE.md` (modified) — update Sub-Phase status; reference new live-region module
- `CLAUDE.md` (root, modified) — update top-level frontend status to reflect 1.0 readiness
- `docs/ideation/ui-decomposition/README.md` (modified) — Sub-Phase 6b section header marked "✅ COMPLETE" with what shipped

**Dependencies:** Phases 1-4. This is the closing phase.

**Done when:** Tier 1 perf test passes deterministically in CI. Tier 2 produces a trace artifact uploaded by CI. Full prior-sub-phase E2E regression passes. All architecture docs and `CLAUDE.md` files updated. The `ui-decomposition/README.md` SP6b section reads as a "what shipped" record matching the format used for SP1-SP6a.

**ACs covered:** `frontend-1-0-polish.AC7.1`, `frontend-1-0-polish.AC7.2`, `frontend-1-0-polish.AC9.1`, `frontend-1-0-polish.AC10.1`, `frontend-1-0-polish.AC10.2`.
<!-- END_PHASE_5 -->

## Additional Considerations

### Documents to Update

| Document | Reason |
|---|---|
| `.impeccable.md` | Wall-display reframe (line 6) |
| `docs/ideation/ui-decomposition/component-patterns.md` | 3 wall-display rewrites/deletions |
| `docs/ideation/ui-decomposition/README.md` | 2 wall-display rewrites; SP6b section marked "✅ COMPLETE" with what shipped |
| `docs/ideation/design-research.md` | 2 wall-display rewrites |
| `docs/design-plans/2026-04-25-interactivity.md` | "Revised by" annotations on lines 30 and 375 |
| `docs/architecture/frontend-app.md` | Add EmptyState component, AriaLiveRegion module, responsive breakpoint contract, animation inventory |
| `frontend/CLAUDE.md` | Update Sub-Phase status; reference new live-region module |
| `CLAUDE.md` (root) | Update top-level frontend status to reflect 1.0 readiness |
| `scripts/doc-mapping.sh` | Add mappings: `EmptyState.svelte`, `AriaLiveRegion.svelte`, `live-region.svelte.ts` → `frontend-app.md` |

### Edge cases

- **`RunConclusion` is null when `RunStatus === 'Completed'`.** Should not happen at the wire level (the backend always populates conclusion on completion), but the frontend `classifyTransition` throws explicitly to surface upstream contract violations. Caught at the dispatcher boundary; no silent zero-fallback.
- **Initial snapshot vs incremental updates.** The first WS connection drains a snapshot of N runs through the dispatcher. We do NOT announce the snapshot — there's no "transition" yet, just initial state. The `LiveRegion.observeFlush` diff naturally handles this: prev snapshot is empty (`Map()`), next snapshot has N runs, but we filter for STATUS CHANGES, not new entries with their initial status. Re-runs (`*→Queued`) still announce.
- **TTL eviction.** When a completed run is evicted by TTL, it disappears from the store. `observeFlush` sees the run removed. We do NOT announce evictions — they're not meaningful column transitions, just cleanup.
- **Non-overlapping bursts.** If a burst closes (debounce fires, summary announced) and then 5+ transitions arrive in a fresh RAF, a new burst opens. Each burst summary is independent; counts do not carry across.
- **Prefers-reduced-motion changes mid-session.** The `prefersReducedMotion.current` rune is reactive — flipping the OS setting mid-session updates the gate without a reload. The `CommandPalette` submenu duration, the kanban transitions, and the halo CSS reset all respond.
- **Container width at exact breakpoints.** Tailwind v4's `sm:` (≥640px) and `xl:` (≥1280px) are inclusive at the lower bound. At exactly 640px the kanban renders 2 columns; at exactly 1280px it renders 3.

### Future extensibility

- **Container queries** become a viable upgrade if a future sidebar or split-view feature shrinks the kanban's effective width independently of the viewport. The migration is mechanical: wrap the kanban in a `@container` and swap `sm:` / `xl:` for `@sm:` / `@xl:`. No architectural change.
- **Filter-empty EmptyState.** When pool filtering is active and excludes all runs, the kanban currently renders empty columns (no `<EmptyState />`). A future enhancement could switch to a filter-aware empty state ("No runs match the current pool filter"). Out of scope here.
- **Detailed transition messages.** The `formatRunTransition` builder is a pure function; future enhancements (e.g., adding job count or duration to the message) are mechanical edits without architectural impact.
- **Additional `RunConclusion` variants.** Adding a new conclusion in `atc-core` triggers ts-rs regeneration, which fails the frontend type-check until `VERB_BY_CONCLUSION` adds the new key. The contract enforces this without runtime tests.

