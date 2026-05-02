<script lang="ts">
  import { getRovingContext } from '$lib/components/roving/context'
  import type { RovingFocusContext } from '$lib/components/roving/context'
  import { roving } from '$lib/components/roving/action'
  import type { PoolKey } from '$lib/filters/pool'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { Job } from '$lib/types/generated/Job'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import KanbanColumn from '../KanbanColumn.svelte'

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

  // Called during init inside the provider's component tree — getRovingContext() succeeds here.
  const ctx = getRovingContext()

  $effect(() => {
    onCtxReady(ctx)
  })
</script>

<!-- use:roving attaches focusin/focusout/keydown listeners for keyboard nav testing -->
<div use:roving={ctx} data-testid="kanban-grid">
  <KanbanColumn
    label="QUEUED"
    headingId="kanban-col-queued"
    runs={queuedRuns}
    {jobStatsByRun}
    {activePoolFilter}
    {jobsByRunId}
  />
  <KanbanColumn
    label="IN_PROGRESS"
    headingId="kanban-col-in-progress"
    runs={inProgressRuns}
    {jobStatsByRun}
    {activePoolFilter}
    {jobsByRunId}
  />
  <KanbanColumn
    label="COMPLETED"
    headingId="kanban-col-completed"
    runs={completedRuns}
    {jobStatsByRun}
    {activePoolFilter}
    {jobsByRunId}
  />
</div>
