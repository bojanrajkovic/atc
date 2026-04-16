# Kanban Board Design

## Summary

Sub-Phase 3 adds the kanban board — the primary view of the ATC dashboard. It renders workflow runs from GitHub Actions inside the existing app shell as three columns (Queued, In Progress, Completed), with cards sorted by operationally meaningful strategies per column (FIFO for queued runs, most-recently-started at top for in-progress, most-recently-finished at top for completed). Sorting lives in `RunStore` rather than in the components, so every consumer gets consistent ordering without each one re-sorting defensively. A connected `KanbanBoard` component reads the store's three pre-filtered, pre-sorted derived arrays and passes them down to pure column and card components.

The standout implementation concern is animation correctness. Cards must visually glide between columns as their status changes, not flicker out of one column and into another. This requires a single shared `crossfade` instance (exported from `kanban-transitions.ts`) that all three columns reference, so Svelte can match a departing card in one column to an arriving card in another by `run.id`. A crossfade fallback keyed on Svelte's `intro` boolean then handles new arrivals (`fly`) and removals (`fade`) in the same module, keeping animation logic in one place. The phase is structured as seven sequential mini-phases — store refactor, pure leaves, animation module, column assembly, board wiring, E2E, documentation — with an explicit scope-contract comment in `RunCard.svelte` that enforces the boundary between this phase's skeleton and Sub-Phase 4's richer card visuals.

## Definition of Done

**Primary deliverables:**
- Three-column kanban (Queued | InProgress | Completed) rendering inside `AppShell`'s `<main>` slot in `frontend/src/App.svelte`.
- New components, each with an exported TypeScript prop interface and a sibling `.test.ts`:
  - `KanbanBoard.svelte` — connected component that reads `RunStore` and passes three filtered+sorted run arrays to three `KanbanColumn` instances.
  - `KanbanColumn.svelte` — pure component that receives a filtered run array, a label, and renders `ColumnHeader` + a scrollable card list with animated transitions.
  - `ColumnHeader.svelte` — pure component that renders uppercase label + total count badge.
  - `RunCard.svelte` — pure component **skeleton only**; displays `displayTitle` and a status indicator. Progress, meta, runner, halo, duration ticker, and full StatusIcon are explicitly Sub-Phase 4 work.
