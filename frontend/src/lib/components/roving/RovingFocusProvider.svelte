<script lang="ts">
  import { tick } from 'svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { setRovingContext, type RovingFocusContext } from './context'
  import { locate, type Columns } from './geometry'

  interface Props {
    children: import('svelte').Snippet
  }
  let { children }: Props = $props()

  let focusedRunId: bigint | null = $state(null)
  let kanbanHasFocus: boolean = $state(false)

  const columnsSnapshot = (): Columns =>
    [
      runStore.queuedRuns,
      runStore.inProgressRuns,
      runStore.completedRuns,
    ] as const satisfies Columns

  const initialFocusRunId = $derived<bigint | null>(
    runStore.queuedRuns[0]?.id ??
      runStore.inProgressRuns[0]?.id ??
      runStore.completedRuns[0]?.id ??
      null
  )
  const currentFocusRunId = $derived<bigint | null>(focusedRunId ?? initialFocusRunId)

  async function restoreFocusToInitial(): Promise<void> {
    focusedRunId = null
    await tick()
    // Capture initialFocusRunId AFTER tick so that runStore mutations from the same task
    // (TTL eviction, panel-close-evicted-source) have propagated through the $derived
    // queuedRuns/inProgressRuns/completedRuns arrays. This is intentionally different from
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
    if (locate(focusedRunId, columnsSnapshot()) === null) {
      void restoreFocusToInitial()
    }
  })
</script>

{@render children()}
