<script lang="ts">
  import RovingFocusProvider from '$lib/components/roving/RovingFocusProvider.svelte'
  import type { RovingFocusContext } from '$lib/components/roving/context'
  import type { PoolKey } from '$lib/filters/pool'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { Job } from '$lib/types/generated/Job'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import KanbanBoardInvariantHarnessInner from './KanbanBoardInvariantHarnessInner.svelte'

  interface Props {
    queuedRuns: readonly WorkflowRun[]
    inProgressRuns: readonly WorkflowRun[]
    completedRuns: readonly WorkflowRun[]
    jobStatsByRun: ReadonlyMap<bigint, JobStats>
    jobsByRunId: ReadonlyMap<bigint, readonly Job[]>
    activePoolFilter?: PoolKey | null
    onCtxReady?: (ctx: RovingFocusContext) => void
  }

  let {
    queuedRuns,
    inProgressRuns,
    completedRuns,
    jobStatsByRun,
    jobsByRunId,
    activePoolFilter = null,
    onCtxReady = () => {},
  }: Props = $props()
</script>

<RovingFocusProvider>
  <KanbanBoardInvariantHarnessInner
    {queuedRuns}
    {inProgressRuns}
    {completedRuns}
    {jobStatsByRun}
    {jobsByRunId}
    {activePoolFilter}
    {onCtxReady}
  />
</RovingFocusProvider>
