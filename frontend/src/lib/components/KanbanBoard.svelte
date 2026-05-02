<script lang="ts">
  import { poolKey } from '$lib/filters/pool'
  import { runnerStore } from '$lib/stores/runners.svelte'
  import { connectionStore } from '$lib/stores/connection.svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import KanbanColumn from './KanbanColumn.svelte'
  import PoolFilterPill from './PoolFilterPill.svelte'
  import { getRovingContext } from '$lib/components/roving/context'
  import { roving } from '$lib/components/roving/action'

  const ctx = getRovingContext()

  const totalRuns = $derived(
    runStore.queuedRuns.length + runStore.inProgressRuns.length + runStore.completedRuns.length
  )
  const jobStatsByRun = $derived(runStore.jobStatsByRun)
  const activePoolFilter = $derived(uiStore.activePoolFilter)
  const jobsByRunId = $derived(runStore.jobsByRunId)

  // Compute label display text from the active filter
  const activeFilterLabelText = $derived.by(() => {
    if (uiStore.activePoolFilter === null) return null
    // Find the matching pool in runnerStore to get its labels
    const matchingPool = runnerStore.pools.find(
      (p) => poolKey(p.labels) === uiStore.activePoolFilter
    )
    if (matchingPool) return [...matchingPool.labels].sort().join(' · ')
    // Fallback: split the PoolKey on '|' to recover labels (the brand value is sort-and-join of labels by '|')
    return (uiStore.activePoolFilter as string).split('|').join(' · ')
  })
</script>

{#if connectionStore.status !== 'connected' && totalRuns === 0}
  <!-- Hydration placeholder: no data yet, not connected -->
  <div class="flex items-center justify-center h-full" style="color: var(--text-dim);">
    <p class="text-sm">Connecting&hellip;</p>
  </div>
{:else if connectionStore.status === 'connected' && totalRuns === 0}
  <!-- Empty state: connected but no workflow runs -->
  <div class="flex items-center justify-center h-full" style="color: var(--text-dim);">
    <p class="text-sm">No workflows yet.</p>
  </div>
{:else}
  {#if uiStore.activePoolFilter !== null && activeFilterLabelText !== null}
    <header class="kanban-header">
      <PoolFilterPill
        labelText={activeFilterLabelText}
        onClear={() => {
          uiStore.activePoolFilter = null
        }}
      />
    </header>
  {/if}
  <!-- Three-column kanban grid -->
  <div use:roving={ctx} class="grid grid-cols-3 gap-4 h-full p-4" style="min-height: 0;">
    <KanbanColumn
      label="QUEUED"
      runs={runStore.queuedRuns}
      headingId="kanban-col-queued"
      {jobStatsByRun}
      {activePoolFilter}
      {jobsByRunId}
    />
    <KanbanColumn
      label="IN PROGRESS"
      runs={runStore.inProgressRuns}
      headingId="kanban-col-in-progress"
      {jobStatsByRun}
      {activePoolFilter}
      {jobsByRunId}
    />
    <KanbanColumn
      label="COMPLETED"
      runs={runStore.completedRuns}
      headingId="kanban-col-completed"
      {jobStatsByRun}
      {activePoolFilter}
      {jobsByRunId}
    />
  </div>
{/if}

<style>
  .kanban-header {
    padding: 0.5rem 1rem;
    display: flex;
    align-items: center;
  }
</style>
