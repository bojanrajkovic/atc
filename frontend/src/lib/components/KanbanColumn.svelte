<script lang="ts">
  import { flip } from 'svelte/animate'
  import { cubicOut } from 'svelte/easing'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import { DURATION_MOVE, receive, send } from '$lib/animations/kanban-transitions'
  import ColumnHeader from './ColumnHeader.svelte'
  import RunCard from './RunCard.svelte'

  let {
    label,
    runs,
    headingId,
  }: {
    label: string
    runs: readonly WorkflowRun[]
    headingId: string
  } = $props()
</script>

<section aria-labelledby={headingId} class="flex flex-col min-h-0">
  <ColumnHeader {label} count={runs.length} {headingId} />
  <div role="list" class="flex flex-col gap-2 overflow-y-auto min-h-0 p-2">
    {#each runs as run (run.id)}
      <article
        role="listitem"
        data-run-id={String(run.id)}
        animate:flip={{ duration: DURATION_MOVE, easing: cubicOut }}
        in:receive={{ key: run.id }}
        out:send={{ key: run.id }}
      >
        <RunCard {run} />
      </article>
    {/each}
  </div>
</section>
