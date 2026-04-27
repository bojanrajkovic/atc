<script lang="ts">
  import PaletteSection from '../PaletteSection.svelte'
  import * as Command from '../ui/command'

  let { heading, content }: { heading: string; content: string } = $props()
</script>

<Command.Root>
  <Command.List>
    <PaletteSection {heading}>
      {#if content}
        <div>{content}</div>
      {/if}
    </PaletteSection>
  </Command.List>
</Command.Root>
