<script lang="ts">
  import PaletteCommandItem from '../PaletteCommandItem.svelte'
  import * as Command from '../ui/command'

  interface WrapperProps {
    label: string
    icon?: string
    shortcut?: string[]
    onSelect: () => void
  }

  let { label, icon, shortcut, onSelect }: WrapperProps = $props()
</script>

<Command.Root>
  <Command.List>
    {#if icon !== undefined && shortcut !== undefined}
      <PaletteCommandItem {label} {icon} {shortcut} {onSelect} />
    {:else if icon !== undefined}
      <PaletteCommandItem {label} {icon} {onSelect} />
    {:else if shortcut !== undefined}
      <PaletteCommandItem {label} {shortcut} {onSelect} />
    {:else}
      <PaletteCommandItem {label} {onSelect} />
    {/if}
  </Command.List>
</Command.Root>
