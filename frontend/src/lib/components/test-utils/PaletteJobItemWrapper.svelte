<script lang="ts">
  import PaletteJobItem from '../PaletteJobItem.svelte'
  import * as Command from '../ui/command'
  import type { Job } from '$lib/types/generated/Job'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  let { job, parentRun, onSelect }: { job: Job; parentRun: WorkflowRun; onSelect: () => void } =
    $props()
</script>

<Command.Root>
  <Command.List>
    <PaletteJobItem {job} {parentRun} {onSelect} />
  </Command.List>
</Command.Root>
