import { runStore } from '$lib/stores/runs.svelte'
import type { RovingFocusContext } from './context'
import { type ArrowKey, type Columns, resolveTarget } from './geometry'

// ---------------------------------------------------------------------------
// columnsSnapshot()
// ---------------------------------------------------------------------------

/**
 * Returns the current kanban columns as a Columns tuple from the runStore.
 * Reads are not in a reactive context (inside an event handler) so they
 * snapshot the current values at the time of the keypress.
 */
function columnsSnapshot(): Columns {
  return [
    runStore.queuedRuns,
    runStore.inProgressRuns,
    runStore.completedRuns,
  ] as const satisfies Columns
}

// ---------------------------------------------------------------------------
// Arrow-key set — used for early-return guard
// ---------------------------------------------------------------------------

const ARROW_KEYS = new Set<string>([
  'ArrowUp',
  'ArrowDown',
  'ArrowLeft',
  'ArrowRight',
  'Home',
  'End',
])

function isArrowKey(k: string): k is ArrowKey {
  return ARROW_KEYS.has(k)
}

// ---------------------------------------------------------------------------
// roving() — Svelte 5 action
// ---------------------------------------------------------------------------

/**
 * Svelte 5 action that wires focusin / focusout / keydown listeners onto the
 * kanban board node and drives the RovingFocusContext accordingly.
 *
 * Attach via `use:roving={ctx}` on the kanban board container.
 *
 * @param node  The HTMLElement the action is attached to.
 * @param ctx   The RovingFocusContext provided by RovingFocusProvider.
 * @returns     An action object with a destroy() method for cleanup.
 */
export function roving(node: HTMLElement, ctx: RovingFocusContext): { destroy(): void } {
  // -------------------------------------------------------------------------
  // focusin listener
  // -------------------------------------------------------------------------

  function onFocusin(event: FocusEvent): void {
    // Always signal that the kanban board has focus.
    ctx.setKanbanHasFocus(true)

    // Walk up from the event target to find the closest .run-card-activate element.
    const target = event.target
    if (!(target instanceof Element)) return

    const activateEl = target.closest('.run-card-activate')
    if (activateEl === null) {
      // Non-card focusable inside the action's node — skip the focus-id sync.
      return
    }

    // Walk up from the activate element to the closest ancestor with data-run-id.
    const runCardEl = activateEl.closest('[data-run-id]')
    if (runCardEl === null) return

    const rawId = runCardEl.getAttribute('data-run-id')
    if (rawId === null) return

    // Defensive parse — malformed data-run-id is a defect elsewhere; don't throw.
    let parsedRunId: bigint
    try {
      parsedRunId = BigInt(rawId)
    } catch {
      return
    }

    ctx.setFocus(parsedRunId)
  }

  // -------------------------------------------------------------------------
  // focusout listener
  // -------------------------------------------------------------------------

  function onFocusout(event: FocusEvent): void {
    const related = event.relatedTarget

    // Focus left the kanban subtree entirely (null or outside node).
    if (related === null || (related instanceof Node && !node.contains(related))) {
      ctx.setKanbanHasFocus(false)
    }
    // Otherwise focus moved within the kanban subtree — no-op.
  }

  // -------------------------------------------------------------------------
  // keydown listener
  // -------------------------------------------------------------------------

  function onKeydown(event: KeyboardEvent): void {
    // Modifier-guard FIRST — let App.svelte's window-level handler and the
    // browser handle modifier combos (Cmd+K, Cmd+Arrow scroll, etc.).
    if (event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) {
      return
    }

    const key = event.key

    // Non-arrow/home/end key — not our concern.
    if (!isArrowKey(key)) {
      return
    }

    const columns = columnsSnapshot()
    const resolved = resolveTarget(ctx.currentFocusRunId, key, columns)

    // Always preventDefault for claimed keys — suppresses browser-default
    // scrolling even on no-op edges (AC2.7).
    event.preventDefault()

    // Only update focus if the resolved target is non-null AND differs from
    // the current focus (avoids a spurious reactive write on no-op edges).
    if (resolved !== null && resolved !== ctx.currentFocusRunId) {
      ctx.setFocus(resolved)
    }
  }

  // -------------------------------------------------------------------------
  // Attach listeners
  // -------------------------------------------------------------------------

  node.addEventListener('focusin', onFocusin)
  node.addEventListener('focusout', onFocusout)
  node.addEventListener('keydown', onKeydown)

  // -------------------------------------------------------------------------
  // Teardown
  // -------------------------------------------------------------------------

  return {
    destroy(): void {
      node.removeEventListener('focusin', onFocusin)
      node.removeEventListener('focusout', onFocusout)
      node.removeEventListener('keydown', onKeydown)
    },
  }
}
