<script lang="ts">
  import { flip } from 'svelte/animate'
  import { cubicOut } from 'svelte/easing'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import type { Job } from '$lib/types/generated/Job'
  import { filterRunsByPool, type PoolKey } from '$lib/filters/pool'
  import { DURATION_MOVE, receive, send } from '$lib/animations/kanban-transitions'
  import ColumnHeader from './ColumnHeader.svelte'
  import RunCard from './RunCard.svelte'

  let {
    label,
    runs,
    headingId,
    jobStatsByRun,
    activePoolFilter,
    jobsByRunId,
  }: {
    label: string
    runs: readonly WorkflowRun[]
    headingId: string
    jobStatsByRun: ReadonlyMap<bigint, JobStats>
    activePoolFilter: PoolKey | null
    jobsByRunId: ReadonlyMap<bigint, readonly Job[]>
  } = $props()

  // Apply pool filter — when activePoolFilter is null, returns runs unchanged (identity)
  const visibleRuns = $derived(filterRunsByPool(runs, jobsByRunId, activePoolFilter))

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
  <ColumnHeader {label} count={visibleRuns.length} {headingId} />
  <div role="list" class="flex flex-col gap-2 overflow-y-auto min-h-0 p-2">
    {#each visibleRuns as run (run.id)}
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
