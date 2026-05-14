<script lang="ts">
  import { SvelteMap } from 'svelte/reactivity'
  import { Separator } from '$lib/components/ui/separator'
  import Logo from './Logo.svelte'
  import RunnerBar from './RunnerBar.svelte'
  import ConnectionIndicator from './ConnectionIndicator.svelte'
  import SettingsPopover from './SettingsPopover.svelte'
  import { connectionStore } from '$lib/stores/connection.svelte'
  import { runnerStore } from '$lib/stores/runners.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import { poolKey } from '$lib/filters/pool'

  type IndicatorState = 'live' | 'stale' | 'connecting' | 'disconnected'

  // Derive connection indicator state from store
  const indicatorState: IndicatorState = $derived.by(() => {
    if (connectionStore.status === 'connected') {
      return connectionStore.isStale ? 'stale' : 'live'
    }
    if (connectionStore.status === 'connecting' || connectionStore.status === 'reconnecting') {
      return 'connecting'
    }
    return 'disconnected'
  })

  // Tick counter that increments every second while stale, forcing
  // indicatorDetail to re-evaluate Date.now() for the elapsed time.
  let staleTick = $state(0)
  let staleTimer: ReturnType<typeof setInterval> | null = null

  $effect(() => {
    if (indicatorState === 'stale') {
      staleTimer = setInterval(() => staleTick++, 1000)
    } else {
      if (staleTimer) clearInterval(staleTimer)
      staleTimer = null
      staleTick = 0
    }
    return () => {
      if (staleTimer) clearInterval(staleTimer)
    }
  })

  const indicatorDetail = $derived.by(() => {
    switch (indicatorState) {
      case 'live':
        return 'Connected'
      case 'stale': {
        // staleTick dependency forces re-evaluation every second
        void staleTick
        const elapsed = connectionStore.lastEventAt
          ? Math.round((Date.now() - connectionStore.lastEventAt) / 1000)
          : 0
        return `No events for ${elapsed}s`
      }
      case 'connecting':
        return connectionStore.reconnectAttempt > 0
          ? `Reconnecting (attempt ${connectionStore.reconnectAttempt})...`
          : 'Connecting...'
      case 'disconnected':
        return 'Disconnected'
    }
  })

  // GitHub uses the literal string "Default" as the runner-group name for
  // every job that isn't placed in a custom runner group. It carries no
  // operator-meaningful information — treat it as if no group were set so the
  // chip label falls back to the runner labels (issue #143).
  function displayGroupName(groupName: string | null): string | null {
    return groupName === 'Default' ? null : groupName
  }

  // Map RunnerPoolStats to RunnerPoolDisplay for RunnerBar
  const pools = $derived.by(() => {
    const allPools = runnerStore.pools

    // Count occurrences of each display group name across the current pool
    // array. Using `displayGroupName` here keeps the count consistent with
    // the label decision below so a single "Default" pool still renders its
    // labels rather than the placeholder name.
    const groupNameCounts = new SvelteMap<string, number>()
    for (const pool of allPools) {
      const name = displayGroupName(pool.groupName)
      if (name !== null) {
        groupNameCounts.set(name, (groupNameCounts.get(name) ?? 0) + 1)
      }
    }

    const activeFilter = uiStore.activePoolFilter

    return allPools.map((pool) => {
      const displayName = displayGroupName(pool.groupName)
      let label: string
      if (displayName === null) {
        label = pool.labels.join(', ')
      } else if ((groupNameCounts.get(displayName) ?? 0) >= 2) {
        label = `${displayName} · ${pool.labels.join(', ')}`
      } else {
        label = displayName
      }

      return {
        key: pool.labels.join(','),
        label,
        running: pool.running,
        queued: pool.queued,
        total: pool.total,
        isElastic: pool.isElastic,
        // Computed here (where original pool.labels is in scope) so RunnerBar
        // stays pure — no uiStore read in the leaf-grid component.
        isActiveFilter: activeFilter !== null && poolKey(pool.labels) === activeFilter,
      }
    })
  })
</script>

<header
  class="flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2 md:py-0 shrink-0"
  style="min-height: 48px; background-color: var(--surface); border-bottom: 1px solid var(--border);"
>
  <!-- Logo: always on row 1 (order-1 at all widths) -->
  <Logo />

  <Separator orientation="vertical" class="h-6 hidden md:block" />

  <!--
    Row-2 container at <md: occupies the full second flex line, laying out
    RunnerBar and SettingsPopover side-by-side.
    At md+: display:contents flattens this wrapper so its children become
    direct flex children of <header> and the md+ order-* classes take effect.
  -->
  <div class="order-3 basis-full flex items-center gap-x-3 md:contents">
    <!-- Runner pools: flex-1 inside the row-2 container at <md; at md+ order-2 flex-1 -->
    <div data-runner-bar class="min-w-0 flex-1 md:order-2 md:flex-1">
      <RunnerBar {pools} />
    </div>

    <Separator orientation="vertical" class="h-6 hidden md:block md:order-3" />

    <!-- Settings: pushed to end of row 2 at <md; at md+ order-5 (last) -->
    <div class="shrink-0 md:order-5">
      <SettingsPopover />
    </div>
  </div>

  <!-- Connection indicator: row 1 at <md (order-2 = next to Logo), row 1 at md+ (order-4) -->
  <div class="order-2 md:order-4">
    <ConnectionIndicator state={indicatorState} detail={indicatorDetail} />
  </div>
</header>
