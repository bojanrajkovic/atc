# Kanban Keyboard Navigation Design

## Summary

This design plan extracts roving-tabindex keyboard navigation from the Sub-Phase 6 polish bundle and ships it as a focused standalone deliverable. It adds 2D arrow-key navigation across the kanban board — ArrowUp/ArrowDown within a column, ArrowLeft/ArrowRight between non-empty columns (skipping empties), Home/End within a column — along with the APG no-wrap-at-edges convention and modifier-key delegation back to the existing window-level handlers. A single entry point (Tab into the kanban from outside) lands on the first card of the first non-empty column; Tab from within the kanban exits the grid as a group. The implementation also fixes an existing latent bug in `RunDetailPanel.svelte` where the `onCloseAutoFocus` callback calls `event.preventDefault()` and then silently short-circuits on a null querySelector result, stranding focus on `<body>` when the source card has been TTL-evicted while the panel was open.

The architectural approach is a new `roving/` module containing pure-function geometry (`geometry.ts`), a Svelte 5 action (`action.ts`), a context shape (`context.ts`), and a `RovingFocusProvider.svelte` wrapper component. Roving state — which card is focused and whether the kanban subtree currently holds document focus — is component-scoped via Svelte 5 `setContext`/`getContext` rather than a sixth module-singleton store, matching the lifecycle of the kanban itself. The `<RovingFocusProvider>` wraps `<AppShell>`, `<CommandPalette>`, and `<RunDetailPanel>` in the `App.svelte` component tree; because Svelte context propagates by component tree rather than DOM tree, the Bits UI portals those dialogs use do not break context access. Focus suspension when dialogs are open is structural — the action's `keydown` listener is scoped to the provider's root element, so focus moving into a portaled Bits UI dialog naturally silences the handler without any explicit coordination flag. Cross-column card-stable focus follows run identity via a `data-run-id` attribute already on each `<article>`; `RunCard.svelte`'s mount-time `$effect` calls `.focus()` on the bound button whenever `isFocused && kanbanHasFocus` becomes true, which handles both user-initiated arrow navigation and FLIP/crossfade migrations in one mechanism.

## Definition of Done

This design plan extracts roving-tabindex keyboard navigation out of the Sub-Phase 6 polish bundle and ships it as its own deliverable, on its own branch and PR, before the rest of Sub-Phase 6.

It is complete when:

1. **2D arrow navigation across the kanban grid.** ArrowUp / ArrowDown move within a column. ArrowLeft / ArrowRight move between non-empty columns (empty columns are skipped). Home / End jump to the first / last card of the current column. APG no-wrap at edges — ArrowDown at the bottom of a column is a no-op, ArrowRight past the last non-empty column is a no-op. Tab leaves the kanban as a single group (the next focusable element after the kanban root receives focus).

2. **ARIA semantics preserved.** The existing `<section>` / `<div role="list">` / `<div role="listitem">` structure stays. Focus management layers on externally via a single root-mounted key handler plus per-card `tabindex` swap (`-1` for non-focused, `0` for the active focusable target). No `role="grid"` or `role="gridcell"` adoption.

3. **Initial focus on first Tab into the kanban** lands on the first card of the first non-empty column. There is no persistent "last focused index" stored across kanban entries — entry behavior is deterministic from current state alone.

4. **Card-stable reorder behavior.** When WS-driven events reorder or move cards while the kanban has focus, focus follows the **run identity**, not the column position. The roving state keys on `run.id`. If the focused run moves from Queued → In Progress, the inner button on that card retains focus through the FLIP / crossfade animation.

5. **Suspension when dialogs are open.** The kanban's arrow / Home / End handler is inert whenever `RunDetailPanel` (Sheet) or `CommandPalette` (Command.Dialog) is open. Bits UI's focus traps already constrain Tab inside those dialogs; this design ensures no roving keys fire from the kanban while either is mounted, so the dialogs own the keyboard cleanly.

6. **Lost-trigger focus restoration on panel close.** When the panel closes and the source RunCard's `data-run-id` is no longer present in the DOM (the run was TTL-evicted, or the design plan revealed the existing latent bug where `RunDetailPanel.svelte`'s `onCloseAutoFocus` calls `event.preventDefault()` and then `?.focus()` on a null query result, leaving focus on `<body>`), focus restores to the first card in the first non-empty column. Both the existing latent bug and the roving-tabindex evicted-source case use the same restoration target. The bug fix ships in this PR.

7. **Library re-evaluation is concrete, not narrative.** The plan documents an actual integration attempt of `jakelazaroff/roving-tabindex` against the project's Vite + Tailwind v4 + Svelte 5 stack — what specifically breaks, what specifically works, what (if any) wrapping would be needed. The plan also re-checks `svelte-roving-ux` for rune-era compatibility against its current published version. Only after both checks does the plan default to a custom Svelte 5 context provider, with the failure modes of the libraries documented as the rejected alternatives in the architecture section.

8. **Out of scope for this plan:** the ARIA live region for run state changes stays deferred to residual Sub-Phase 6. None of the other Sub-Phase 6 polish items (EmptyState, responsive breakpoints, scrollbar styling, full reduced-motion audit, performance budget verification) are touched here.

9. **Documentation updates ship in the same PR.** `docs/ideation/ui-decomposition/README.md` Sub-Phase 6 section is updated to indicate roving-tabindex shipped separately, with a forward-pointer to this design plan and the eventual implementation PR. `docs/architecture/frontend-app.md` is updated for any new patterns introduced (focus management context provider, key handler attachment site). The `Documents to Update` table in this design plan enumerates every file that changes.

10. **Visual exploration via playground.** A self-contained HTML playground produced via the `/impeccable:frontend-design` and `/playground:playground` skills — committed under `docs/design-plans/playgrounds/2026-05-01-kanban-keyboard-nav-explorer.html` — captures the focus-ring visuals, key-handler interaction model, and reduced-motion fallback. Mirrors the Sub-Phase 5 playground pattern (`docs/design-plans/playgrounds/2026-04-25-interactivity-explorer.html`).

