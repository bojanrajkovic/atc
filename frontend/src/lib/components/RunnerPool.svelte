<script lang="ts">
  import CapacityBar from './CapacityBar.svelte'
  import { Badge } from '$lib/components/ui/badge'

  interface RunnerPoolDisplay {
    label: string
    running: number
    queued: number
    total: number | null
    isElastic: boolean
  }

  export interface Props {
    pool: RunnerPoolDisplay
    isActiveFilter?: boolean
  }

  let { pool, isActiveFilter = false }: Props = $props()

  const utilization = $derived(
    pool.total !== null && pool.total > 0 ? pool.running / pool.total : null
  )

  const dotColor = $derived(
    utilization !== null
      ? utilization >= 1.0
        ? 'var(--failed)'
        : utilization >= 0.7
          ? 'var(--running)'
          : 'var(--success)'
      : pool.running > 0
        ? 'var(--success)'
        : 'var(--text-dim)'
  )
</script>

<div
  class="runner-pool flex items-center gap-2 text-sm"
  class:is-active-filter={isActiveFilter}
  role="group"
  aria-label="{pool.label} runner pool"
  data-testid="runner-pool-{pool.label}"
>
  <!-- Status dot -->
  <span
    class="inline-block h-2 w-2 shrink-0 rounded-full"
    style="background-color: {dotColor};"
    aria-hidden="true"
  ></span>

  <!-- Pool label -->
  <span class="truncate max-w-24" style="color: var(--text-dim);">{pool.label}</span>

  <!-- Capacity bar (known-capacity pools only) -->
  {#if pool.total !== null}
    <div class="w-16">
      <CapacityBar used={pool.running} total={pool.total} />
    </div>
  {/if}

  <!-- Count -->
  <span class="tabular-nums font-mono text-xs" style="color: var(--text);">
    {#if pool.total !== null}
      {pool.running}/{pool.total}
    {:else}
      {pool.running}
    {/if}
  </span>

  <!-- Queued badge -->
  {#if pool.queued > 0}
    <Badge variant="secondary">
      +{pool.queued} queued
    </Badge>
  {/if}
</div>

<style>
  .runner-pool.is-active-filter {
    box-shadow: 0 0 0 2px var(--accent);
    background: var(--surface-raised);
    border-radius: 4px;
    padding-inline: 4px;
  }
</style>
