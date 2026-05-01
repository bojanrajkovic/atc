<script lang="ts">
  import * as Sheet from '../ui/sheet'
  import * as Command from '../ui/command'

  let panelOpen = $state(true)
  let paletteOpen = $state(true)
</script>

<Sheet.Root bind:open={panelOpen}>
  <Sheet.Content side="right">panel content</Sheet.Content>
</Sheet.Root>
<Command.Dialog bind:open={paletteOpen}>
  <Command.Input placeholder="search" />
  <Command.List>
    <Command.Item value="a">a</Command.Item>
  </Command.List>
</Command.Dialog>
