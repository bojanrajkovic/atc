<script lang="ts">
  import { tick } from 'svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import { filterRunsByPool } from '$lib/filters/pool'
  import { setRovingContext, type RovingFocusContext } from './context'
  import { locate, type Columns } from './geometry'

  interface Props {
    children: import('svelte').Snippet
  }
  let { children }: Props = $props()

  let focusedRunId: bigint | null = $state(null)
  let kanbanHasFocus: boolean = $state(false)

  /**
   * Single source of truth for the visible kanban columns — mirrors what the
   * DOM renders. Derived from runStore columns filtered by uiStore.activePoolFilter
   * so that geometry resolution, initialFocusRunId, and the eviction $effect all
   * agree with the rendered DOM even under an active pool filter.
   */
  const visibleColumns = $derived<Columns>([
    filterRunsByPool(runStore.queuedRuns, runStore.jobsByRunId, uiStore.activePoolFilter),
    filterRunsByPool(runStore.inProgressRuns, runStore.jobsByRunId, uiStore.activePoolFilter),
    filterRunsByPool(runStore.completedRuns, runStore.jobsByRunId, uiStore.activePoolFilter),
  ] as const satisfies Columns)

  const initialFocusRunId = $derived<bigint | null>(
    visibleColumns[0][0]?.id ?? visibleColumns[1][0]?.id ?? visibleColumns[2][0]?.id ?? null
  )
  const currentFocusRunId = $derived<bigint | null>(focusedRunId ?? initialFocusRunId)

  async function restoreFocusToInitial(): Promise<void> {
    focusedRunId = null
    await tick()
    // Capture initialFocusRunId AFTER tick so that runStore mutations from the same task
    // (TTL eviction, panel-close-evicted-source) have propagated through the $derived
    // visibleColumns / initialFocusRunId derivations. This is intentionally different from
    // the original design plan (which captured before tick); for the single-mutation case
    // both orderings give the same answer (SvelteMap.delete is synchronous), but for any
    // future caller that mutates inside the same microtask, capturing after tick is the
    // safer default.
    const target = initialFocusRunId
    if (target === null) return
    const el = document.querySelector<HTMLElement>(
      `.run-card[data-run-id="${target}"] .run-card-activate`
    )
    el?.focus()
  }

  const ctx: RovingFocusContext = {
    get focusedRunId() {
      return focusedRunId
    },
    get initialFocusRunId() {
      return initialFocusRunId
    },
    get currentFocusRunId() {
      return currentFocusRunId
    },
    get kanbanHasFocus() {
      return kanbanHasFocus
    },
    getVisibleColumns() {
      return visibleColumns
    },
    setFocus(id) {
      focusedRunId = id
    },
    setKanbanHasFocus(v) {
      kanbanHasFocus = v
    },
    restoreFocusToInitial,
  }

  setRovingContext(ctx)

  $effect(() => {
    if (focusedRunId === null) return
    if (locate(focusedRunId, visibleColumns) === null) {
      if (kanbanHasFocus) {
        // The focused card was evicted while the kanban owns focus — restore
        // focus to the new initialFocusRunId (the first visible card).
        void restoreFocusToInitial()
      } else {
        // Background eviction while focus is elsewhere (TopBar, palette, panel,
        // etc.) — reset roving state only. Do NOT call .focus(); that would yank
        // focus away from the user's current target.
        focusedRunId = null
      }
    }
  })
</script>

{@render children()}
