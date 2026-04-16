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
  - `RunCard.svelte` — pure component **skeleton only**; displays `displayTitle` and a status indicator that combines color **with** a non-color cue (a small glyph **and** visually-hidden status text) so the indicator never communicates via color alone (`.impeccable.md` principle 2). Progress, meta, runner, halo, duration ticker, and full `StatusIcon` are explicitly Sub-Phase 4 work.
- Card animations on state changes, implemented via a single shared `crossfade` instance whose `fallback` uses the `intro: boolean` parameter to distinguish arrival from removal:
  - `animate:flip` for within-column reordering, paired with `crossfade` on every card (mitigates Svelte issue #10252 for multi-item simultaneous transitions). `animate:flip` consumes the same `DURATION_MOVE` constant exported from `kanban-transitions.ts` — a single reduced-motion check zeroes every motion primitive in the kanban.
  - `crossfade` `send`/`receive` pair (matched by `run.id`) for cross-column movement — a card that transitions Queued→InProgress visually animates between the columns, not fade-out-then-fade-in.
  - Crossfade fallback with `intro=true` → `fly` for new-card arrival. There is no separate `transition:fly` directive; the behavior is consolidated into the crossfade fallback.
  - Crossfade fallback with `intro=false` → `fade` for card removal. There is no separate `transition:fade` directive; the behavior is consolidated into the crossfade fallback.
  - All animations (crossfade, flip, fallback) respect `prefers-reduced-motion` and degrade to instant state changes via the single module-level duration branch.
- `RunStore` derived arrays get per-column sort strategies with deterministic tie-breakers:
  - Queued: ascending by `createdAt`, then ascending by `run.id` (FIFO — which runs next).
  - InProgress: descending by `runStartedAt ?? createdAt`, then descending by `run.id` (most recently started at top).
  - Completed: descending by `updatedAt`, then descending by `run.id` (most recently finished at top).
  - Comparison uses direct lexical comparison of ISO-8601 strings (`a < b`, `a > b`) — NOT `localeCompare`. ISO-8601 is machine-sortable; locale-sensitive comparison is both slower and wrong. Secondary key on `run.id` prevents Map-iteration-order leakage on reconnect when timestamps tie.

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
- **kanban-board.AC1.1 Success:** Visiting the app after mount renders three column containers labeled "QUEUED", "IN PROGRESS", "COMPLETED" inside `<main>`. Labels are presentation only — the underlying domain values remain `Queued`, `InProgress`, `Completed`.
- **kanban-board.AC1.2 Success:** Each column is a `<section>` with `aria-labelledby` referencing its heading; inside the section, the card container is a `role="list"` and each card is a `role="listitem"`. The heading is a real `<h2>` (visually styled per the design system) so screen-reader navigation treats each column as an addressable region.
- **kanban-board.AC1.3 Success:** The board occupies the full width of `<main>` via `display: grid; grid-template-columns: repeat(3, 1fr)`.

### kanban-board.AC2: ColumnHeader renders label and count
- **kanban-board.AC2.1 Success:** ColumnHeader with `label="queued"` and `count=3` renders text "QUEUED" (uppercase) in the heading and "3" as a plain-text count badge. The badge is NOT a `role="status"` element — in a live dashboard, column counts change constantly, and `role="status"` would produce screen-reader churn.
- **kanban-board.AC2.2 Edge:** ColumnHeader with `count=0` still renders the badge with "0" (does not hide it). The empty-state message is a board-level concern, not a column-level one.

### kanban-board.AC3: Column sort strategies are deterministic
- **kanban-board.AC3.1 Success:** `runStore.queuedRuns` is sorted ascending by `createdAt`. Given runs with `createdAt = ["2026-04-16T10:00:00Z", "2026-04-16T09:00:00Z"]`, the resulting array is `["2026-04-16T09:00:00Z", "2026-04-16T10:00:00Z"]`.
- **kanban-board.AC3.2 Success:** `runStore.inProgressRuns` is sorted descending by `runStartedAt`. Given runs with `runStartedAt = ["2026-04-16T09:00:00Z", "2026-04-16T10:00:00Z"]`, the resulting array is `["2026-04-16T10:00:00Z", "2026-04-16T09:00:00Z"]`.
- **kanban-board.AC3.3 Edge:** `runStore.inProgressRuns` with a null `runStartedAt` falls back to `createdAt` for sort key; no crash, no NaN.
- **kanban-board.AC3.4 Success:** `runStore.completedRuns` is sorted descending by `updatedAt`. Most recently updated completed run appears at index 0.
- **kanban-board.AC3.5 Success:** When two runs have identical primary-sort timestamps, the tie-breaker is `run.id` (ascending for Queued, descending for InProgress and Completed). Given three runs with the same `createdAt` and `run.id = [3n, 1n, 2n]`, `queuedRuns` yields them in order `[1n, 2n, 3n]`.
- **kanban-board.AC3.6 Success:** A snapshot reload (`runStore.loadSnapshot(...)`) of the same runs with unchanged timestamps produces the same array order as before the reload. No gratuitous reshuffling from Map iteration order.
- **kanban-board.AC3.7 Success:** Sort comparison uses direct lexical string comparison on ISO-8601 timestamps (`a < b`), not `localeCompare`. Assertion: sort implementation contains no call to `localeCompare`.

### kanban-board.AC4: RunCard skeleton renders minimum information
- **kanban-board.AC4.1 Success:** RunCard with a `run` prop renders `run.displayTitle` as visible text.
- **kanban-board.AC4.2 Success:** RunCard's status indicator combines color with a non-color cue. Color is derived only from `run.status` (three values): `--queued` for Queued, `--running` for InProgress, `--text-dim` for Completed. Alongside the color, each indicator includes (a) a small glyph that differs per status (e.g., `○` / `▶` / `●` or equivalent geometric shapes) AND (b) visually-hidden status text (e.g., `<span class="sr-only">Status: In Progress</span>`) so the status is accessible to screen readers and distinguishable when color is unavailable. Conclusion-based coloring (distinguishing Success, Failure, Cancelled, TimedOut, etc.) remains Sub-Phase 4 scope.
- **kanban-board.AC4.3 Reviewer guidance:** RunCard source file should contain zero matches for the forbidden import list (`StatusIcon`, `ProgressBar`, `JobMeta`, `JobHeader`, `RunnerLabel`), zero `@keyframes` rules, and zero `setInterval` calls. This is a reviewer-checked convention, NOT an automated gate — the scope-contract comment at the top of `RunCard.svelte` documents the expectation, and code review is expected to catch violations.

### kanban-board.AC5: Animation module exports the expected contract
- **kanban-board.AC5.1 Success:** `kanban-transitions.ts` exports `send`, `receive`, `DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`. All are defined.
- **kanban-board.AC5.2 Success:** The crossfade fallback returns a function when called with `intro=true` (arrival) and a function when called with `intro=false` (removal).
- **kanban-board.AC5.3 Success:** A KanbanColumn rendered in browser mode with two keyed cards has `animate:flip` applied to each card wrapper (directive presence verified via DOM transforms after reorder).
- **kanban-board.AC5.4 Success:** In browser mode, when a run's status changes causing it to move between two rendered columns, both the `out:send` directive on the source column's card wrapper and the `in:receive` directive on the destination column's card wrapper fire with matching `run.id` keys (verified by instrumenting the crossfade's `send`/`receive` calls). The card is not merely removed from one column and inserted as a fresh element in the other — the crossfade pair matches by key.
- **kanban-board.AC5.5 Success:** `bigint` as an `{#each}` key works in Svelte 5's keyed-each: in browser mode, reordering the `runs` array for a KanbanColumn keeps each card's `data-run-id` attribute stable across the re-render, and no Svelte runtime errors occur.
- **kanban-board.AC5.6 Success:** In browser mode, mutating multiple runs in the same RAF batch (e.g., two runs transition Queued→InProgress simultaneously) lands all cards in their correct final columns. Asserted on final DOM state after the frame settles, not on intermediate animation frames.

