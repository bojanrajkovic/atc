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

  // Map RunnerPoolStats to RunnerPoolDisplay for RunnerBar
  const pools = $derived.by(() => {
    const allPools = runnerStore.pools

    // Count occurrences of each non-null groupName across the current pool array.
    const groupNameCounts = new SvelteMap<string, number>()
    for (const pool of allPools) {
      if (pool.groupName !== null) {
        groupNameCounts.set(pool.groupName, (groupNameCounts.get(pool.groupName) ?? 0) + 1)
      }
    }

    const activeFilter = uiStore.activePoolFilter

    return allPools.map((pool) => {
      let label: string
      if (pool.groupName === null) {
        label = pool.labels.join(', ')
      } else if ((groupNameCounts.get(pool.groupName) ?? 0) >= 2) {
        label = `${pool.groupName} · ${pool.labels.join(', ')}`
      } else {
        label = pool.groupName
      }

      return {
        key: pool.labels.join(','),
        label,
        running: pool.running,
        queued: pool.queued,
        total: pool.total,
        isElastic: pool.isElastic,
        // AC5.2: matching pool indicator highlights when active filter is set.
        // Computed here (where original pool.labels is in scope) so RunnerBar
        // stays pure — no uiStore read in the leaf-grid component.
        isActiveFilter: activeFilter !== null && poolKey(pool.labels) === activeFilter,
      }
    })
  })
</script>

<header
  class="flex items-center gap-3 px-4 shrink-0"
  style="height: 48px; background-color: var(--surface); border-bottom: 1px solid var(--border);"
>
  <!-- Left section: Logo -->
  <Logo />

  <Separator orientation="vertical" class="h-6" />

  <!-- Center section: Runner pools -->
  <div class="flex-1 min-w-0">
    <RunnerBar {pools} />
  </div>

  <Separator orientation="vertical" class="h-6" />

  <!-- Right section: Connection status + Settings -->
  <div class="flex items-center gap-3">
    <ConnectionIndicator state={indicatorState} detail={indicatorDetail} />
    <SettingsPopover />
  </div>
</header>
