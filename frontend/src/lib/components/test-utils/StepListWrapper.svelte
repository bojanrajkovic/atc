<script lang="ts">
  import StepList from '../StepList.svelte'
  import StepItem from '../StepItem.svelte'
  import type { StatusKey } from '$lib/format/status-key'

  let {
    steps,
  }: {
    steps: Array<{ name: string; statusKey: StatusKey; durationText: string }>
  } = $props()
</script>

<StepList>
  {#each steps as step (step.name)}
    <StepItem name={step.name} statusKey={step.statusKey} durationText={step.durationText} />
  {/each}
</StepList>
