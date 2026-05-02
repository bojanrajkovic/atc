<script lang="ts">
  import { getRovingContext } from '$lib/components/roving/context'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { RovingFocusContext } from '$lib/components/roving/context'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import RunCard from '../RunCard.svelte'

  interface CardEntry {
    run: WorkflowRun
    jobStats: JobStats
  }

  interface Props {
    cards: ReadonlyArray<CardEntry>
    onCtxReady: (ctx: RovingFocusContext) => void
  }
  let { cards, onCtxReady }: Props = $props()

  // Called during init, inside the provider's component tree, so getRovingContext succeeds.
  const ctx = getRovingContext()

  $effect(() => {
    onCtxReady(ctx)
  })
</script>

{#each cards as { run, jobStats } (run.id)}
  <RunCard {run} {jobStats} />
{/each}
