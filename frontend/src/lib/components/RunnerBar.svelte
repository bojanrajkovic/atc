<script lang="ts">
  import RunnerPool from './RunnerPool.svelte'

  interface RunnerPoolDisplay {
    key: string
    label: string
    running: number
    queued: number
    total: number | null
    isElastic: boolean
    /**
     * True when this pool's labels match `uiStore.activePoolFilter`.
     * Computed at the connected layer (TopBar) so RunnerBar stays pure —
     * it does not read uiStore. The matching pool renders with a 2px accent
     * ring via the `is-active-filter` class on RunnerPool's root.
     */
    isActiveFilter: boolean
  }

  let { pools }: { pools: RunnerPoolDisplay[] } = $props()
</script>

<div class="flex items-center gap-4" role="list" aria-label="Runner pools">
  {#each pools as pool (pool.key)}
    <div role="listitem">
      <RunnerPool {pool} isActiveFilter={pool.isActiveFilter} />
    </div>
  {/each}
  {#if pools.length === 0}
    <span class="text-sm" style="color: var(--text-dim);">No active runners</span>
  {/if}
</div>
