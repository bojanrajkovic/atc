<script lang="ts">
  import RovingFocusProvider from '../RovingFocusProvider.svelte'
  import RovingHarnessGrid from './RovingHarnessGrid.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import type { RovingFocusContext } from '../context'

  interface Props {
    runs: readonly WorkflowRun[]
    onCtxReady: (ctx: RovingFocusContext) => void
  }
  let { runs, onCtxReady }: Props = $props()
</script>

<RovingFocusProvider>
  <RovingHarnessGrid {runs} {onCtxReady} />
</RovingFocusProvider>
