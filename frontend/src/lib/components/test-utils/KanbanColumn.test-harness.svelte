<script lang="ts">
  import RovingFocusProvider from '$lib/components/roving/RovingFocusProvider.svelte'
  import KanbanColumn from '../KanbanColumn.svelte'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { Job } from '$lib/types/generated/Job'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  interface Props {
    label: string
    headingId: string
    runs: readonly WorkflowRun[]
    jobStatsByRun: ReadonlyMap<bigint, JobStats>
    activePoolFilter: import('$lib/filters/pool').PoolKey | null
    jobsByRunId: ReadonlyMap<bigint, readonly Job[]>
  }
  let { label, headingId, runs, jobStatsByRun, activePoolFilter, jobsByRunId }: Props = $props()
</script>

<RovingFocusProvider>
  <KanbanColumn {label} {headingId} {runs} {jobStatsByRun} {activePoolFilter} {jobsByRunId} />
</RovingFocusProvider>
