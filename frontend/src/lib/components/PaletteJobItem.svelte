<script lang="ts">
  import * as Command from '$lib/components/ui/command'
  import StatusIcon from './StatusIcon.svelte'
  import { resolveJobStatusKey, statusKeyToVar } from '$lib/format/status-key'
  import type { StatusKey } from '$lib/format/status-key'
  import type { Job } from '$lib/types/generated/Job'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  export interface Props {
    job: Job
    parentRun: WorkflowRun
    onSelect: () => void
  }

  let { job, parentRun, onSelect }: Props = $props()
  const statusKey: StatusKey = $derived(resolveJobStatusKey(job))
</script>

<Command.Item
  value={`job-${job.id}`}
  keywords={[parentRun.repo, parentRun.displayTitle, job.name]}
  {onSelect}
>
  <span class="status" style="--status-color: var(--{statusKeyToVar(statusKey)});">
    <StatusIcon value={statusKey} />
  </span>
  <span class="title">{job.name}</span>
  <span class="meta">in {parentRun.displayTitle}</span>
</Command.Item>

<style>
  .status {
    font-size: 0.85em;
  }

  .meta {
    color: var(--text-quiet);
    margin-left: auto;
    min-width: 24ch;
    text-align: right;
  }
</style>
