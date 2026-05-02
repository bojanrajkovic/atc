<script lang="ts">
  import { roving } from './action'
  import { getRovingContext } from './context'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import type { RovingFocusContext } from './context'

  interface Props {
    runs: readonly WorkflowRun[]
    onCtxReady: (ctx: RovingFocusContext) => void
  }
  let { runs, onCtxReady }: Props = $props()

  // Component-scoped — getRovingContext is valid here because this component
  // is mounted as a child of <RovingFocusProvider> which has already called
  // setRovingContext during its own init.
  const ctx = getRovingContext()

  // Report the live ctx reference to the test once at mount.
  $effect(() => {
    onCtxReady(ctx)
  })
</script>

<div use:roving={ctx} data-testid="grid">
  {#each runs as run (run.id)}
    <article class="run-card" data-run-id={run.id}>
      <button
        class="run-card-activate"
        type="button"
        tabindex={ctx.currentFocusRunId === run.id ? 0 : -1}
      >
        {run.displayTitle ?? `Run ${run.id}`}
      </button>
    </article>
  {/each}
</div>