11. **Tests ship in the same PR.** Per-component tests (jsdom unit + browser-mode for any keyboard / focus / DOM-state behavior that requires a real browser) and Playwright E2E tests covering: all four arrow directions, Home / End, panel-open suspension (verified by inspecting the listener's active state, not just visually), focus restoration on panel close in both healthy and evicted-card paths, and card-stable reorder during a burst of WS events using the existing `e2e/lib/ws-mock.ts` harness (`makeRunEvent` / `makeJobSeqEvent` / `sendWS`). No test debt deferred to a polish phase.

## Acceptance Criteria

### `kanban-keyboard-nav.AC1`: Initial focus and tabindex correctness

- **kanban-keyboard-nav.AC1.1 Success:** After the first WS payload delivers at least one run, exactly one card has `tabindex="0"`. That card is the first card of the first non-empty column (Queued > InProgress > Completed priority).
- **kanban-keyboard-nav.AC1.2 Success:** Tab from outside the kanban lands focus on the card that has `tabindex="0"`.
- **kanban-keyboard-nav.AC1.3 Success:** Clicking any card sets `focusedRunId` to that run's id via the action's `focusin` listener; that card now has `tabindex="0"`, all others `tabindex="-1"`.
- **kanban-keyboard-nav.AC1.4 Success:** When the kanban has zero runs across all columns, no card has `tabindex="0"` (no cards exist) and `currentFocusRunId === null`.
- **kanban-keyboard-nav.AC1.5 Edge:** When the kanban transitions from zero to one-or-more runs, the new first card receives `tabindex="0"` reactively without user action.
- **kanban-keyboard-nav.AC1.6 Failure:** Two cards never simultaneously have `tabindex="0"` — even mid-reorder.
- **kanban-keyboard-nav.AC1.7 Failure:** No `role="grid"` or `role="gridcell"` attribute exists in the kanban subtree. The existing `<section>` / `role="list"` / `role="listitem"` structure is preserved unchanged.

### `kanban-keyboard-nav.AC2`: 2D arrow navigation

- **kanban-keyboard-nav.AC2.1 Success:** ArrowDown with focus on a card moves focus to the next card in the same column.
- **kanban-keyboard-nav.AC2.2 Success:** ArrowUp moves focus to the previous card in the same column.
- **kanban-keyboard-nav.AC2.3 Success:** ArrowRight moves focus to the corresponding row in the next non-empty column to the right.
- **kanban-keyboard-nav.AC2.4 Success:** ArrowLeft moves focus to the corresponding row in the next non-empty column to the left.
- **kanban-keyboard-nav.AC2.5 Success:** Home moves focus to the first card in the current column.
- **kanban-keyboard-nav.AC2.6 Success:** End moves focus to the last card in the current column.
- **kanban-keyboard-nav.AC2.7 Success:** Every claimed key calls `event.preventDefault()` to suppress browser-default scrolling.

### `kanban-keyboard-nav.AC3`: Edge and asymmetric-column behavior

- **kanban-keyboard-nav.AC3.1 Success:** ArrowDown at the last card of a column is a no-op (focus does not move). preventDefault is still called.
- **kanban-keyboard-nav.AC3.2 Success:** ArrowUp at the first card of a column is a no-op.
- **kanban-keyboard-nav.AC3.3 Success:** ArrowRight in the rightmost non-empty column is a no-op.
- **kanban-keyboard-nav.AC3.4 Success:** ArrowLeft in the leftmost non-empty column is a no-op.
- **kanban-keyboard-nav.AC3.5 Success:** ArrowRight skips empty columns: with focus in Queued and InProgress empty, ArrowRight lands in Completed.
- **kanban-keyboard-nav.AC3.6 Success:** ArrowLeft skips empty columns symmetrically.
- **kanban-keyboard-nav.AC3.7 Success:** Asymmetric columns: focus in row 5 of a 10-card column; ArrowRight to a 3-card column lands in row 2 (clamped to target's last index).
- **kanban-keyboard-nav.AC3.8 Edge:** ArrowRight where the immediate next column is empty but a later column is non-empty: focus lands in the further non-empty column. If none exists, no-op.

### `kanban-keyboard-nav.AC4`: Modifier-key delegation

- **kanban-keyboard-nav.AC4.1 Success:** Cmd+K with focus on a card opens the command palette (kanban handler returns early on `metaKey`; App.svelte's window-level handler fires).
- **kanban-keyboard-nav.AC4.2 Success:** Cmd+D toggles dark mode while focus is on a card.
- **kanban-keyboard-nav.AC4.3 Success:** Cmd+\\ toggles compact density.
- **kanban-keyboard-nav.AC4.4 Success:** Cmd+ArrowDown / Cmd+ArrowUp: kanban handler returns early; browser default takes over.
- **kanban-keyboard-nav.AC4.5 Success:** Shift+Arrow returns early in the kanban handler.
- **kanban-keyboard-nav.AC4.6 Success:** Alt+Arrow returns early.
- **kanban-keyboard-nav.AC4.7 Failure:** Bare ArrowDown is NOT delegated; the kanban handler claims it (no other handler runs).

### `kanban-keyboard-nav.AC5`: Suspension via natural focus scoping

- **kanban-keyboard-nav.AC5.1 Success:** When `paletteStore.paletteOpen === true` and focus is in the palette input, ArrowDown does NOT move kanban focus.
- **kanban-keyboard-nav.AC5.2 Success:** When `uiStore.selectedRunId !== null` and focus is inside the Sheet's focus trap, ArrowDown does NOT move kanban focus.
- **kanban-keyboard-nav.AC5.3 Success:** With both dialogs stacked, arrow keys do NOT affect kanban focus.
- **kanban-keyboard-nav.AC5.4 Success:** When the panel closes and focus returns to the trigger card, ArrowDown resumes working.

### `kanban-keyboard-nav.AC6`: Card-stable reorder

- **kanban-keyboard-nav.AC6.1 Success:** In-column reorder (e.g., another Queued card's sort key changes): focus persists on the same `data-run-id`, same DOM node (verified via reference equality, mirroring `KanbanColumn.browser.test.ts:18-67`).
- **kanban-keyboard-nav.AC6.2 Success:** Cross-column move via crossfade: focus migrates to the new `<button>` in the destination column with the same `data-run-id`. Verified via `document.activeElement.closest('[data-run-id]')` after `tick()`.
- **kanban-keyboard-nav.AC6.3 Success:** During the crossfade transition window, focus is on the new node from `tick()` onward; the old node's outro completes and unmounts without affecting focus.
- **kanban-keyboard-nav.AC6.4 Success:** With `kanbanHasFocus === false`, a cross-column move does NOT cause focus to migrate (RunCard's `$effect` is guarded).
- **kanban-keyboard-nav.AC6.5 Edge:** Burst WS events reorder cards while user holds ArrowDown. Final focus state matches user intent: card-stable across the burst.

### `kanban-keyboard-nav.AC7`: Lost-trigger restoration

- **kanban-keyboard-nav.AC7.1 Success:** Panel-close happy path: trigger card still mounted, focus restores to its `.run-card-activate` button (existing Sub-Phase 5 behavior preserved).
- **kanban-keyboard-nav.AC7.2 Success:** Panel-close with evicted source (regression test for the existing bug at `RunDetailPanel.svelte:52-66`): focus lands on the first card of the first non-empty column. NOT `<body>`.
- **kanban-keyboard-nav.AC7.3 Success:** Eviction during keyboard nav: provider's `$effect` detects `locate(focusedRunId, columns) === null`, calls `restoreFocusToInitial()`, focus lands on the first card of the first non-empty column.
- **kanban-keyboard-nav.AC7.4 Success:** Both restoration paths (AC7.2 and AC7.3) land on the same DOM node under identical preconditions.
- **kanban-keyboard-nav.AC7.5 Edge:** Panel close with no `lastTriggerRunId` recorded: `onCloseAutoFocus` returns early without `preventDefault`, browser-default focus restoration handles it.
- **kanban-keyboard-nav.AC7.6 Edge:** All columns become empty while a card is focused: `restoreFocusToInitial()` finds `initialFocusRunId === null` and returns without calling `.focus()`. No thrown error; focus lands wherever the browser places it (body).

## Glossary

- **roving tabindex**: A focus-management pattern where a container makes exactly one child focusable (`tabindex="0"`) at a time, with all others set to `tabindex="-1"`. Tab enters and exits the container as a single group; arrow keys move the "roving" `tabindex="0"` to the next target. Described by the WAI-ARIA Authoring Practices Guide (APG) for grid, listbox, and toolbar widgets.
- **APG (WAI-ARIA Authoring Practices Guide)**: W3C specification published by the ARIA Working Group that documents interaction patterns and keyboard conventions for accessible UI widgets. The "no-wrap at edges" rule in this design — ArrowDown at the last card is a no-op rather than wrapping to the first — comes from the APG grid pattern.
- **`tabindex`**: An HTML attribute controlling keyboard focus order. `tabindex="0"` makes an element naturally focusable and puts it in the document's tab sequence. `tabindex="-1"` removes it from the tab sequence but keeps it programmatically focusable via `.focus()`.
- **Svelte 5 action**: A function `(node: HTMLElement, params) => { destroy() }` that attaches imperative behavior to a DOM element via the `use:` directive. Actions are Svelte's escape hatch for direct DOM event listener management. This plan introduces the first action in the ATC codebase.
- **`setContext` / `getContext`**: Svelte's built-in mechanism for sharing state down a component tree without prop-drilling. `setContext(key, value)` registers a value in the current component; `getContext(key)` retrieves it from any descendant. State propagates by Svelte component tree, not by DOM tree — so Bits UI portal-rendered descendants still receive context from their Svelte parent.
- **`$state`**: Svelte 5 rune that declares a reactive variable. Reads are tracked; writes trigger reactive re-evaluation of any `$derived` or `$effect` that depends on the value.
- **`$derived`**: Svelte 5 rune that declares a value computed from reactive state. Re-evaluates automatically when its reactive dependencies change. Used here for `initialFocusRunId`, `currentFocusRunId`, and `isFocused` on each RunCard.
- **`$effect`**: Svelte 5 rune that runs a side-effectful callback whenever its reactive dependencies change, and after the DOM has been updated. Used in `RovingFocusProvider` to detect TTL eviction and in `RunCard` to call `.focus()` when the active card changes.
- **`tick()`**: A Svelte utility that returns a Promise resolving after the next DOM update cycle. Used in `restoreFocusToInitial()` to ensure the target card's button element is in the DOM before calling `.focus()`.
- **`animate:flip`**: A Svelte built-in animation directive that computes the delta between an element's old and new position in the DOM and interpolates between them. Used on run cards within a column to animate in-column reorder events driven by WebSocket updates.
- **crossfade**: A Svelte shared transition (`import { crossfade }` from `'svelte/transition'`) that coordinates the outgoing element in one list with the incoming element in another, producing a smooth "teleport" effect. Used here when a run moves between kanban columns (e.g., Queued → In Progress). The card-stable focus mechanic must account for the new DOM node created by the incoming side of the crossfade.
- **focus trap**: A keyboard interaction constraint — typically applied by dialog/modal implementations — that cycles Tab and Shift+Tab only among the focusable elements within a designated container, preventing focus from escaping to the page behind it. Bits UI implements focus traps for `Sheet` (the detail panel) and `Command.Dialog` (the command palette).
- **`kanbanHasFocus`**: A boolean context field tracked by the `roving` action via `focusin` / `focusout` events on the provider's root element. Guards the RunCard `$effect` from calling `.focus()` during initial page load or while focus is outside the kanban — preventing focus-stealing when arrow-key navigation is not the user's current intent.
- **`lastTriggerRunId`**: A field on `UIStore` (set in `RunCard.handleActivate`) that records which run's card opened the detail panel. `RunDetailPanel.onCloseAutoFocus` reads this to return focus to the source card on close. The existing bug this plan fixes: when the trigger card is absent from the DOM (evicted while the panel was open), the current code calls `event.preventDefault()` but then silently no-ops via optional chaining (`?.focus()`), leaving focus on `<body>`.
- **light-DOM web component**: A custom HTML element (`<tag-name>`) whose children are rendered directly in the document's main DOM tree, as opposed to shadow-DOM components whose internals are isolated in a shadow root. `jakelazaroff/roving-tabindex` is a light-DOM web component, meaning Tailwind utility classes and CSS custom properties pass through to it without the isolation issues shadow DOM would introduce.
- **Bits UI**: A headless Svelte component library providing accessible primitives (Dialog, Sheet, Popover, etc.) with built-in portal rendering into `<body>`, focus trapping, and ARIA attribute management. The detail panel and command palette are both built on Bits UI; their portaled DOM placement is why the action's DOM-scoped `keydown` handler naturally silences when those dialogs have focus.
- **`data-run-id`**: A `data-*` attribute on each `<article class="run-card">` holding the run's `bigint` ID as a string. Serves as the stable identity anchor for focus management — the roving system keys on run identity via this attribute rather than on DOM position, so focus follows a card through column transitions and in-column reorders.
- **`display: contents`**: A CSS value that makes an element's box invisible to layout — the element participates in neither block formatting nor flex/grid layout, and its children are laid out as if the wrapper did not exist. Applied via Tailwind's `contents` class on `<RovingFocusProvider>`'s root `<div>` so the wrapper does not disrupt the existing flex/grid structure of `AppShell` and `KanbanBoard`.

## Architecture

The implementation lives in a new module `frontend/src/lib/components/roving/` plus modifications to `App.svelte`, `RunCard.svelte`, and `RunDetailPanel.svelte`. No new store, no role-changing DOM restructuring, no third-party library. Roving state is component-scoped via Svelte 5 `setContext`/`getContext`; geometry is pure functions; keyboard handling is a Svelte action attached to a single root element.

### Module layout

```
frontend/src/lib/components/roving/
  context.ts                    # Context type + setRovingContext / getRovingContext
  context.test.ts               # jsdom unit tests for context lifecycle and mutators
  geometry.ts                   # Pure functions for 2D nav resolution
  geometry.test.ts              # jsdom unit tests covering all directions × edges × shapes
  action.ts                     # `roving` Svelte 5 action: focusin/focusout/keydown
  action.test.ts                # jsdom unit tests for action handler matrix
  RovingFocusProvider.svelte    # Wrapper component owning context state + applying action
  RovingFocusProvider.browser.test.ts  # Browser-mode test for kanbanHasFocus + handler scoping
```

### Context contract

```typescript
// frontend/src/lib/components/roving/context.ts

export interface RovingFocusContext {
  /** Explicit user-set focus target. Null means "no explicit selection — fall back to initial." */
  readonly focusedRunId: bigint | null
  /** First card in first non-empty column, or null when all columns are empty. $derived from runStore. */
  readonly initialFocusRunId: bigint | null
  /** Effective focus target: focusedRunId ?? initialFocusRunId. Used for tabindex derivation. */
  readonly currentFocusRunId: bigint | null
  /** Whether document focus is currently inside the kanban subtree. Toggled by focusin/focusout. */
  readonly kanbanHasFocus: boolean

  /** Set the explicit focus target. Pass null to clear and fall back to initialFocusRunId. */
  setFocus(id: bigint | null): void
  /** Restore focus to first card in first non-empty column. Used by eviction + lost-trigger paths. */
  restoreFocusToInitial(): void
}

export const ROVING_CONTEXT_KEY: unique symbol
export function setRovingContext(ctx: RovingFocusContext): void
export function getRovingContext(): RovingFocusContext
```

### Geometry contract

```typescript
// frontend/src/lib/components/roving/geometry.ts

export type ColIdx = 0 | 1 | 2
export type Position = { col: ColIdx; row: number }
export type Columns = readonly [
  readonly WorkflowRun[],   // queued
  readonly WorkflowRun[],   // inProgress
  readonly WorkflowRun[],   // completed
]
export type ArrowKey = 'ArrowUp' | 'ArrowDown' | 'ArrowLeft' | 'ArrowRight' | 'Home' | 'End'

export function locate(runId: bigint | null, columns: Columns): Position | null
export function nextNonEmptyColumn(from: ColIdx, dir: -1 | 1, columns: Columns): ColIdx | null
export function clampRow(targetCol: ColIdx, desiredRow: number, columns: Columns): number
export function runIdAt(pos: Position, columns: Columns): bigint | null
export function resolveTarget(
  currentRunId: bigint | null,
  key: ArrowKey,
  columns: Columns,
): bigint | null
```

`locate` is O(n) where n is total visible runs; at dashboard scale (typically <100), single-digit microseconds per keypress. The alternative (a `Map<RunId, Position>` `$derived` rebuilt on every WS event) shifts cost to the wrong axis for our access pattern.

### Action contract

```typescript
// frontend/src/lib/components/roving/action.ts

export function roving(node: HTMLElement, ctx: RovingFocusContext): {
  destroy(): void
}
```

The action installs three event listeners on `node`:

- **`focusin`** — sets `ctx.kanbanHasFocus = true`. If `event.target` is inside a `.run-card-activate`, reads the ancestor `[data-run-id]`, parses to `bigint`, calls `ctx.setFocus(id)` to keep `focusedRunId` in sync with whatever card just received focus (Tab into kanban, click, programmatic focus from panel close).
- **`focusout`** — if `event.relatedTarget` is not contained by `node`, sets `ctx.kanbanHasFocus = false`. Skipped when relatedTarget is still inside the kanban (intra-kanban focus moves don't toggle the flag).
- **`keydown`** — modifier-guard-first: returns immediately on `metaKey || ctrlKey || altKey || shiftKey` (delegates Cmd+K/D/\ and shift/Cmd+arrow to App.svelte's window-level handler and the browser default). Then matches `event.key` against `ArrowUp/Down/Left/Right/Home/End`. Calls `resolveTarget(ctx.currentFocusRunId, key, columnsSnapshot())`. If the target is non-null and different from current, calls `ctx.setFocus(targetRunId)` and `event.preventDefault()`. Does NOT call `.focus()` directly — RunCard's mount/update `$effect` handles the actual focus call once `currentFocusRunId` propagates.

`columnsSnapshot()` is a small inline helper that reads `runStore.queuedRuns`, `inProgressRuns`, `completedRuns` and returns them as a `Columns` tuple per keypress.

### Provider component

```svelte
<!-- frontend/src/lib/components/roving/RovingFocusProvider.svelte -->
<script lang="ts">
  import { tick } from 'svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { setRovingContext, type RovingFocusContext } from './context'
  import { locate, type Columns } from './geometry'
  import { roving } from './action'

  let { children } = $props()

  let focusedRunId = $state<bigint | null>(null)
  let kanbanHasFocus = $state(false)

  const initialFocusRunId = $derived<bigint | null>(
    runStore.queuedRuns[0]?.id
      ?? runStore.inProgressRuns[0]?.id
      ?? runStore.completedRuns[0]?.id
      ?? null
  )
  const currentFocusRunId = $derived(focusedRunId ?? initialFocusRunId)

  function columnsSnapshot(): Columns {
    return [runStore.queuedRuns, runStore.inProgressRuns, runStore.completedRuns]
  }

  function restoreFocusToInitial(): void {
    focusedRunId = null
    const target = initialFocusRunId
    if (target === null) return
    tick().then(() => {
      const el = document.querySelector<HTMLElement>(
        `.run-card[data-run-id="${target}"] .run-card-activate`
      )
      el?.focus()
    })
  }

  const ctx: RovingFocusContext = {
    get focusedRunId() { return focusedRunId },
    get initialFocusRunId() { return initialFocusRunId },
    get currentFocusRunId() { return currentFocusRunId },
    get kanbanHasFocus() { return kanbanHasFocus },
    setFocus(id) { focusedRunId = id },
    restoreFocusToInitial,
  }
  setRovingContext(ctx)

  // Eviction: focusedRunId points to a run no longer in any column → restore.
  $effect(() => {
    if (focusedRunId !== null && locate(focusedRunId, columnsSnapshot()) === null) {
      restoreFocusToInitial()
    }
  })
</script>

<div class="contents" use:roving={ctx}>
  {@render children()}
</div>
```

The `contents` Tailwind class collapses the wrapper from layout (`display: contents`) so existing flex/grid/min-h-0 behavior in `AppShell` and `KanbanBoard` is unaffected. Internal kanbanHasFocus updates are driven by the action listening on this same `<div>` element.

### App.svelte tree change

```svelte
<ConnectionManager />
<RovingFocusProvider>
  <AppShell><KanbanBoard /></AppShell>
  <CommandPalette />
  <RunDetailPanel />
</RovingFocusProvider>
```

`CommandPalette` and `RunDetailPanel` are intentionally inside the provider. Svelte context propagates by component tree, not DOM tree, so the Bits UI portals these dialogs use to mount their DOM into `<body>` do not break `getRovingContext()`. RunDetailPanel needs the context to call `restoreFocusToInitial()` from `onCloseAutoFocus`; CommandPalette does not currently consume it, but inclusion is free and future-proof.

### RunCard modifications

```svelte
<!-- frontend/src/lib/components/RunCard.svelte (modifications only) -->
<script lang="ts">
  import { getRovingContext } from './roving/context'
  // ...existing imports...

  const ctx = getRovingContext()
  const isFocused = $derived(ctx.currentFocusRunId === run.id)
  let buttonEl: HTMLButtonElement | undefined = $state()

  $effect(() => {
    if (isFocused && ctx.kanbanHasFocus && buttonEl) {
      buttonEl.focus()
    }
  })
</script>

<article class="run-card" bind:this={articleEl} data-run-id={run.id} ...>
  <button
    bind:this={buttonEl}
    class="run-card-activate"
    type="button"
    tabindex={isFocused ? 0 : -1}
    aria-label={ariaLabel}
    onclick={handleActivate}
  ></button>
  <!-- ...existing children... -->
</article>
```

The `$effect` handles two scenarios with one mechanism: (1) user-initiated arrow nav, where the action calls `ctx.setFocus(targetId)` and the new card's `isFocused` flips to true; (2) cross-column move via crossfade, where a fresh RunCard mounts with `isFocused === true` because `currentFocusRunId === run.id` is unchanged — the effect runs on mount and focuses the new bound button before the outgoing crossfade completes. The `kanbanHasFocus` guard prevents focus-stealing on initial page load and during eviction-while-focus-is-elsewhere scenarios.

The `tabindex` attribute is `$derived`-driven, declarative, and follows the component naturally if the DOM shape is later refactored.

### RunDetailPanel onCloseAutoFocus rewrite

```typescript
// frontend/src/lib/components/RunDetailPanel.svelte (callback only)
const ctx = getRovingContext()

onCloseAutoFocus={(event) => {
  if (uiStore.selectedRunId !== null) return  // panel closing for a different reason; defer

  const triggerId = uiStore.lastTriggerRunId
  uiStore.lastTriggerRunId = null
  if (triggerId === null) return  // no trigger recorded; let browser handle restoration

  event.preventDefault()
  const trigger = document.querySelector<HTMLElement>(
    `.run-card[data-run-id="${triggerId}"] .run-card-activate`
  )
  if (trigger !== null) {
    trigger.focus()  // happy path: source card still mounted
  } else {
    ctx.restoreFocusToInitial()  // bug fix: source evicted while panel open
  }
}}
```

The new `else` branch is the existing-bug fix. Previously the optional-chained `?.focus()` short-circuited on null and left focus on `<body>` because `event.preventDefault()` had already run. Routing through `restoreFocusToInitial()` ensures focus always lands on a visible card.

### Suspension is structural

The action installs `keydown` via `node.addEventListener('keydown', handler)` on the provider's `<div class="contents">`. Bubble-phase scoping means the handler only fires when the focused element is a descendant of that node. When `CommandPalette` opens, Bits UI moves focus into the palette input (which is portaled into `<body>` — outside the provider's `<div>` in DOM terms, but reachable from the same context tree). When `RunDetailPanel` opens, Bits UI's Sheet focus trap moves focus into the Sheet content (also portaled, also outside the provider's `<div>`). Either way, focus leaves the kanban subtree, the kanban listener silences naturally, and the dialogs own the keyboard. No explicit suspension flag, no coordination with `paletteStore.paletteOpen` or `uiStore.selectedRunId`.

### Lost-trigger restoration is centralized

`ctx.restoreFocusToInitial()` is the single source of truth for "involuntary focus loss → first card in first non-empty column." Two callers:

1. **Eviction during keyboard nav.** The provider's `$effect` watches `focusedRunId` against `locate()`; if locate returns null, the focused run was evicted (TTL eviction or status transition past completion's retention window). Calls `restoreFocusToInitial()`.
2. **Panel close with evicted source.** `RunDetailPanel.onCloseAutoFocus` calls it when the trigger-card querySelector returns null.

Both end at the same target via the same code path. Symmetric by design.

### System boundaries

All new code lives within `frontend/`. No backend changes. No new ts-rs-generated types. No new shadcn-svelte components. No new design tokens. The existing OKLCH `--accent` / `--accent-foreground` tokens cover the focus ring (already in use for `.run-card-activate`'s `:focus-visible` outline at `RunCard.svelte:217-221`).

## Existing Patterns

The design follows established codebase patterns:

- **Pure-leaf decomposition** — `roving/geometry.ts` is a pure-function module mirroring `lib/format/duration-text.ts`, `lib/format/runners.ts`, `lib/format/status-key.ts`, and `lib/filters/pool.ts`. Inputs, outputs, no store reads inside, no DOM access. Trivially unit-testable with hand-built `Columns` fixtures.
- **`.svelte` + `.test.ts` per-component sibling tests** — every new file gets a sibling test. Established in Sub-Phases 2–5 and reaffirmed by `frontend/CLAUDE.md`.
- **Vitest projects split** — jsdom for unit tests covering pure functions and rune-state behavior; browser-mode (`*.browser.test.ts`) for DOM-state assertions that require a real browser (focus events, computed styles, transition lifecycle).
- **`bind:this` element refs** — RunCard already uses `bind:this={articleEl}` for HoverPeekPopover anchoring; adding `bind:this={buttonEl}` for the focus call follows the existing pattern (RunCard.svelte:65, 174).
- **`data-run-id` PascalCase status attribute** — preserved on `<article>` (RunCard.svelte:175-176). The CSS selector in `restoreFocusToInitial()` (`'.run-card[data-run-id="${id}"] .run-card-activate'`) reuses the same selector RunDetailPanel already uses at line 60.
- **`.run-card-activate` inner button as the focus target** — Sub-Phase 5 established this; the `:focus-visible` outline is already styled (RunCard.svelte:217-221). Adding `tabindex` to it does not change the activation contract.
- **`uiStore.lastTriggerRunId` for focus restoration** — established in Sub-Phase 5; the new design extends the restoration logic without changing the protocol. RunCard still sets `lastTriggerRunId = run.id` in `handleActivate` (RunCard.svelte:167); RunDetailPanel still reads it in `onCloseAutoFocus`.
- **`e2e/lib/ws-mock.ts` harness** — `makeRunEvent`, `makeJobSeqEvent`, `sendWS`, plus the `window.__stores` dev-mode bridge for direct store manipulation. New E2E tests use this harness for the eviction and reorder scenarios.
- **Bits UI dialog stacking semantics** — `CommandPalette` and `RunDetailPanel` keep `escapeKeydownBehavior="defer-otherwise-close"` and `interactOutsideBehavior="defer-otherwise-close"`. This design does not modify their stacking behavior; it only adds the provider as a parent in the component tree.
- **Conventional Commits + lefthook three-tier hooks** — phase commits follow `feat(frontend):` / `test(frontend):` / `docs(design):` / `docs(ideation):` prefixes per `.commitlintrc.mjs`.
- **5-store rune-class architecture** — preserved. Roving state is component-scoped via Svelte context, not module-singleton store. The README's revised "5 stores is the ceiling, with PaletteStore as the empirical exception" principle applies: roving state's lifecycle (component-scoped, dies with the kanban) fails the test for joining UIStore or becoming a 6th store.

The design diverges from existing patterns in two minor, documented ways:

1. **First Svelte 5 action in the codebase.** Prior phases used component composition + store reactivity for behavior; the `roving` action is the first place a Svelte 5 action attaches behavior to a DOM element. The action signature follows Svelte's standard contract (`(node, params) => { destroy }`). No ADR needed — actions are first-class Svelte primitives, not a divergence; this just records that this is the first usage. If future features warrant a second action (e.g., a generic `keyboard-grid` action extracted from this one), the pattern is established.
2. **First `setContext`/`getContext` use in the codebase.** All prior cross-component state has flowed via stores or props. Context here is justified by the same lifecycle argument as the store-ceiling principle: roving state is scoped to "the kanban exists in the tree" and should die with it. ts-rs-generated types are not affected.

## Implementation Phases

Four phases on the `feat/kanban-keyboard-nav` branch, all merged in one PR. Each phase ends with a green local test run and a commit; later phases depend on earlier phases.

<!-- START_PHASE_1 -->
### Phase 1: Roving module foundation

**Goal:** Land the pure-function geometry, the context shape, and the action — all with unit-test coverage. No app integration yet, no user-visible change.

**Components:**

- `frontend/src/lib/components/roving/context.ts` — `RovingFocusContext` interface (per Architecture), `ROVING_CONTEXT_KEY` symbol, `setRovingContext` / `getRovingContext` accessors with throwing get-when-missing.
- `frontend/src/lib/components/roving/context.test.ts` — jsdom unit tests: get-without-set throws; setRovingContext stores; getRovingContext retrieves; symbol-keyed isolation from other contexts.
- `frontend/src/lib/components/roving/geometry.ts` — pure functions: `locate`, `nextNonEmptyColumn`, `clampRow`, `runIdAt`, `resolveTarget`. Type exports: `ColIdx`, `Position`, `Columns`, `ArrowKey`.
- `frontend/src/lib/components/roving/geometry.test.ts` — jsdom unit tests with hand-built mock `Columns`. Coverage matrix: every direction × no-wrap edge × empty-column-skip × asymmetric-column-clamp × null currentRunId fallback × runId-not-in-columns return null.
- `frontend/src/lib/components/roving/action.ts` — `roving` Svelte 5 action. Listeners: focusin, focusout, keydown. Modifier-guard-first in keydown.
- `frontend/src/lib/components/roving/action.test.ts` — jsdom unit tests with mock context: each listener mutates context correctly; modifier delegation returns early; key-not-in-arrow-set is no-op; data-run-id parse failure is no-op (defensive).

**Dependencies:** None (first phase).

**Done when:** `pnpm test` passes for the four new test files; `pnpm check` passes (no new TypeScript errors); no app integration yet (`App.svelte`, `RunCard.svelte`, `RunDetailPanel.svelte` unchanged). Covers `kanban-keyboard-nav.AC2.*` and `kanban-keyboard-nav.AC3.*` at the unit level (geometry resolution); covers `kanban-keyboard-nav.AC4.1` (modifier delegation early-return) at the unit level.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Provider wrap and tabindex integration

**Goal:** Wrap the app subtree in `<RovingFocusProvider>`, wire `RunCard` to the context for tabindex, and verify initial focus + tabindex correctness without yet activating any keyboard behavior.

**Components:**

- `frontend/src/lib/components/roving/RovingFocusProvider.svelte` — wrapper component per Architecture. Owns `focusedRunId` + `kanbanHasFocus` rune state. Exposes context per the contract. Includes the eviction `$effect` (validates `locate`, calls `restoreFocusToInitial` on null).
- `frontend/src/lib/components/roving/RovingFocusProvider.browser.test.ts` — browser-mode test: mount provider with a fixture, dispatch focusin/focusout on inner button, assert `kanbanHasFocus` toggles correctly; assert `currentFocusRunId` falls back to `initialFocusRunId` when `focusedRunId` is null; assert eviction `$effect` calls `restoreFocusToInitial` when a focused run is removed from the columns.
- `frontend/src/App.svelte` — wrap the existing `<AppShell>`/`<CommandPalette>`/`<RunDetailPanel>` block in `<RovingFocusProvider>`. ConnectionManager stays outside (it's a service component, no DOM, no children).
- `frontend/src/lib/components/RunCard.svelte` — import `getRovingContext`, derive `isFocused`, add `bind:this={buttonEl}`, change inner button `tabindex` to `{isFocused ? 0 : -1}`.
- `frontend/src/lib/components/RunCard.test.ts` — extend with tabindex-derivation tests using a mock context: focused-run gets `tabindex=0`, non-focused gets `tabindex=-1`; tabindex flips when context's `currentFocusRunId` changes.
- `frontend/src/lib/components/KanbanColumn.test.ts` — add a test that confirms exactly one card has `tabindex=0` per kanban (initial-focus invariant) when the provider is mounted with mock data.

**Dependencies:** Phase 1 (context, action, geometry must exist).

**Done when:** `pnpm test` and `pnpm test:e2e` both pass with no regressions to existing 547 unit/browser + 79 E2E tests. First card of first non-empty column has `tabindex=0` after WS connect. Clicking a card still opens the panel (existing behavior preserved). Tab into the kanban via browser default focuses the first card. Covers `kanban-keyboard-nav.AC1.*` (initial focus + tabindex correctness).
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Keyboard navigation activation

**Goal:** Activate the action's keydown handler with full geometry routing, modifier delegation, and natural suspension when dialogs open.

**Components:**

- `frontend/src/lib/components/roving/action.ts` — already created in Phase 1. No code changes here unless Phase 2 surfaced a contract gap. Phase 3's "activation" is about end-to-end verification, not new code.
- `frontend/e2e/kanban-keyboard-nav.test.ts` — new E2E test file. Scenarios:
  - Tab into kanban from elsewhere on page → first card receives focus
  - ArrowDown moves focus to next card; ArrowUp to previous; no-wrap at edges
  - ArrowLeft/Right move between non-empty columns; skip empty columns
  - Asymmetric columns: ArrowLeft from row 5 of a 10-card column to a 3-card column lands on row 2 (clamped to last)
  - Home/End jump to first/last card in current column
  - Cmd+K with focus on a card opens palette (kanban listener silent for modifier keys)
  - Cmd+D toggles dark mode while focus on card (modifier delegation)
  - With `selectedRunId` set via `__stores` bridge, focus is in Sheet, ArrowDown does NOT move kanban focus (suspension via natural focus scoping)
  - With `paletteStore.paletteOpen = true`, focus is in palette input, ArrowDown does NOT move kanban focus

**Dependencies:** Phase 2 (provider + tabindex must exist for keyboard nav to be observable).

**Done when:** All E2E scenarios above pass. `pnpm test:e2e` ships green. Covers `kanban-keyboard-nav.AC2.*` (2D nav at the integration level), `kanban-keyboard-nav.AC3.*` (edge behavior), `kanban-keyboard-nav.AC4.*` (modifier delegation), `kanban-keyboard-nav.AC5.*` (suspension).
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Card-stable transitions, lost-trigger restoration, README split, playground, docs

**Goal:** Land the cross-column re-focus mechanic, the existing `onCloseAutoFocus` bug fix, the Sub-Phase 6 README split, the playground, and complete the Documents to Update pass.

**Components:**

- `frontend/src/lib/components/RunCard.svelte` — already has the mount-time `$effect` from Phase 2. Phase 4 verifies it via tests rather than adding new code.
- `frontend/src/lib/components/RunCard.browser.test.ts` — extend with cross-column re-focus on mount: render a fixture with two columns, focus a card in column A by setting context's `focusedRunId` and dispatching focusin, mutate the column arrays to move the run to column B, await `tick()`, assert `document.activeElement === <new buttonEl in column B>` and `<old buttonEl>` is no longer focused.
- `frontend/src/lib/components/RunDetailPanel.svelte` — rewrite `onCloseAutoFocus` per Architecture. Import `getRovingContext`, branch on `trigger === null` to call `ctx.restoreFocusToInitial()`.
- `frontend/src/lib/components/RunDetailPanel.test.ts` — extend with the bug-fix regression: open panel with `selectedRunId = X`, `lastTriggerRunId = X`, then evict X from `runStore`, then close panel; assert `restoreFocusToInitial` was called and focus landed on the first card of the first non-empty column.
- `frontend/e2e/kanban-keyboard-nav.test.ts` — extend with end-to-end scenarios:
  - Card-stable reorder: focus a Queued card, fire `WorkflowRunStarted` via `sendWS`, assert focus migrates to the same run in InProgress (verified via `data-run-id` of `document.activeElement`)
  - Eviction during keyboard nav: focus a card, evict it via `sendWS`, assert focus restores to first card in first non-empty column
  - Panel close with evicted source (regression test for existing bug): open panel via Cmd+K → run, evict source via `sendWS` while panel open, close panel via Esc, assert focus is on first card in first non-empty column (NOT body)
- `docs/ideation/ui-decomposition/README.md` — replace the existing `### Sub-Phase 6: Polish + Responsive` section with two subsections per the established Sub-Phases 1–5 pattern:
  - `### Sub-Phase 6a: Kanban Keyboard Navigation ✅ COMPLETE` with PR# (filled in by the final commit on this branch), per-design-plan link, and "What was built" listing the deliverables
  - `### Sub-Phase 6b: Polish + Responsive` (renamed; goal line trimmed to drop "roving-tabindex keyboard navigation"; bulleted list trimmed to remove the roving-tabindex item and its nested implementation note; tests bullet trimmed to remove roving-tabindex E2E coverage)
- `docs/architecture/frontend-app.md` — add a Roving Focus section: context shape, geometry contract, action lifecycle, kanbanHasFocus mechanic, eviction + lost-trigger restoration, RunCard integration. Update the Component Tree diagram to show `<RovingFocusProvider>` between `App.svelte` root and `AppShell` / `CommandPalette` / `RunDetailPanel`. Bump "Last verified" to 2026-05-01.
- `frontend/CLAUDE.md` — pointer-level update: add `lib/components/roving/` directory to the Key Files table; add `RovingFocusProvider.svelte` row; update the `RunCard.svelte` row to mention the new tabindex/focus integration; update the Status section to reference Sub-Phase 6a completion. No architectural detail — that lives in `docs/architecture/frontend-app.md`.
- `scripts/doc-mapping.sh` — add new mappings: `frontend/src/lib/components/roving/*` → `docs/architecture/frontend-app.md`; `frontend/src/lib/components/roving/RovingFocusProvider.svelte` → same.
- `docs/design-plans/playgrounds/2026-05-01-kanban-keyboard-nav-explorer.html` — NEW. Self-contained HTML playground produced via `/impeccable:frontend-design` (visual quality) + `/playground:playground` (single-file wrapper). Scope: theme + mode + density picker; static three-column kanban with mock cards; live arrow-key driving via inlined geometry; "simulate reorder" button; asymmetric-column toggle. Excludes WS mocking, shadcn-svelte components, Tailwind v4 — framework-free focus-behavior explorer.
- `docs/test-plans/2026-05-01-kanban-keyboard-nav.md` — NEW. AC traceability matrix mirroring Sub-Phase 4 + 5 patterns. Maps each `kanban-keyboard-nav.AC*` to its automated test file. Posted as the first PR comment per project convention; never committed inside the PR description.

**Dependencies:** Phase 3 (keyboard nav must work end-to-end before card-stable + restoration tests are meaningful).

**Done when:** All phase E2E tests pass including the regression test for the existing `onCloseAutoFocus` bug. Playground HTML committed and renders correctly. README diff applied. All Documents to Update entries complete. Covers `kanban-keyboard-nav.AC6.*` (card-stable reorder) and `kanban-keyboard-nav.AC7.*` (lost-trigger restoration).
<!-- END_PHASE_4 -->

## Documents to Update

Per `.ed3d/design-plan-guidance.md` rule 6, every design plan must list the documents that change alongside implementation:

| Document | What changes |
|----------|--------------|
| `docs/architecture/frontend-app.md` | Add Roving Focus section (context shape, geometry, action lifecycle, kanbanHasFocus mechanic, eviction + lost-trigger restoration, RunCard integration). Update Component Tree to show `<RovingFocusProvider>` wrapping AppShell/CommandPalette/RunDetailPanel. Note RunCard's new `bind:this` / `$effect` / tabindex pattern. Bump "Last verified" to 2026-05-01. |
| `frontend/CLAUDE.md` | Pointer-level update: add `lib/components/roving/` to the Key Files table, add `RovingFocusProvider.svelte` row, update `RunCard.svelte` row to mention tabindex/focus integration, update Status section after merge with Sub-Phase 6a completion. Architectural detail lives in `docs/architecture/frontend-app.md`. |
| `docs/ideation/ui-decomposition/README.md` | Replace `### Sub-Phase 6: Polish + Responsive` with two subsections: `### Sub-Phase 6a: Kanban Keyboard Navigation ✅ COMPLETE` (Sub-Phases 1–5 ✅ pattern; PR# placeholder filled in by the final commit on this branch) + `### Sub-Phase 6b: Polish + Responsive` (renamed; roving-tabindex item, library-survey implementation note, and roving-related test bullets removed). |
| `frontend/src/App.svelte` | Wrap `<AppShell>`/`<CommandPalette>`/`<RunDetailPanel>` block in `<RovingFocusProvider>`. ConnectionManager stays outside the provider. |
| `frontend/src/lib/components/RunCard.svelte` | Add `bind:this={buttonEl}`, `let buttonEl: HTMLButtonElement | undefined = $state()`, `tabindex={isFocused ? 0 : -1}` on `.run-card-activate`, mount-time `$effect` calling `buttonEl.focus()` when `isFocused && ctx.kanbanHasFocus`. Import `getRovingContext` and derive `isFocused`. |
| `frontend/src/lib/components/RunDetailPanel.svelte` | Bug fix: rewrite `onCloseAutoFocus` (current lines 52–66) to call `ctx.restoreFocusToInitial()` when the trigger-card querySelector returns null. Import `getRovingContext` and bind `ctx` at component scope. |
| `scripts/doc-mapping.sh` | Add mappings: `frontend/src/lib/components/roving/*` → `docs/architecture/frontend-app.md`; `frontend/src/lib/components/roving/RovingFocusProvider.svelte` → same. |
| `docs/design-plans/playgrounds/2026-05-01-kanban-keyboard-nav-explorer.html` | NEW. Self-contained focus-behavior explorer. Theme + mode + density picker; static three-column kanban with mock cards; live arrow-key driving; simulate-reorder + asymmetric-column toggles. Created via `/impeccable:frontend-design` + `/playground:playground`. |
| `docs/test-plans/2026-05-01-kanban-keyboard-nav.md` | NEW. AC traceability matrix. Posted as first PR comment per project convention; never committed inside the PR description. |

## Additional Considerations

**Why context, not a 6th store.** The README's revised principle ("5 stores is the ceiling, with PaletteStore as the empirical exception") applies to roving state. PaletteStore split from UIStore because palette state has fundamentally different lifecycle properties — high-frequency mutation per keystroke, ephemeral session-scoped recent-items tracking, submenu state that doesn't survive logical navigation. Roving state has a different lifecycle yet again — it's component-scoped (dies when the kanban unmounts, hypothetically; the kanban is a singleton today but the design shouldn't lock that in), and it doesn't need to survive any persistence boundary. Folding into UIStore would mix preference-state semantics with transient-state semantics that don't even share a lifetime. A new store would fight the README's principle without empirical justification. Svelte context is the textbook fit: component-tree-scoped state, propagates by composition, dies with the provider. No persistence, no `localStorage`, no `sessionStorage`.

**Existing-bug fix scope.** The `RunDetailPanel.onCloseAutoFocus` bug at lines 52–66 (preventDefault + null-safe `?.focus()` leaving focus on body when the trigger is missing) was discovered while reading the code for this design. Fixing it inside this plan is the right call: the fix uses the same `restoreFocusToInitial()` mechanism the eviction case needs, so the alternative (a separate PR with its own restoration logic, then this PR layering on top) would create two short-lived restoration code paths. Bundling means one mechanism, one regression test, one merge.

**Library re-evaluation conclusion.** The original Sub-Phase 5 deferral note flagged `jakelazaroff/roving-tabindex` with "Vite/Tailwind v4 integration risk" and `svelte-roving-ux` as "Svelte-4-era and unmaintained for runes." Re-evaluation in this design plan (via `internet-researcher` + GitHub source reads) updated those findings:

- `jakelazaroff/roving-tabindex` v0.3.3 (Dec 2024) is a **light-DOM** web component, not shadow-DOM. Tailwind v4 utilities pass through cleanly. Zero dependencies, native `direction="grid"` mode, ~300–400 LOC core. The "Vite/Tailwind v4 integration risk" framing was incorrect.
- `svelte-roving-ux` (v1.1.0, July 2023, 18+ months idle) remains stale. No Svelte 5 work, no 2D grid support. Verdict unchanged.

The custom Svelte 5 implementation is preferred not because the library is unviable but because (a) the ATC-specific edge cases (asymmetric columns, cross-column re-focus across crossfade) are our problem regardless of approach, costing ~110 LOC of adapter under the library path; (b) custom is ~170 LOC end-to-end; (c) the imperative-DOM-bridge pattern that vendoring a web component would require has no peer in the codebase, while the custom path uses Svelte 5 idioms that match RunCard, the stores, and Sub-Phase 5's existing architecture. The 60-LOC delta does not justify introducing a one-off pattern.

**Reduced motion.** The design adds no animations. The focus ring is the existing `:focus-visible` outline at `RunCard.svelte:217-221` (`outline: 2px solid var(--accent); outline-offset: -2px; border-radius: 8px`). No transitions on focus changes; respects `prefers-reduced-motion` by virtue of being motion-free.

**Performance.** `locate()` is O(n) where n is total visible runs (~100 typical). Per-keypress cost is single-digit microseconds. The provider's eviction `$effect` runs whenever any column derived array changes — i.e., on every WS event affecting runs. Inside the effect, `locate()` runs once if `focusedRunId !== null`. At a worst-case burst of 10 events/RAF (per `dispatcher.ts` batching), that's 10 × 100 = 1000 array comparisons per frame — still negligible. No micro-optimization warranted at current scale.
