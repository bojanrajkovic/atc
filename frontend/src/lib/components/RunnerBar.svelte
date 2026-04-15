<script lang="ts">
  import RunnerPool from './RunnerPool.svelte'

  interface RunnerPoolDisplay {
    label: string
    running: number
    queued: number
    total: number | null
    isElastic: boolean
  }

  let { pools }: { pools: RunnerPoolDisplay[] } = $props()
</script>

<div class="flex items-center gap-4" role="list" aria-label="Runner pools">
  {#each pools as pool (pool.label)}
    <div role="listitem">
      <RunnerPool {pool} />
    </div>
  {/each}
  {#if pools.length === 0}
    <span class="text-sm" style="color: var(--text-dim);">No pools</span>
  {/if}
</div>