### kanban-board.AC6: Animations respect `prefers-reduced-motion`
- **kanban-board.AC6.1 Success:** When `prefersReducedMotion.current` is true at module init, `DURATION_MOVE`, `DURATION_ARRIVE`, and `DURATION_REMOVE` are all `0`.
- **kanban-board.AC6.2 Success:** Unit test: the reduced-motion branch's exported durations are exactly `0`.
- **kanban-board.AC6.3 Success:** Browser-mode test with `matchMedia` mocked to match `(prefers-reduced-motion: reduce)` verifies cross-column movement completes without visible animation (cards appear in final positions in the destination column without animated transit).
- **kanban-board.AC6.4 Success:** Browser-mode test with reduced motion verifies that **within-column reorder** (pure `animate:flip`, no crossfade) also completes instantly. Because `animate:flip` consumes the same `DURATION_MOVE` constant, zeroing it under reduced motion suppresses FLIP motion too.

### kanban-board.AC7: KanbanBoard wires RunStore to three columns and distinguishes loading from empty
- **kanban-board.AC7.1 Success:** When `connectionStore.status !== 'connected'` (i.e., `connecting`, `reconnecting`, or `disconnected`), KanbanBoard renders a neutral hydration placeholder (inline text, not a reusable primitive) — NOT "No workflows yet." The placeholder distinguishes "we haven't finished loading" from "we loaded and there's nothing to show."
- **kanban-board.AC7.2 Success:** When `connectionStore.status === 'connected'` AND `totalRuns === 0`, KanbanBoard renders "No workflows yet." text inline.
- **kanban-board.AC7.3 Success:** When `connectionStore.status === 'connected'` AND `totalRuns > 0`, KanbanBoard renders the three-column kanban with cards distributed across their status-appropriate columns.
- **kanban-board.AC7.4 Success:** After `runStore.applyRunEvent` with three runs of distinct statuses, each card appears in its corresponding column (verified via `data-run-id` attribute on the card DOM).
- **kanban-board.AC7.5 Success:** ColumnHeader count for each column reflects `runStore.{queued,inProgress,completed}Runs.length` after mutation.
- **kanban-board.AC7.6 Success:** A snapshot reload via `runStore.loadSnapshot(...)` with identical run contents (same IDs, same timestamps) preserves DOM identity and ordering across the reload. Visual continuity is preserved on reconnect when nothing substantive has changed.

