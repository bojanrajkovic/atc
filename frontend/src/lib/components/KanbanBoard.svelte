<script lang="ts">
  import { connectionStore } from '$lib/stores/connection.svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import KanbanColumn from './KanbanColumn.svelte'

  const totalRuns = $derived(
    runStore.queuedRuns.length + runStore.inProgressRuns.length + runStore.completedRuns.length
  )
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
  <!-- Three-column kanban grid -->
  <div class="grid grid-cols-3 gap-4 h-full p-4" style="min-height: 0;">
    <KanbanColumn label="QUEUED" runs={runStore.queuedRuns} headingId="kanban-col-queued" />
    <KanbanColumn
      label="IN PROGRESS"
      runs={runStore.inProgressRuns}
      headingId="kanban-col-in-progress"
    />
    <KanbanColumn
      label="COMPLETED"
      runs={runStore.completedRuns}
      headingId="kanban-col-completed"
    />
  </div>
{/if}
