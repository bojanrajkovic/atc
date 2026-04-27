<script lang="ts">
  import PaletteRunItem from '../PaletteRunItem.svelte'
  import * as Command from '../ui/command'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  let { run, onSelect }: { run: WorkflowRun; onSelect: () => void } = $props()
</script>

<Command.Root>
  <Command.List>
    <PaletteRunItem {run} {onSelect} />
  </Command.List>
</Command.Root>
