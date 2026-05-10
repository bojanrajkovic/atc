<script lang="ts">
  import MetaGrid from '../MetaGrid.svelte'
  import MetaCell from '../MetaCell.svelte'

  let {
    cells,
  }: {
    cells: Array<{ label: string; value: string | undefined | null }>
  } = $props()
</script>

<MetaGrid>
  {#each cells as cell (cell.label)}
    <MetaCell label={cell.label} value={cell.value} />
  {/each}
</MetaGrid>
