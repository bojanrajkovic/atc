<script lang="ts">
  import { flip } from 'svelte/animate'
  import { cubicOut } from 'svelte/easing'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import { DURATION_MOVE, receive, send } from '$lib/animations/kanban-transitions'
  import ColumnHeader from './ColumnHeader.svelte'
  import RunCard from './RunCard.svelte'

  let {
    label,
    runs,
    headingId,
    jobStatsByRun,
  }: {
    label: string
    runs: readonly WorkflowRun[]
    headingId: string
    jobStatsByRun: ReadonlyMap<bigint, JobStats>
  } = $props()

  function requireJobStats(id: bigint): JobStats {
    const stats = jobStatsByRun.get(id)
    if (stats === undefined) {
      throw new Error(
        `jobStatsByRun total-map invariant broken: run ${id} has no JobStats entry. ` +
          `Every runId in runStore.runs must resolve to a JobStats via runStore.jobStatsByRun.`
      )
    }
    return stats
  }
</script>

<section aria-labelledby={headingId} class="flex flex-col min-h-0">
  <ColumnHeader {label} count={runs.length} {headingId} />
  <div role="list" class="flex flex-col gap-2 overflow-y-auto min-h-0 p-2">
    {#each runs as run (run.id)}
      <div
        role="listitem"
        animate:flip={{ duration: DURATION_MOVE, easing: cubicOut }}
        in:receive={{ key: run.id }}
        out:send={{ key: run.id }}
      >
        <RunCard {run} jobStats={requireJobStats(run.id)} />
      </div>
    {/each}
  </div>
</section>
