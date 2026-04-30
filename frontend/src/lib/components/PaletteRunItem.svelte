<script lang="ts">
  import * as Command from '$lib/components/ui/command'
  import StatusIcon from './StatusIcon.svelte'
  import { resolveStatusKey, statusKeyToVar } from '$lib/format/status-key'
  import type { StatusKey } from '$lib/format/status-key'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  export interface Props {
    run: WorkflowRun
    onSelect: () => void
  }

  let { run, onSelect }: Props = $props()
  const statusKey: StatusKey = $derived(resolveStatusKey(run))
</script>

<Command.Item
  value={`run-${run.id}`}
  keywords={[run.repo, run.branch ?? '', run.displayTitle]}
  {onSelect}
>
  <span class="status" style="--status-color: var(--{statusKeyToVar(statusKey)});">
    <StatusIcon value={statusKey} />
  </span>
  <span class="title">{run.displayTitle}</span>
  <span class="meta">{run.repo}{run.branch ? ` · ${run.branch}` : ''}</span>
</Command.Item>

<style>
  .meta {
    color: var(--text-quiet);
    margin-left: auto;
    min-width: 24ch;
    text-align: right;
  }
</style>
