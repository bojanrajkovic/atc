<script lang="ts">
  import * as Command from '$lib/components/ui/command'
  import StatusIcon from './StatusIcon.svelte'
  import { resolveStatusKey, statusKeyToVar } from '$lib/format/status-key'
  import type { StatusKey } from '$lib/format/status-key'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  export interface Props {
    run: WorkflowRun
    onSelect: () => void
    /**
     * Section prefix for the Command.Item's `value` field. cmdk uses `value`
     * as a unique selection key; the same run can appear in both Recent and
     * Runs sections, so the parent must pass a prefix that distinguishes the
     * two render paths. Defaults to `'run'` for the canonical Runs section.
     */
    valuePrefix?: string
  }

  let { run, onSelect, valuePrefix = 'run' }: Props = $props()
  const statusKey: StatusKey = $derived(resolveStatusKey(run))
</script>

<Command.Item
  value={`${valuePrefix}-${run.id}`}
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
