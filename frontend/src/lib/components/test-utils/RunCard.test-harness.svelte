<script lang="ts">
  import RovingFocusProvider from '$lib/components/roving/RovingFocusProvider.svelte'
  import RunCardHarnessInner from './RunCardHarnessInner.svelte'
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { RovingFocusContext } from '$lib/components/roving/context'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  interface CardEntry {
    run: WorkflowRun
    jobStats: JobStats
  }

  interface Props {
    cards: ReadonlyArray<CardEntry>
    onCtxReady?: (ctx: RovingFocusContext) => void
  }
  let { cards, onCtxReady = () => {} }: Props = $props()
</script>

<RovingFocusProvider>
  <RunCardHarnessInner {cards} {onCtxReady} />
</RovingFocusProvider>