### kanban-board.AC8: End-to-end lifecycle via mock WS event stream
- **kanban-board.AC8.1 Success:** On app load, the E2E test sees the hydration placeholder ("Connecting…") on initial mount before the mock snapshot arrives. After the mock snapshot loads empty, it sees "No workflows yet." After the first mock WS event arrives, it sees all three column headers and the card in the appropriate column.
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
      KanbanBoard.svelte                (NEW — connected, reads runStore + connectionStore)
        {#if connectionStore.status !== 'connected'}
          hydration placeholder         (inline — "Connecting…")
        {:else if totalRuns === 0}
          empty message                 (inline — "No workflows yet.")
        {:else}
          <section> × 3                 (NEW — column sections)
            <h2>                        (column heading — aria-labelledby target)
            <div role="list">           (NEW — card container)
              <article role="listitem"> (one per card)
                KanbanColumn.svelte × 3 (NEW — pure, one per status)
                  ColumnHeader.svelte   (NEW — pure, renders the <h2>)
                  RunCard.svelte × N    (NEW — pure skeleton)
```

### New files

| Path | Role |
|------|------|
| `frontend/src/lib/components/KanbanBoard.svelte` | Connected. Reads `runStore.queuedRuns`, `runStore.inProgressRuns`, `runStore.completedRuns`, and `connectionStore.status`. Renders one of three states: hydration placeholder (`status !== 'connected'`), empty state (`connected && totalRuns === 0`), or the three-column grid (`connected && totalRuns > 0`). Grid is `display: grid; grid-template-columns: repeat(3, 1fr)`. |
| `frontend/src/lib/components/KanbanColumn.svelte` | Pure. Props: `label`, `runs` (sorted `readonly WorkflowRun[]`), `headingId` (the `aria-labelledby` anchor). Renders as a `<section aria-labelledby={headingId}>` with `ColumnHeader` providing the `<h2 id={headingId}>` and a `role="list"` card container below. Each card wrapper is a `role="listitem"` with `animate:flip` + `in:receive` + `out:send`. |
| `frontend/src/lib/components/ColumnHeader.svelte` | Pure. Props: `label`, `count`. Renders uppercase label + total count badge. No conclusion breakdown. |
| `frontend/src/lib/components/RunCard.svelte` | Pure **skeleton**. Props: `run: WorkflowRun`. Renders `displayTitle` and an inline status indicator that combines **color + glyph + visually-hidden status text** (never color alone, per `.impeccable.md` principle 2). Scope-contract comment block at top of file documents what this phase does NOT do (reviewer guidance, not an enforced gate). |
| `frontend/src/lib/animations/kanban-transitions.ts` | Shared `crossfade` instance. Exports `send`, `receive`, and motion constants (`DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`). Respects `prefersReducedMotion` at module init. |

### Modified files

| Path | Change |
|------|--------|
| `frontend/src/lib/stores/runs.svelte.ts` | Add `.sort()` to the **existing** three `$derived` arrays (`queuedRuns`, `inProgressRuns`, `completedRuns`). No new deriveds. |
| `frontend/src/App.svelte` | Replace the placeholder `<div>` inside `<AppShell>` with `<KanbanBoard />`. |

### Component contracts

```typescript
// KanbanBoard.svelte — no props (reads runStore + connectionStore directly)

// KanbanColumn.svelte
export interface KanbanColumnProps {
  label: string                     // "QUEUED" | "IN PROGRESS" | "COMPLETED" — presentation only, not a domain state
  runs: readonly WorkflowRun[]      // already filtered and sorted by the parent; readonly expresses the ownership boundary
  headingId: string                 // DOM id used by aria-labelledby on the <section> wrapper
}

// ColumnHeader.svelte
export interface ColumnHeaderProps {
  label: string           // uppercase label
  count: number           // total card count (rendered as plain text, NOT role="status")
  headingId: string       // DOM id assigned to the emitted <h2>, used by aria-labelledby on the parent <section>
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
// frontend/src/lib/stores/runs.svelte.ts (conceptual — direct lexical ISO compare + run.id tie-breaker)
queuedRuns     = $derived( ...filter(Queued)
  .sort((a, b) => a.createdAt === b.createdAt
    ? (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)         // asc tie-breaker
    : (a.createdAt < b.createdAt ? -1 : 1)) )

inProgressRuns = $derived( ...filter(InProgress)
  .sort((a, b) => {
    const aKey = a.runStartedAt ?? a.createdAt
    const bKey = b.runStartedAt ?? b.createdAt
    return aKey === bKey
      ? (a.id > b.id ? -1 : a.id < b.id ? 1 : 0)       // desc tie-breaker
      : (aKey > bKey ? -1 : 1)                         // desc primary
  }) )

completedRuns  = $derived( ...filter(Completed)
  .sort((a, b) => a.updatedAt === b.updatedAt
    ? (a.id > b.id ? -1 : a.id < b.id ? 1 : 0)
    : (a.updatedAt > b.updatedAt ? -1 : 1)) )
```

All three timestamps are ISO-8601 strings; sorting uses **direct lexical comparison** (`a < b`) — NOT `localeCompare`. Locale-sensitive comparison is both slower and incorrect: ISO-8601 is designed for lexical sort. Each column has a deterministic secondary key on `run.id` (bigint comparison) so identical timestamps produce stable order independent of Map iteration order. The `runStartedAt ?? createdAt` coalesce guards the transient window where an `InProgress` event is applied before the timestamp arrives.

### Animation model

A single `crossfade` instance lives in `kanban-transitions.ts` at module scope. It is shared across all three columns so that key-based pair matching works across sibling DOM subtrees (a card leaving `KanbanColumn[InProgress]` and arriving in `KanbanColumn[Completed]` must match on `run.id` across different `{#each}` blocks).

The fallback hook handles three distinct behaviors in one place:

| Trigger | Send/Receive | Fallback intro | Effect |
|---------|--------------|----------------|--------|
| Cross-column movement | both fire, keys match | — (not used) | Smooth `crossfade` (DURATION_MOVE) |
| New-card arrival | only `receive` fires | `intro=true` | `fly` (y=FLY_SETTLE_Y, DURATION_ARRIVE) |
| Card removal | only `send` fires | `intro=false` | `fade` (DURATION_REMOVE) |

`animate:flip` is applied to every card wrapper and pairs with `crossfade` to mitigate Svelte issue #10252 (inconsistent fallback firing under simultaneous multi-item transitions). **`animate:flip` consumes the same `DURATION_MOVE` constant** exported from `kanban-transitions.ts`, not a hardcoded value — this is what makes reduced motion a single-source-of-truth concern.

`prefers-reduced-motion` is consumed once at module init via `prefersReducedMotion` from `svelte/motion`. When true, exported durations are set to `0` — a single check point, not per-component branches. Because `animate:flip`, the crossfade pair, AND the fallback all read their durations from the same module-level constants, zeroing them suppresses every motion primitive in the kanban consistently.

## Existing Patterns

Investigation identified established patterns in Sub-Phases 1 and 2 that this design follows.

**Testing split — unit (jsdom) vs browser (Playwright chromium).** Sub-Phase 2 introduced `*.browser.test.ts` files (`SettingsPopover.browser.test.ts`, `TopBar.browser.test.ts`) for components that need a real browser environment. Sub-Phase 3 extends that split: any test that exercises `animate:flip` or `crossfade` lives in the browser project because jsdom returns zero-sized rects from `getBoundingClientRect`.

**Store-dependent component tests use `vi.resetModules()` + dynamic import.** `TopBar.browser.test.ts` established this idiom to get a fresh store module per test. `KanbanBoard.test.ts` follows the same shape.

**Accessibility-first test selectors.** `@testing-library/svelte` with `getByRole` / `getByLabelText` / `getByText` is used throughout. `getByTestId` is reserved for layout containers with no semantic role. Sub-Phase 3 components expose ARIA-appropriate roles: each column is a `<section aria-labelledby>` with an `<h2>` heading and a nested `role="list"` / `role="listitem"` card container.

**Hydration signal via `connectionStore.status`.** The existing `ConnectionManager` (`frontend/src/lib/connection.ts`) only transitions `connectionStore.status` to `'connected'` AFTER the WS handshake completes, the state snapshot loads, and the pre-connect buffer drains. Sub-Phase 3 uses this as its hydration gate — the empty state message is only shown when `status === 'connected'` AND the store is empty, never while `connecting` or `reconnecting`.

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
- `frontend/src/lib/stores/runs.svelte.ts` — add `.sort(...)` to the existing `queuedRuns`, `inProgressRuns`, `completedRuns` `$derived` expressions. Comparators use direct lexical ISO-8601 string comparison (`a < b`, NOT `localeCompare`) with `run.id` (bigint) as the secondary key for deterministic ordering.
- Existing `frontend/src/lib/stores/runs.*.test.ts` suite — add sort-order assertions and tie-breaker assertions; audit existing assertions for any that depend on the previous unsorted order (likely none — current tests assert membership, not position).
- Snapshot-stability test: `runStore.loadSnapshot(...)` with the same runs twice in a row produces the same sort order (covers `kanban-board.AC3.6` and the reconnect-reconcile risk from review).

**Dependencies:** None (first phase).

**Done when:** Tests covering `kanban-board.AC3.1` through `kanban-board.AC3.7` pass. Existing runs.*.test.ts suite remains green. `just lint` and `just check` pass.

*Note:* `bigint`-as-`{#each}`-key is not verifiable in this phase (no keyed-each exists yet). It is verified in Phase 4 via `kanban-board.AC5.5`.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Pure leaf components

**Goal:** Build the two purest leaves of the kanban component tree: `ColumnHeader` (trivially testable) and `RunCard` (skeleton only, with scope-contract comment).

**Components:**
- `frontend/src/lib/components/ColumnHeader.svelte` — pure, props `{ label: string, count: number, headingId: string }`. Renders `<h2 id={headingId}>` containing uppercase label + plain-text count badge. Badge is NOT a `role="status"` element (would announce churn in a live dashboard).
- `frontend/src/lib/components/ColumnHeader.test.ts` — unit test: renders label uppercase, renders count as plain text, heading id is assigned to the `<h2>`.
- `frontend/src/lib/components/RunCard.svelte` — pure skeleton, prop `{ run: WorkflowRun }`. Renders `run.displayTitle` and an inline status indicator that emits color + a distinct glyph + visually-hidden status text ("Queued" / "In Progress" / "Completed"). Never color alone. File opens with the scope-contract comment block (reviewer guidance, self-marked for removal in Sub-Phase 4's first commit).
- `frontend/src/lib/components/RunCard.test.ts` — unit test: renders `displayTitle`, status indicator includes glyph and sr-only text, color matches `run.status`.

**Dependencies:** None (pure leaves).

**Done when:** Tests covering `kanban-board.AC2.1`, `kanban-board.AC2.2`, `kanban-board.AC4.1`, `kanban-board.AC4.2` pass. `just lint` and `just check` pass.

*Note:* `kanban-board.AC4.3` is reviewer guidance, not a test — the scope-contract comment documents the convention and code review enforces it.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Animation module

**Goal:** Produce the shared `crossfade` instance and motion constants that `KanbanColumn` will consume.

**Components:**
- `frontend/src/lib/animations/kanban-transitions.ts` — exports `send`, `receive`, `DURATION_MOVE`, `DURATION_ARRIVE`, `DURATION_REMOVE`, `FLY_SETTLE_Y`. Single `crossfade` call at module scope with a `fallback` that switches on the `intro` boolean (`true` → `fly`, `false` → `fade`). Durations collapse to `0` when `prefersReducedMotion.current` is true. **`DURATION_MOVE` is intentionally consumed by `animate:flip` too** (in Phase 4) — zeroing it suppresses both cross-column crossfade and within-column FLIP in one place.
- `frontend/src/lib/animations/kanban-transitions.test.ts` — unit test: exports are defined; fallback returns a function for both `intro=true` and `intro=false`; reduced-motion branch returns zero for all three durations.

**Dependencies:** None (standalone module).

**Done when:** Tests covering `kanban-board.AC5.1`, `kanban-board.AC5.2`, `kanban-board.AC6.1`, `kanban-board.AC6.2` pass. `just lint` and `just check` pass.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: KanbanColumn

**Goal:** Compose the leaf components and animation module into a column that handles reordering and cross-column transitions correctly.

**Components:**
- `frontend/src/lib/components/KanbanColumn.svelte` — pure, props `{ label: string, runs: readonly WorkflowRun[], headingId: string }`. Renders as `<section aria-labelledby={headingId}>` containing `ColumnHeader` (which emits the `<h2 id={headingId}>`) + a scrollable `role="list"` `{#each runs as run (run.id)}` block. Each card is wrapped in an `<article role="listitem" animate:flip={{ duration: DURATION_MOVE, easing: cubicOut }} in:receive={{ key: run.id }} out:send={{ key: run.id }}>` containing `<RunCard {run} />`. `animate:flip` consumes the shared `DURATION_MOVE`, so reduced motion is inherited from the transitions module.
- `frontend/src/lib/components/KanbanColumn.test.ts` — unit test (jsdom): correct number of cards rendered, stable `data-run-id` on each, correct ARIA hierarchy (`<section aria-labelledby>` → `<h2>` + `role="list"` with `role="listitem"` children), empty-list branch renders nothing. Does NOT assert animation behavior (jsdom can't measure positions).
- `frontend/src/lib/components/KanbanColumn.browser.test.ts` — browser-mode test with `svelte/transition` mocked at the file scope (chosen over test-level serial to prevent cross-file crossfade state races while keeping parallel execution). Verifies: `animate:flip` directive present and reorders cards; `in:receive`/`out:send` fire with matching `run.id` keys when a card moves between two rendered columns; `prefers-reduced-motion` via `matchMedia` mock suppresses both within-column FLIP motion AND cross-column transit; multi-run burst mutation in one RAF lands all cards in correct final columns (`kanban-board.AC5.6`).

**Dependencies:** Phase 2 (ColumnHeader, RunCard), Phase 3 (kanban-transitions).

**Done when:** Tests covering `kanban-board.AC1.2`, `kanban-board.AC5.3`, `kanban-board.AC5.4`, `kanban-board.AC5.5`, `kanban-board.AC5.6`, `kanban-board.AC6.3`, `kanban-board.AC6.4` pass. `just lint`, `just check`, and `just test` pass.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: KanbanBoard + wire-up

**Goal:** Stand up the connected component that reads `runStore`, renders three `KanbanColumn`s in a CSS Grid, handles the empty-board case, and replaces the placeholder in `App.svelte`.

**Components:**
- `frontend/src/lib/components/KanbanBoard.svelte` — connected; reads `runStore.queuedRuns`, `runStore.inProgressRuns`, `runStore.completedRuns`, and `connectionStore.status`. Renders one of three states determined by the pair `(status, totalRuns)`:
  1. `status !== 'connected'` → inline hydration placeholder ("Connecting…"). This is the **loading** state.
  2. `status === 'connected' && totalRuns === 0` → inline empty text ("No workflows yet."). This is the **truly empty** state.
  3. `status === 'connected' && totalRuns > 0` → three-column grid (`display: grid; grid-template-columns: repeat(3, 1fr)`) with three `KanbanColumn`s.
  Stable `headingId`s are generated per column (e.g., `kanban-col-queued`) and passed down.
- `frontend/src/lib/components/KanbanBoard.test.ts` — uses `vi.resetModules()` + dynamic import of `runStore` and `connectionStore` (matches `TopBar.browser.test.ts` pattern). Asserts: hydration placeholder shown while `status !== 'connected'`; empty state shown only when `status === 'connected' && totalRuns === 0`; cards distribute to correct columns after `applyRunEvent`; column counts update on store mutation; snapshot reload with identical runs preserves card DOM identity and order (`kanban-board.AC7.6`).
- `frontend/src/App.svelte` — replace the placeholder `<div>` inside `<AppShell>` with `<KanbanBoard />`.

**Dependencies:** Phase 1 (sorted deriveds), Phase 4 (KanbanColumn).

**Done when:** Tests covering `kanban-board.AC1.1`, `kanban-board.AC1.3`, `kanban-board.AC7.1` through `kanban-board.AC7.6` pass. `just lint`, `just check`, and `just test` pass.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: E2E coverage

**Goal:** Verify end-to-end behavior: the kanban renders from a real Vite build, cards move between columns as WebSocket events arrive, and animations run without errors.

**Components:**
- `frontend/e2e/kanban.spec.ts` — Playwright test that mounts the app with a mock WebSocket event stream. Covers:
  1. Hydration placeholder ("Connecting…") appears briefly before the mock snapshot loads.
  2. After the mock snapshot loads empty, the "No workflows yet." empty state is shown.
  3. Driving a run through `Queued → InProgress → Completed` via mock WS events moves the card across columns; assert card placement at each step (not animation behavior).
  4. One `prefers-reduced-motion: reduce` viewport variant verifies the same lifecycle completes without animation-related console errors.

**Dependencies:** Phases 1-5 (full kanban functional).

**Done when:** Tests covering `kanban-board.AC8.1`, `kanban-board.AC8.2`, `kanban-board.AC8.3` pass. Full `just test` suite passes (unit + browser + E2E).
<!-- END_PHASE_6 -->

<!-- START_PHASE_7 -->
### Phase 7: Documentation

**Goal:** Update architecture docs, module CLAUDE.md files, and doc-staleness mappings per project convention, so the doc-staleness gate at pre-push stays green and future contributors find the canonical description of the kanban.

**Components:**
- `docs/architecture/frontend-app.md` — two-part update, NOT just an append:
  1. **Staleness sweep:** review the entire file for language that's now out of date (e.g., "The foundation infrastructure is complete... Component feature implementation is deferred to the next phase" — stale post-Sub-Phase 2). Refresh the status paragraph to reflect Sub-Phase 3. Update `Last verified` timestamp.
  2. **Append Kanban Board section:** component hierarchy (with the loading/empty/populated three-state branch), data flow (RunStore + ConnectionStore integration), animation model (shared crossfade + animate:flip sharing durations), sort strategies with tie-breakers, and testing approach (three-project split + what goes where).
- `frontend/CLAUDE.md` — extend Key Files table with `KanbanBoard.svelte`, `KanbanColumn.svelte`, `ColumnHeader.svelte`, `RunCard.svelte`, `kanban-transitions.ts`. Update Status paragraph.
- `CLAUDE.md` (root) — update Status paragraph to reflect Sub-Phase 3 completion.
- `scripts/doc-mapping.sh` — **no changes needed.** The existing `frontend/src/*` mapping already catches all new kanban files. Verified during design; mentioned here to prevent redundant additions.

**Dependencies:** Phases 1-6 (code stable).

**Done when:** `scripts/check-docs-lefthook.sh` passes for the full diff. `Last verified` dates on all touched architecture/CLAUDE docs are current. No stale doc warnings.
<!-- END_PHASE_7 -->

## Additional Considerations

### Scope guardrails (reviewer guidance, NOT an enforced gate)

A concrete list of temptations that would leak Sub-Phase 4 work into this phase. These are **reviewer conventions** — the scope-contract comment at the top of `RunCard.svelte` documents them, and PR review is expected to catch violations. There is deliberately no CI gate, lint rule, or pre-commit script enforcing the list. Grep-based enforcement is brittle (easy to forget, easy to bypass, easy to outdate), and the added infrastructure to maintain-then-remove the check isn't worth the cost for a single-phase guardrail.

- `RunCard.svelte` should not import `StatusIcon`, `ProgressBar`, `JobMeta`, `JobHeader`, or `RunnerLabel` — none of these components should exist yet; all are Sub-Phase 4.
- No CSS `@keyframes` rules in any Sub-Phase 3 file (the pulsating halo is Sub-Phase 4).
- No `setInterval` or recurring `$effect` in any Sub-Phase 3 component (the duration ticker is Sub-Phase 4).
- `RunCard`'s rendered body must not exceed the skeleton contract: `displayTitle` + status indicator (color + glyph + sr-only text). No progress bar, no meta (repo/branch), no runner label, no accent bar.
- `ColumnHeader` renders total count only. No conclusion breakdown pills (Sub-Phase 4).
- No card click handlers, no keyboard navigation, no detail panel (Sub-Phase 5).
- No ARIA live regions for card-level announcements (Sub-Phase 5 — see "Accessibility deferrals" below).
- No responsive breakpoints, no virtualization, no reusable `EmptyState.svelte` primitive (Sub-Phase 6).

During PR review, a reviewer checking for scope leakage can run this grep as a spot-check, but it is NOT a gate:

```bash
grep -E "(StatusIcon|ProgressBar|JobMeta|JobHeader|RunnerLabel|@keyframes|setInterval)" \
  frontend/src/lib/components/{Kanban*,RunCard}.svelte \
  frontend/src/lib/animations/kanban-transitions.ts
```

### Release-unit rationale (why Sub-Phase 3 is intentionally not standalone-valuable)

Frontend sub-phases 1–6 are internal checkpoints; they do not ship independently to users. The 1.0 release is the bundled delivery of all six sub-phases together. This means observations like "the Completed column is information-poor without a failure cue" or "live-update announcements are missing" are acknowledged and deliberately deferred — they are closed before 1.0 by Sub-Phase 4 (StatusIcon + conclusion coloring) and Sub-Phase 5 (ARIA live regions) respectively. Deferrals are documented below so later sub-phases know what gaps they own.

### Accessibility deferrals and Sub-Phase 5 handoff

This phase introduces real-time card movement but intentionally defers live-update screen-reader announcements to Sub-Phase 5. To prevent reinvention, here is the recommended implementation approach for Sub-Phase 5 when it designs the live-region strategy:

- **One polite live region** (`aria-live="polite"`, `aria-atomic="true"`) rendered at the KanbanBoard level (NOT per-column).
- **Per-run announcements with RAF-batch debouncing.** When the EventDispatcher flushes a RAF batch that changed card placements, aggregate the changes into a single announcement: "Run {displayTitle} moved to In Progress" for single changes, "{N} runs moved" for burst batches exceeding a threshold (e.g., 5).
- **Column counts are NOT live-announced.** Already noted in `kanban-board.AC2.1` — `ColumnHeader` badges are plain text, not `role="status"`, specifically to prevent announcement churn.
- **Mute during reconnect.** When `connectionStore.status` transitions through `reconnecting` → `connected` with a snapshot reload, suppress announcements for changes that result purely from reconcile.

### Handoff to Sub-Phase 4

The scope guardrails above are **not permanent architectural rules**. They exist only for Sub-Phase 3 and must be removed when Sub-Phase 4 begins, or legitimate Sub-Phase 4 work will be blocked by folklore.

Three places carry the expiration signal:

1. **The scope-contract comment at the top of `RunCard.svelte`** explicitly states that the block is temporary and must be removed as part of Sub-Phase 4's first task.
2. **This design document** — this section — records the handoff expectation so nothing is silently forgotten.
3. **Sub-Phase 4's future design plan** must include a Phase 0 (pre-work) task: *"Remove the Sub-Phase 3 scope-contract comment from `RunCard.svelte`. 1 commit, no behavior change. This yields a clean, low-risk first commit on the Sub-Phase 4 branch and verifies the branch is building before any real component work starts."*

### Branch and PR workflow

Per `.ed3d/design-plan-guidance.md` principle 8, this design document lives on branch `feat/kanban-board` (not main). All Sub-Phase 3 implementation commits land on the same branch. The PR title, when the branch is opened for review, will name the full Sub-Phase 3 deliverable (e.g., `feat: add kanban board with animated column transitions`) — NOT the design doc commit — because this repo uses squash merges and the PR title becomes the commit message on main.

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
| Svelte issue #10252 — multi-item simultaneous crossfade fallback can fire inconsistently. | `animate:flip` paired with `crossfade` on every card (per research). Phase 4's browser-mode test (`kanban-board.AC5.6`) mutates multiple runs in the same RAF batch and asserts final column placement; if the mitigation is insufficient, we'll catch it before merge. |
| `crossfade`'s global state can race across parallel test files. | Browser-mode tests that exercise `send`/`receive` are marked serial, or `svelte/transition` is mocked at file scope. Either approach is fine; pick whichever keeps the test file more readable. |

### Documents to update

Per `.ed3d/design-plan-guidance.md` principle 6, this design enumerates every documentation surface that must change alongside the code, so none is missed during implementation:

| Document | Change |
|----------|--------|
| `docs/architecture/frontend-app.md` | **Staleness sweep** (refresh the "foundation infrastructure is complete... deferred to next phase" language, update `Last verified`) + append a Kanban Board section (component hierarchy with loading/empty/populated branch, data flow including ConnectionStore hydration gate, animation model, sort strategies with tie-breakers, testing approach) |
| `frontend/CLAUDE.md` | Extend Key Files table with the 4 new components + `kanban-transitions.ts`; refresh Status paragraph; update `Last verified` |
| `CLAUDE.md` (root) | Update Status paragraph to reflect Sub-Phase 3 completion |
| `scripts/doc-mapping.sh` | **No changes** — existing `frontend/src/*` mapping already catches all new kanban files |