- Card animations on state changes, implemented via a single shared `crossfade` instance whose `fallback` uses the `intro: boolean` parameter to distinguish arrival from removal:
  - `animate:flip` for within-column reordering, paired with `crossfade` on every card (mitigates Svelte issue #10252 for multi-item simultaneous transitions).
  - `crossfade` `send`/`receive` pair (matched by `run.id`) for cross-column movement — a card that transitions Queued→InProgress visually animates between the columns, not fade-out-then-fade-in.
  - Crossfade fallback with `intro=true` → `fly` for new-card arrival. There is no separate `transition:fly` directive; the behavior is consolidated into the crossfade fallback.
  - Crossfade fallback with `intro=false` → `fade` for card removal. There is no separate `transition:fade` directive; the behavior is consolidated into the crossfade fallback.
  - All animations respect `prefers-reduced-motion` and degrade to instant state changes.
- `RunStore` derived arrays get per-column sort strategies:
  - Queued: ascending by `createdAt` (FIFO — which runs next).
  - InProgress: descending by `runStartedAt` (most recently started at top).
  - Completed: descending by `updatedAt` (most recently finished at top).

**Success criteria:**
- Cards render in the correct column for their status; status changes move the card with a visible animation.
- Unit + browser-mode Vitest tests pass. Playwright E2E tests verify card placement and animated transitions using a mock event stream.
- `just lint`, `just check`, and `just test` all pass.
- Architecture docs updated: `docs/architecture/frontend-app.md` gains a Kanban Board section; `frontend/CLAUDE.md` updates its Key Files table.
- The existing app shell E2E tests still pass.

**Explicitly out of scope (forbidden in this phase — enforced at review):**
- Full `RunCard` visuals: progress bar, meta (repo/branch), runner label, pulsating halo, duration ticker, status accent bar.
- Full `StatusIcon` component with all conclusion symbols and ARIA. A minimal status indicator (colored dot or small symbol) is allowed on `RunCard`, but `StatusIcon.svelte` as a reusable component is Sub-Phase 4.
- `ColumnHeader` conclusion breakdown (success/failed/cancelled mini-pills).
- Card click-to-select, keyboard navigation, detail panel, command palette (Sub-Phase 5).
- Responsive breakpoints, virtualization, `EmptyState` component as a reusable primitive, reduced-motion audit beyond this phase's animations (Sub-Phase 6).

**Forbidden imports in Sub-Phase 3:**
- `RunCard.svelte` must not import `StatusIcon` (the Sub-Phase 4 component), `ProgressBar`, `JobMeta`, `JobHeader`, or `RunnerLabel` (none of which should exist yet).
- No CSS `@keyframes` rules (halo is Sub-Phase 4).
- No `setInterval` or recurring `$effect` in any Sub-Phase 3 component (duration ticker is Sub-Phase 4).

## Acceptance Criteria

### kanban-board.AC1: The kanban renders inside the app shell
- **kanban-board.AC1.1 Success:** Visiting the app after mount renders three column containers with uppercase labels "QUEUED", "IN PROGRESS", "COMPLETED" inside `<main>`.
- **kanban-board.AC1.2 Success:** Each column is a `role="list"`; each rendered card is a `role="listitem"`.
- **kanban-board.AC1.3 Success:** The board occupies the full width of `<main>` via `display: grid; grid-template-columns: repeat(3, 1fr)`.

### kanban-board.AC2: ColumnHeader renders label and count
- **kanban-board.AC2.1 Success:** ColumnHeader with `label="queued"` and `count=3` renders text "QUEUED" (uppercase) and "3" in a visible count badge.
- **kanban-board.AC2.2 Edge:** ColumnHeader with `count=0` still renders the badge with "0" (does not hide it). The empty-state message is a board-level concern, not a column-level one.

### kanban-board.AC3: Column sort strategies are deterministic
- **kanban-board.AC3.1 Success:** `runStore.queuedRuns` is sorted ascending by `createdAt`. Given runs with `createdAt = ["2026-04-16T10:00:00Z", "2026-04-16T09:00:00Z"]`, the resulting array is `["2026-04-16T09:00:00Z", "2026-04-16T10:00:00Z"]`.
- **kanban-board.AC3.2 Success:** `runStore.inProgressRuns` is sorted descending by `runStartedAt`. Given runs with `runStartedAt = ["2026-04-16T09:00:00Z", "2026-04-16T10:00:00Z"]`, the resulting array is `["2026-04-16T10:00:00Z", "2026-04-16T09:00:00Z"]`.
- **kanban-board.AC3.3 Edge:** `runStore.inProgressRuns` with a null `runStartedAt` falls back to `createdAt` for sort key; no crash, no NaN.
- **kanban-board.AC3.4 Success:** `runStore.completedRuns` is sorted descending by `updatedAt`. Most recently updated completed run appears at index 0.

### kanban-board.AC4: RunCard skeleton renders minimum information
- **kanban-board.AC4.1 Success:** RunCard with a `run` prop renders `run.displayTitle` as visible text.
- **kanban-board.AC4.2 Success:** RunCard renders a status indicator whose color is derived only from `run.status` (three values): `--queued` for Queued, `--running` for InProgress, `--text-dim` for Completed. Conclusion-based coloring (distinguishing Success, Failure, Cancelled, TimedOut, etc.) is explicitly deferred to Sub-Phase 4's `StatusIcon` work.
- **kanban-board.AC4.3 Failure:** RunCard source file contains zero matches for the forbidden import list (`StatusIcon`, `ProgressBar`, `JobMeta`, `JobHeader`, `RunnerLabel`), zero `@keyframes` rules, and zero `setInterval` calls. Enforced by the pre-merge grep checklist.

### kanban-board.AC5: Animation module exports the expected contract
- **kanban-board.AC5.1 Success:** `kanban-transitions.ts` exports `send`, `receive`, `DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`. All are defined.
- **kanban-board.AC5.2 Success:** The crossfade fallback returns a function when called with `intro=true` (arrival) and a function when called with `intro=false` (removal).
- **kanban-board.AC5.3 Success:** A KanbanColumn rendered in browser mode with two keyed cards has `animate:flip` applied to each card wrapper (directive presence verified via DOM transforms after reorder).
- **kanban-board.AC5.4 Success:** In browser mode, moving a card between two rendered columns (by mutating the store) produces matching `send`/`receive` keys; the card does not unmount and remount without the crossfade pair firing.
- **kanban-board.AC5.5 Success:** `bigint` as an `{#each}` key works: in browser mode, reordering the `runs` array for a KanbanColumn preserves the same DOM node identity (verified via a stable `data-run-id` attribute) across the re-render. No runtime errors from Svelte's keyed-each Map.

### kanban-board.AC6: Animations respect `prefers-reduced-motion`
- **kanban-board.AC6.1 Success:** When `prefersReducedMotion.current` is true at module init, `DURATION_MOVE`, `DURATION_ARRIVE`, and `DURATION_REMOVE` are all `0`.
- **kanban-board.AC6.2 Success:** Unit test: the reduced-motion branch's exported durations are exactly `0`.
- **kanban-board.AC6.3 Success:** Browser-mode test with `matchMedia` mocked to match `(prefers-reduced-motion: reduce)` verifies cards appear in final positions without visible animation errors.

### kanban-board.AC7: KanbanBoard wires RunStore to three columns
- **kanban-board.AC7.1 Success:** When `runStore` is empty, KanbanBoard renders "No workflows yet." text inline (no reusable `EmptyState` component).
- **kanban-board.AC7.2 Success:** After `runStore.applyRunEvent` with three runs of distinct statuses, each card appears in its corresponding column (verified via `data-run-id` attribute on the card DOM).
- **kanban-board.AC7.3 Success:** ColumnHeader count for each column reflects `runStore.{queued,inProgress,completed}Runs.length` after mutation.

### kanban-board.AC8: End-to-end lifecycle via mock WS event stream
- **kanban-board.AC8.1 Success:** On app load with no events, the E2E test sees "No workflows yet." and all three column headers.
- **kanban-board.AC8.2 Success:** Driving a run through `Queued → InProgress → Completed` via mock WS events moves the card across columns; E2E asserts on card placement at each step (not on animation behavior itself).
- **kanban-board.AC8.3 Success:** One viewport variant with `prefers-reduced-motion: reduce` completes the same lifecycle without animation-related console errors.

## Glossary

- **RunStore**: The Svelte 5 store (`runs.svelte.ts`) that holds all known `WorkflowRun` objects in a `Map<bigint, WorkflowRun>` and exposes three pre-filtered, pre-sorted `$derived` arrays: `queuedRuns`, `inProgressRuns`, and `completedRuns`.
- **WorkflowRun**: The core domain type (from `atc-core`) representing a single GitHub Actions workflow run, including fields like `id` (a bigint), `status`, `displayTitle`, `createdAt`, `runStartedAt`, and `updatedAt`.
- **displayTitle**: A human-readable string derived from the workflow run, used as the primary label on a `RunCard`. Computed internally from the run's data.
- **EventDispatcher**: The existing service component that receives WebSocket events from the backend, batches them in a `requestAnimationFrame` callback, and dispatches them to the store.
- **AppShell**: The existing layout component that provides the top bar and a `<main>` slot. The kanban board mounts inside this slot.
- **Queued / InProgress / Completed**: The three workflow run statuses that map to kanban columns. These are domain values from `atc-core`, not presentation strings — a card's column membership is determined entirely by its `status` field.
- **scope-contract comment**: A block comment at the top of `RunCard.svelte` that explicitly lists forbidden imports and patterns (Sub-Phase 4 components, `@keyframes`, `setInterval`). Enforced at code review via grep, not by the compiler. Removed as Sub-Phase 4's first task.
- **`$derived`**: A Svelte 5 rune that declares a value computed from reactive state. Svelte re-evaluates it automatically whenever its dependencies change. Used for the three column arrays in `RunStore`.
- **`$state`**: A Svelte 5 rune that declares reactive mutable state. The `runs` Map in `RunStore` is `$state`.
- **`{#each}` keyed block**: A Svelte template construct — `{#each items as item (item.id)}` — where the key in parentheses tells Svelte which DOM node corresponds to which data item across re-renders. Required for `animate:flip` and `crossfade` to work correctly.
- **`crossfade`**: A Svelte built-in transition factory (`svelte/transition`) that produces a matched `send`/`receive` pair. When a keyed element is removed from one DOM location and added in another, Svelte matches the keys and animates the element gliding from the old position to the new one, rather than fading out and fading in separately.
- **`crossfade` fallback**: The function passed as `fallback` to `crossfade(...)`. Svelte calls it when only one side of the `send`/`receive` pair fires (i.e., the card has no match in another column). The `intro` boolean parameter distinguishes new-card arrival (`true` → `fly`) from card removal (`false` → `fade`).
- **`animate:flip`**: A Svelte directive that applies a FLIP animation to an element when its position changes within a keyed `{#each}` block due to reordering. Paired with `crossfade` here to mitigate Svelte issue #10252.
- **FLIP**: First, Last, Invert, Play — a technique for animating layout changes. The browser records an element's starting position, lets the DOM update, then inverts the transform to snap the element back to where it was, then plays the transform to zero so the element appears to glide to its new position. `animate:flip` applies FLIP automatically.
- **`prefersReducedMotion`**: A Svelte motion utility (from `svelte/motion`) that exposes the `prefers-reduced-motion` media query as a reactive value. Read once at module init in `kanban-transitions.ts` to collapse all animation durations to `0`.
- **OKLCH**: A perceptually uniform color space used as the basis for the ATC design system's color tokens (e.g., `--queued`, `--running`, `--success`). Values are expressed as `oklch(L C H)` rather than hex or HSL.
- **FIFO**: First In, First Out. The sort strategy for the Queued column — runs that arrived earliest appear at the top, reflecting which run is next in line to start.
- **RAF**: `requestAnimationFrame`. The browser API used by `EventDispatcher` to batch WebSocket events into the rendering loop, avoiding redundant store updates within the same frame.
- **browser-mode**: A Vitest execution mode that runs tests inside a real Chromium browser (via Playwright) rather than jsdom. Required for any test that exercises `getBoundingClientRect`, `animate:flip`, or `crossfade`, since jsdom returns zero-sized rects.
- **Svelte issue #10252**: A known Svelte bug where the crossfade fallback can fire inconsistently when multiple items transition simultaneously. Mitigated in this design by pairing `animate:flip` with `crossfade` on every card wrapper.
- **connected component**: A component that reads from a store directly (here, `KanbanBoard` reads `runStore`). Contrasted with a **pure component**, which receives all its data via props and has no store dependencies.

## Architecture

Sub-Phase 3 introduces the kanban view that renders inside `AppShell`'s `<main>` slot. All new work is additive: no existing component in `frontend/src/lib/components/` is modified beyond wiring `<KanbanBoard />` into `App.svelte` in place of the current placeholder text.

### Component tree

```
App.svelte                              (modified: replace placeholder)
  ConnectionManager.svelte              (existing, unchanged)
  AppShell.svelte                       (existing, unchanged)
    TopBar.svelte                       (existing, unchanged)
    <main> (AppShell's slot)
      KanbanBoard.svelte                (NEW — connected, reads runStore)
        KanbanColumn.svelte × 3         (NEW — pure, one per status)
          ColumnHeader.svelte           (NEW — pure)
          <div class="card-list">       (inline — scrollable flex column)
            RunCard.svelte × N          (NEW — pure skeleton)
        {#if totalRuns === 0}
          inline empty message          (no new EmptyState primitive)
```

### New files

| Path | Role |
|------|------|
| `frontend/src/lib/components/KanbanBoard.svelte` | Connected. Reads `runStore.queuedRuns`, `runStore.inProgressRuns`, `runStore.completedRuns`. Renders three `KanbanColumn`s in a CSS Grid with three equal columns. Shows inline empty state when all columns are empty. |
| `frontend/src/lib/components/KanbanColumn.svelte` | Pure. Props: `label`, `runs` (sorted array). Renders `ColumnHeader` + a scrollable keyed `{#each}` of `RunCard` wrappers with `animate:flip` + `in:receive` + `out:send`. |
| `frontend/src/lib/components/ColumnHeader.svelte` | Pure. Props: `label`, `count`. Renders uppercase label + total count badge. No conclusion breakdown. |
| `frontend/src/lib/components/RunCard.svelte` | Pure **skeleton**. Props: `run: WorkflowRun`. Renders `displayTitle` and a minimal inline status indicator (colored dot). Scope-contract comment block at top of file forbids sibling-component imports. |
| `frontend/src/lib/animations/kanban-transitions.ts` | Shared `crossfade` instance. Exports `send`, `receive`, and motion constants (`DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`). Respects `prefersReducedMotion` at module init. |

### Modified files

| Path | Change |
|------|--------|
| `frontend/src/lib/stores/runs.svelte.ts` | Add `.sort()` to the **existing** three `$derived` arrays (`queuedRuns`, `inProgressRuns`, `completedRuns`). No new deriveds. |
| `frontend/src/App.svelte` | Replace the placeholder `<div>` inside `<AppShell>` with `<KanbanBoard />`. |

### Component contracts

```typescript
// KanbanBoard.svelte — no props (reads runStore directly)

// KanbanColumn.svelte
export interface KanbanColumnProps {
  label: string           // "QUEUED" | "IN PROGRESS" | "COMPLETED"
  runs: WorkflowRun[]     // already filtered and sorted by the parent
}

// ColumnHeader.svelte
export interface ColumnHeaderProps {
  label: string           // uppercase label
  count: number           // total card count
}

// RunCard.svelte
export interface RunCardProps {
  run: WorkflowRun        // single-prop idiom; display derived internally
}

// kanban-transitions.ts — module-scope exports
export const send: TransitionFn
export const receive: TransitionFn
export const DURATION_MOVE: number
export const DURATION_ARRIVE: number
export const DURATION_REMOVE: number
export const FLY_SETTLE_Y: number
```

### Data flow

```
WebSocket event
  → EventDispatcher (existing, RAF-batched)
    → RunStore.applyRunEvent / applyJobEvent (existing)
      → RunStore.runs (Map<bigint, WorkflowRun>) mutated
        → $derived queuedRuns / inProgressRuns / completedRuns re-evaluate
          → KanbanBoard re-renders (prop identity of filtered arrays changes)
            → KanbanColumn receives new array
              → {#each} keyed by run.id
                → Svelte computes the diff, fires animate:flip / send / receive
                  → RunCard renders updated run
```

Sorting lives in `RunStore` because it is a data-ordering concern, not a presentation concern. Keeping sort in the store avoids every consumer re-sorting defensively and keeps "single source of truth" intact.

The three sort strategies:

```typescript
// frontend/src/lib/stores/runs.svelte.ts
queuedRuns     = $derived( ... .filter(Queued)     .sort(asc  by createdAt) )
inProgressRuns = $derived( ... .filter(InProgress) .sort(desc by runStartedAt ?? createdAt) )
completedRuns  = $derived( ... .filter(Completed)  .sort(desc by updatedAt) )
```

All three timestamps are ISO-8601 strings; sorting uses `localeCompare` rather than `new Date()` to avoid allocation in the hot path. The `runStartedAt ?? createdAt` coalesce guards the transient window where an `InProgress` event is applied before the timestamp arrives.

### Animation model

A single `crossfade` instance lives in `kanban-transitions.ts` at module scope. It is shared across all three columns so that key-based pair matching works across sibling DOM subtrees (a card leaving `KanbanColumn[InProgress]` and arriving in `KanbanColumn[Completed]` must match on `run.id` across different `{#each}` blocks).

The fallback hook handles three distinct behaviors in one place:

| Trigger | Send/Receive | Fallback intro | Effect |
|---------|--------------|----------------|--------|
| Cross-column movement | both fire, keys match | — (not used) | Smooth `crossfade` (DURATION_MOVE) |
| New-card arrival | only `receive` fires | `intro=true` | `fly` (y=FLY_SETTLE_Y, DURATION_ARRIVE) |
| Card removal | only `send` fires | `intro=false` | `fade` (DURATION_REMOVE) |

`animate:flip` is applied to every card wrapper and pairs with `crossfade` to mitigate Svelte issue #10252 (inconsistent fallback firing under simultaneous multi-item transitions).

`prefers-reduced-motion` is consumed once at module init via `prefersReducedMotion` from `svelte/motion`. When true, exported durations are set to `0` — a single check point, not per-component branches.

## Existing Patterns

Investigation identified established patterns in Sub-Phases 1 and 2 that this design follows.

**Testing split — unit (jsdom) vs browser (Playwright chromium).** Sub-Phase 2 introduced `*.browser.test.ts` files (`SettingsPopover.browser.test.ts`, `TopBar.browser.test.ts`) for components that need a real browser environment. Sub-Phase 3 extends that split: any test that exercises `animate:flip` or `crossfade` lives in the browser project because jsdom returns zero-sized rects from `getBoundingClientRect`.

**Store-dependent component tests use `vi.resetModules()` + dynamic import.** `TopBar.browser.test.ts` established this idiom to get a fresh store module per test. `KanbanBoard.test.ts` follows the same shape.

**Accessibility-first test selectors.** `@testing-library/svelte` with `getByRole` / `getByLabelText` / `getByText` is used throughout. `getByTestId` is reserved for layout containers with no semantic role. Sub-Phase 3 components expose ARIA-appropriate roles (`role="list"` on card containers, `role="listitem"` on cards).

**Exported TypeScript `Props` interface per component.** Sub-Phase 2 established this per the UI decomposition README's principle #4. Sub-Phase 3 continues it — no inline prop types, no `any`.

**Component purity classification.** Pure / connected / service taxonomy from the UI decomposition README. `KanbanBoard` is the only new connected component; everything else is pure.

**OKLCH design tokens and inline `style="background-color: var(--x);"`.** Established in `CapacityBar`, `ConnectionIndicator`, `RunnerPool`. Sub-Phase 3 reuses existing status color tokens (`--queued`, `--running`, `--success`, `--failed`, `--cancelled`). No new color tokens needed.

**`$state` + `$derived`, no duplicated state.** Established throughout the frontend. Sub-Phase 3 sorts in-place by modifying the existing three `$derived` arrays, not by adding new sorted deriveds.

**New pattern introduced by this phase.** CSS Grid for the three-column layout. Existing components use flex-based layouts; `KanbanBoard`'s three-column-equal-width layout is the first use of `display: grid; grid-template-columns: repeat(3, 1fr);` in this codebase. This introduces a small convention (Grid for peer-column layouts, Flex for everything else) that future multi-column features can follow.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Store sort refactor

**Goal:** Extend `RunStore`'s three existing derived arrays with deterministic per-column sort strategies, without introducing new deriveds.

**Components:**
- `frontend/src/lib/stores/runs.svelte.ts` — add `.sort(...)` to the existing `queuedRuns`, `inProgressRuns`, `completedRuns` `$derived` expressions
- Existing `frontend/src/lib/stores/runs.*.test.ts` suite — add sort-order assertions; audit existing assertions for any that depend on the previous unsorted order (likely none — current tests assert membership, not position)
- New test: bigint-key round-trip sanity test in `runs.apply-events.test.ts` or a dedicated `runs.bigint-key.test.ts` — verifies that re-applying an event for an existing `run.id` yields the same object identity from the Map (foundational for `animate:flip` correctness)

**Dependencies:** None (first phase).

**Done when:** Tests covering `kanban-board.AC3.1`, `kanban-board.AC3.2`, `kanban-board.AC3.3`, `kanban-board.AC3.4` pass. Existing runs.*.test.ts suite remains green. `just lint` and `just check` pass.

*Note:* `bigint`-as-`{#each}`-key is not verifiable in this phase (no keyed-each exists yet). It is verified in Phase 4 via `kanban-board.AC5.5`.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Pure leaf components

**Goal:** Build the two purest leaves of the kanban component tree: `ColumnHeader` (trivially testable) and `RunCard` (skeleton only, with scope-contract comment).

**Components:**
- `frontend/src/lib/components/ColumnHeader.svelte` — pure, props `{ label: string, count: number }`, renders uppercase label + total count badge
- `frontend/src/lib/components/ColumnHeader.test.ts` — unit test: renders label uppercase, renders count in a `role="status"` badge
- `frontend/src/lib/components/RunCard.svelte` — pure skeleton, prop `{ run: WorkflowRun }`, renders `run.displayTitle` and a minimal inline status indicator (colored dot derived from `run.status`). File opens with the scope-contract comment block enumerating forbidden imports
- `frontend/src/lib/components/RunCard.test.ts` — unit test: renders `displayTitle`, status indicator reflects `run.status`

**Dependencies:** None (pure leaves).

**Done when:** Tests covering `kanban-board.AC2.1`, `kanban-board.AC2.2`, `kanban-board.AC4.1`, `kanban-board.AC4.2`, `kanban-board.AC4.3` pass. `just lint` and `just check` pass.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Animation module

**Goal:** Produce the shared `crossfade` instance and motion constants that `KanbanColumn` will consume.

**Components:**
- `frontend/src/lib/animations/kanban-transitions.ts` — exports `send`, `receive`, `DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`. Single `crossfade` call at module scope with a `fallback` that switches on the `intro` boolean (`true` → `fly`, `false` → `fade`). Durations collapse to `0` when `prefersReducedMotion.current` is true.
- `frontend/src/lib/animations/kanban-transitions.test.ts` — unit test: exports are defined; fallback returns a function for both `intro=true` and `intro=false`; reduced-motion branch returns zero-duration transitions

**Dependencies:** None (standalone module).

**Done when:** Tests covering `kanban-board.AC5.1`, `kanban-board.AC5.2`, `kanban-board.AC6.1`, `kanban-board.AC6.2` pass. `just lint` and `just check` pass.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: KanbanColumn

**Goal:** Compose the leaf components and animation module into a column that handles reordering and cross-column transitions correctly.

**Components:**
- `frontend/src/lib/components/KanbanColumn.svelte` — pure, props `{ label: string, runs: WorkflowRun[] }`, renders `ColumnHeader` + a scrollable `{#each runs as run (run.id)}` block; each card is wrapped in a `<div animate:flip={...} in:receive={{ key: run.id }} out:send={{ key: run.id }}>` that contains `<RunCard {run} />`. Uses `role="list"` / `role="listitem"` for a11y.
- `frontend/src/lib/components/KanbanColumn.test.ts` — unit test (jsdom): correct number of cards rendered, stable `data-run-id` on each, empty-list branch renders nothing. Does NOT assert animation behavior (jsdom can't measure positions).
- `frontend/src/lib/components/KanbanColumn.browser.test.ts` — browser-mode test: `Element.prototype.animate` mocked to skip duration; `animate:flip` directive present; `prefers-reduced-motion` via `matchMedia` mock yields zero-duration animations; marked serial (or `svelte/transition` mocked) to avoid cross-file crossfade state races.

**Dependencies:** Phase 2 (ColumnHeader, RunCard), Phase 3 (kanban-transitions).

**Done when:** Tests covering `kanban-board.AC1.2`, `kanban-board.AC5.3`, `kanban-board.AC5.4`, `kanban-board.AC5.5`, `kanban-board.AC6.3` pass. `just lint`, `just check`, and `just test` pass.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: KanbanBoard + wire-up

**Goal:** Stand up the connected component that reads `runStore`, renders three `KanbanColumn`s in a CSS Grid, handles the empty-board case, and replaces the placeholder in `App.svelte`.

**Components:**
- `frontend/src/lib/components/KanbanBoard.svelte` — connected; reads `runStore.queuedRuns`, `runStore.inProgressRuns`, `runStore.completedRuns`; renders three columns in `display: grid; grid-template-columns: repeat(3, 1fr)`. Inline `{#if totalRuns === 0}` renders "No workflows yet." (no new `EmptyState` primitive). `totalRuns` is a local `$derived`.
- `frontend/src/lib/components/KanbanBoard.test.ts` — uses `vi.resetModules()` + dynamic import of `runStore` (matches `TopBar.browser.test.ts` pattern). Asserts: empty state appears when store is empty; cards distribute to correct columns after `applyRunEvent`; column counts update on store mutation.
- `frontend/src/App.svelte` — replace the placeholder `<div>` inside `<AppShell>` with `<KanbanBoard />`.

**Dependencies:** Phase 1 (sorted deriveds), Phase 4 (KanbanColumn).

**Done when:** Tests covering `kanban-board.AC1.1`, `kanban-board.AC1.3`, `kanban-board.AC7.1`, `kanban-board.AC7.2` pass. `just lint`, `just check`, and `just test` pass.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: E2E coverage

**Goal:** Verify end-to-end behavior: the kanban renders from a real Vite build, cards move between columns as WebSocket events arrive, and animations run without errors.

**Components:**
- `frontend/e2e/kanban.spec.ts` — Playwright test that mounts the app with a mock WebSocket event stream. Drives a run through `Queued → InProgress → Completed`, asserting the card appears in the correct column at each step. Asserts the empty state renders before any events. Includes one `prefers-reduced-motion: reduce` viewport variant that verifies no animation errors and cards still appear in the right column.

**Dependencies:** Phases 1-5 (full kanban functional).

**Done when:** Tests covering `kanban-board.AC8.1`, `kanban-board.AC8.2`, `kanban-board.AC8.3` pass. Full `just test` suite passes (unit + browser + E2E).
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7: Documentation

**Goal:** Update architecture docs, module CLAUDE.md files, and doc-staleness mappings per project convention, so the doc-staleness gate at pre-push stays green and future contributors find the canonical description of the kanban.

**Components:**
- `docs/architecture/frontend-app.md` — add Kanban Board section covering component hierarchy, data flow, animation model, sort strategies, and testing approach
- `frontend/CLAUDE.md` — extend Key Files table with `KanbanBoard.svelte`, `KanbanColumn.svelte`, `ColumnHeader.svelte`, `RunCard.svelte`, `kanban-transitions.ts`
- `CLAUDE.md` (root) — update Status paragraph to reflect Sub-Phase 3 completion
- `scripts/doc-mapping.sh` — add mapping: `lib/components/Kanban*.svelte` → `docs/architecture/frontend-app.md`; `lib/components/RunCard.svelte` → `docs/architecture/frontend-app.md`; `lib/animations/kanban-transitions.ts` → `docs/architecture/frontend-app.md`

**Dependencies:** Phases 1-6 (code stable).

**Done when:** `scripts/check-docs-lefthook.sh` passes for the full diff. No stale doc warnings.
<!-- END_PHASE_7 -->

## Additional Considerations

### Scope guardrails (forbidden in this phase, enforced at review)

A concrete list of temptations that would leak Sub-Phase 4 work into this phase. All enforced via code review (grep checks in the pre-merge checklist), not via lint or type rules.

- `RunCard.svelte` must not import `StatusIcon`, `ProgressBar`, `JobMeta`, `JobHeader`, or `RunnerLabel` — none of these components should exist yet; all are Sub-Phase 4.
- No CSS `@keyframes` rules in any Sub-Phase 3 file (the pulsating halo is Sub-Phase 4).
- No `setInterval` or recurring `$effect` in any Sub-Phase 3 component (the duration ticker is Sub-Phase 4).
- `RunCard`'s rendered body must not exceed the skeleton contract: `displayTitle` + minimal inline status indicator. No progress bar, no meta (repo/branch), no runner label, no accent bar.
- `ColumnHeader` renders total count only. No conclusion breakdown pills (Sub-Phase 4).
- No card click handlers, no keyboard navigation, no detail panel (Sub-Phase 5).
- No responsive breakpoints, no virtualization, no reusable `EmptyState.svelte` primitive (Sub-Phase 6).

**Pre-merge review checklist (explicit):**

```bash
# Forbidden imports in RunCard
grep -E "(StatusIcon|ProgressBar|JobMeta|JobHeader|RunnerLabel)" frontend/src/lib/components/RunCard.svelte
# Should return zero matches

# Forbidden patterns in all Sub-Phase 3 components
grep -rE "@keyframes|setInterval" frontend/src/lib/components/Kanban*.svelte frontend/src/lib/components/RunCard.svelte frontend/src/lib/animations/kanban-transitions.ts
# Should return zero matches
```

### Handoff to Sub-Phase 4

The scope guardrails above are **not permanent architectural rules**. They exist only for Sub-Phase 3 and must be removed when Sub-Phase 4 begins, or legitimate Sub-Phase 4 work will be blocked by folklore.

Three places carry the expiration signal:

1. **The scope-contract comment at the top of `RunCard.svelte`** explicitly states that the block is temporary and must be removed as part of Sub-Phase 4's first task.
2. **This design document** — this section — records the handoff expectation so nothing is silently forgotten.
3. **Sub-Phase 4's future design plan** must include a Phase 0 (pre-work) task: *"Remove the Sub-Phase 3 scope-contract comment from `RunCard.svelte`. 1 commit, no behavior change. This yields a clean, low-risk first commit on the Sub-Phase 4 branch and verifies the branch is building before any real component work starts."*

### Open risks and mitigations

| Risk | Mitigation |
|------|------------|
| `bigint` as `{#each}` key may interact oddly with Svelte's internal keyed-list Map. | Phase 4 adds a browser-mode test (AC5.5) that reorders a column's `runs` array and asserts stable DOM identity via a `data-run-id` attribute. If this fails, we coerce the key to string at render time (`(run) => String(run.id)`) or raise the issue upstream. |
| Svelte issue #10252 — multi-item simultaneous crossfade fallback can fire inconsistently. | `animate:flip` paired with `crossfade` on every card (per research). The E2E test drives multiple simultaneous transitions and asserts final DOM state; if the mitigation is insufficient, we'll catch it before merge. |
| `crossfade`'s global state can race across parallel test files. | Browser-mode tests that exercise `send`/`receive` are marked serial, or `svelte/transition` is mocked at file scope. Either approach is fine; pick whichever keeps the test file more readable. |

### Documents to update

Per `.ed3d/design-plan-guidance.md` principle 6, this design enumerates every documentation surface that must change alongside the code, so none is missed during implementation:

| Document | Change |
|----------|--------|
| `docs/architecture/frontend-app.md` | Add Kanban Board section (component hierarchy, data flow, animation model, testing approach) |
| `frontend/CLAUDE.md` | Extend Key Files table with the 4 new components + `kanban-transitions.ts` |
| `CLAUDE.md` (root) | Update Status paragraph to reflect Sub-Phase 3 completion |
| `scripts/doc-mapping.sh` | Add source → `frontend-app.md` mappings for the new files |

