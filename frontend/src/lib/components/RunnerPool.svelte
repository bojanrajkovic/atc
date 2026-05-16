<script lang="ts">
  import CapacityBar from './CapacityBar.svelte'
  import { Badge } from '$lib/components/ui/badge'
  import type { RunnerPoolTotal } from '$lib/types/generated/RunnerPoolTotal'

  interface RunnerPoolDisplay {
    label: string
    running: number
    queued: number
    total: RunnerPoolTotal
  }

  export interface Props {
    pool: RunnerPoolDisplay
    isActiveFilter?: boolean
  }

  let { pool, isActiveFilter = false }: Props = $props()

  // Exhaustive runtime narrowing keeps the bar/no-bar decision and the dot
  // color in one place. Each branch returns the data the template needs.
  // The `default` arm is the typed-union sharp edge documented in
  // `frontend/CLAUDE.md`: an off-shape value from a boundary would otherwise
  // silently render `undefined` and cascade into a broken downstream view.
  function describeTotal(total: RunnerPoolTotal): {
    showBar: boolean
    barTotal: number | null
    countText: string
    unboundedAffordance: boolean
    utilization: number | null
  } {
    switch (total.kind) {
      case 'Bounded': {
        const utilization = total.value > 0 ? pool.running / total.value : null
        return {
          showBar: true,
          barTotal: total.value,
          countText: `${pool.running}/${total.value}`,
          unboundedAffordance: false,
          utilization,
        }
      }
      case 'Unbounded':
        return {
          showBar: false,
          barTotal: null,
          countText: `${pool.running}`,
          unboundedAffordance: true,
          utilization: null,
        }
      case 'Undeclared':
        return {
          showBar: false,
          barTotal: null,
          countText: `${pool.running}`,
          unboundedAffordance: false,
          utilization: null,
        }
      default: {
        const _exhaustive: never = total
        throw new Error(`unhandled RunnerPoolTotal: ${JSON.stringify(_exhaustive)}`)
      }
    }
  }

  const view = $derived(describeTotal(pool.total))

  const dotColor = $derived(
    view.utilization !== null
      ? view.utilization >= 1.0
        ? 'var(--failed)'
        : view.utilization >= 0.7
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

  <!-- Pool label: truncates to 12ch at <md, unconstrained at md+ -->
  <span class="truncate max-w-[12ch] md:max-w-none" style="color: var(--text-dim);"
    >{pool.label}</span
  >

  <!-- Capacity bar (Bounded variant only) -->
  {#if view.showBar && view.barTotal !== null}
    <div class="w-16">
      <CapacityBar used={pool.running} total={view.barTotal} />
    </div>
  {/if}

  <!-- Count -->
  <span class="tabular-nums font-mono text-xs" style="color: var(--text);">
    {view.countText}
  </span>

  <!-- Unbounded affordance (operator-declared unbounded pools only). The
       visible glyph carries an accessible label so screen readers don't
       read it as a bare math symbol — WCAG SC 1.4.1 forbids relying on
       visual treatment alone to distinguish Unbounded from Undeclared. -->
  {#if view.unboundedAffordance}
    <span
      class="text-xs font-medium tracking-tight"
      style="color: var(--text-dim);"
      aria-label="unbounded capacity"
      title="Unbounded — no declared ceiling"
      data-testid="unbounded-affordance"
    >
      ∞
    </span>
  {/if}

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
