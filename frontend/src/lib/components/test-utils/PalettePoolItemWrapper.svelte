<script lang="ts">
  import PalettePoolItem from '../PalettePoolItem.svelte'
  import * as Command from '../ui/command'
  import type { PoolDisplay } from '../PalettePoolItem.svelte'

  let { pool, query, onSelect }: { pool: PoolDisplay; query: string; onSelect: () => void } = $props()
</script>

<Command.Root>
  <Command.List>
    <PalettePoolItem {pool} {query} {onSelect} />
  </Command.List>
</Command.Root>
