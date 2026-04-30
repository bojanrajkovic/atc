<script lang="ts">
  import { tick } from 'svelte'
  import { slide } from 'svelte/transition'
  import * as Command from '$lib/components/ui/command'
  import PaletteSection from './PaletteSection.svelte'
  import PaletteRunItem from './PaletteRunItem.svelte'
  import PaletteJobItem from './PaletteJobItem.svelte'
  import PalettePoolItem from './PalettePoolItem.svelte'
  import PaletteCommandItem from './PaletteCommandItem.svelte'
  import { paletteStore } from '$lib/stores/palette.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { runnerStore } from '$lib/stores/runners.svelte'
  import { connectionStore } from '$lib/stores/connection.svelte'
  import { poolKey } from '$lib/filters/pool'
  import commandScore from 'command-score'

  let inputEl: HTMLInputElement | null = $state(null)

  // Re-focus the input on open — Bits UI Dialog auto-focuses the first focusable element
  // (usually the close button); suppress with onOpenAutoFocus, then manually focus the input.
  $effect(() => {
    if (paletteStore.paletteOpen && inputEl) {
      tick().then(() => inputEl?.focus())
    }
  })

  // Prune evicted run ids from the LRU. The derivation below already filters
  // out missing runs so users never see stale entries, but the storage backing
  // (sessionStorage-persisted recentRunIds) would otherwise hold them
  // indefinitely until displaced by 10 fresh visits. This effect runs whenever
  // runStore.runs changes and writes back a filtered list when eviction has
  // occurred — guarded by a length check so it doesn't loop on the write.
  $effect(() => {
    const ids = paletteStore.recentRunIds
    const present = ids.filter((id) => runStore.runs.has(id))
    if (present.length !== ids.length) {
      paletteStore.recentRunIds = present
    }
  })

  // SCORE_THRESHOLD: command-score returns 0 for no match; any positive score is a match.
  // Tune upward later if zero-relevance items surface too often.
  const SCORE_THRESHOLD = 0.0

  // All runs in source order: queued + inProgress + completed (Recent section takes priority above).
  // With shouldFilter={false}, we filter manually via command-score.
  const recentRuns = $derived.by(() => {
    const ids = paletteStore.recentRunIds
    const runs = ids
      .map((id) => runStore.runs.get(id))
      .filter((r): r is import('$lib/types/generated/WorkflowRun').WorkflowRun => r !== undefined)
    if (paletteStore.paletteQuery === '') return runs
    return runs.filter(
      (r) => commandScore(r.displayTitle, paletteStore.paletteQuery) > SCORE_THRESHOLD
    )
  })

  const allRuns = $derived.by(() => {
    const runs = [...runStore.queuedRuns, ...runStore.inProgressRuns, ...runStore.completedRuns]
    if (paletteStore.paletteQuery === '') return runs
    return runs.filter(
      (r) => commandScore(r.displayTitle, paletteStore.paletteQuery) > SCORE_THRESHOLD
    )
  })

  // Flatten jobs across all known runs for the Jobs section
  const allJobs = $derived.by(() => {
    const entries = [...runStore.jobsByRunId.entries()]
      .flatMap(([runId, jobs]) => jobs.map((job) => ({ job, parentRun: runStore.runs.get(runId) })))
      .filter(
        (
          entry
        ): entry is {
          job: import('$lib/types/generated/Job').Job
          parentRun: import('$lib/types/generated/WorkflowRun').WorkflowRun
        } => entry.parentRun !== undefined
      )
    if (paletteStore.paletteQuery === '') return entries
    return entries.filter(
      ({ job, parentRun }) =>
        commandScore(job.name, paletteStore.paletteQuery) > SCORE_THRESHOLD ||
        commandScore(parentRun.displayTitle, paletteStore.paletteQuery) > SCORE_THRESHOLD
    )
  })

  const filteredPools = $derived.by(() => {
    const pools = runnerStore.pools
    if (paletteStore.paletteQuery === '') return pools
    return pools.filter(
      (p) => commandScore(p.labels.join(' '), paletteStore.paletteQuery) > SCORE_THRESHOLD
    )
  })

  async function selectRun(runId: bigint) {
    uiStore.selectedRunId = runId
    await tick() // let Sheet mount
    paletteStore.paletteOpen = false
    paletteStore.recordRunVisit(runId)
  }

  async function selectJob(job: import('$lib/types/generated/Job').Job) {
    uiStore.selectedRunId = job.runId
    uiStore.selectedJobId = job.id
    await tick() // let Sheet mount and JobBlock $effect see selectedJobId
    paletteStore.paletteOpen = false
  }

  function selectPool(pool: (typeof runnerStore.pools)[number]) {
    uiStore.activePoolFilter = poolKey(pool.labels)
    paletteStore.paletteOpen = false
  }

  // Commands list — conditional rendering for AC1.12 + AC1.13
  function toggleDarkMode() {
    uiStore.mode = uiStore.mode === 'dark' ? 'light' : 'dark'
    paletteStore.paletteOpen = false
  }
  function toggleDensity() {
    uiStore.density = uiStore.density === 'comfortable' ? 'compact' : 'comfortable'
    paletteStore.paletteOpen = false
  }
  function clearPoolFilter() {
    uiStore.activePoolFilter = null
    paletteStore.paletteOpen = false
  }
  function closeDetailPanel() {
    uiStore.selectedRunId = null
    paletteStore.paletteOpen = false
  }
  function reconnect() {
    connectionStore.requestReconnect()
    paletteStore.paletteOpen = false
  }
  function enterThemeSubmenu() {
    paletteStore.enterSubmenu('theme')
  }

  function selectTheme(theme: 'warm' | 'radar' | 'violet' | 'pink') {
    uiStore.theme = theme
    paletteStore.exitSubmenu()
    paletteStore.paletteOpen = false
  }
</script>

<Command.Dialog
  open={paletteStore.paletteOpen}
  onOpenChange={(open) => (open ? paletteStore.open() : paletteStore.close())}
  shouldFilter={false}
  onOpenAutoFocus={(e) => e.preventDefault()}
  onCloseAutoFocus={(event) => {
    // When the palette closes and the panel is open underneath, return focus to
    // the panel's close button. Otherwise, the default behavior (restore to body
    // or the previously-focused element) is fine.
    if (uiStore.selectedRunId !== null) {
      event.preventDefault()
      // Look up the panel's close button by aria-label (set in PanelActions.svelte).
      // As long as the panel is open, exactly one element in the DOM has this
      // aria-label — query-selector is a pragmatic alternative to threading refs
      // across component boundaries via stores.
      const closeButton = document.querySelector<HTMLElement>(
        'button[aria-label="Close detail panel"]'
      )
      closeButton?.focus()
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape' && paletteStore.subMenu !== null) {
      e.preventDefault()
      e.stopPropagation()
      paletteStore.exitSubmenu()
    }
  }}
>
  <!--
    shouldFilter={false} disables Bits UI's auto-filter+auto-sort entirely.
    Each section renders only its pre-filtered $derived array, preserving fixed
    section order (Recent → Runs → Jobs → Pools → Commands) and source order
    within each section regardless of match scores.

    onOpenAutoFocus and the focus $effect above are forwarded by Phase 1's patch
    to command-dialog.svelte. Without that patch they would be silently dropped.
  -->
  <Command.Input
    bind:ref={inputEl}
    value={paletteStore.paletteQuery}
    oninput={(e) => {
      const target = e.target as HTMLInputElement
      paletteStore.setQuery(target.value)
    }}
    placeholder="Search runs, jobs, pools, commands…"
  />
  <Command.List>
    <!--
      Manual empty-state: with shouldFilter={false}, <Command.Empty> does not
      auto-fire when nothing matches. Gate it manually: show only when a query is
      active AND all non-command sections are empty (Commands are always present
      and excluded from the empty-state check per AC1.10).
    -->
    {#if paletteStore.paletteQuery !== '' && recentRuns.length === 0 && allRuns.length === 0 && allJobs.length === 0 && filteredPools.length === 0}
      <Command.Empty forceMount>
        Nothing in flight matching “{paletteStore.paletteQuery}”.
      </Command.Empty>
    {/if}

    {#if paletteStore.subMenu === 'theme'}
      <div transition:slide|local={{ duration: 200 }}>
        <PaletteSection heading="Switch theme">
          <PaletteCommandItem label="Warm" onSelect={() => selectTheme('warm')} />
          <PaletteCommandItem label="Radar" onSelect={() => selectTheme('radar')} />
          <PaletteCommandItem label="Violet" onSelect={() => selectTheme('violet')} />
          <PaletteCommandItem label="Pink" onSelect={() => selectTheme('pink')} />
        </PaletteSection>
      </div>
    {:else}
      {#if recentRuns.length > 0}
        <PaletteSection heading="Recent">
          {#each recentRuns as run (run.id)}
            <PaletteRunItem {run} valuePrefix="recent-run" onSelect={() => selectRun(run.id)} />
          {/each}
        </PaletteSection>
      {/if}

      {#if allRuns.length > 0}
        <PaletteSection heading="Runs">
          {#each allRuns as run (run.id)}
            <PaletteRunItem {run} onSelect={() => selectRun(run.id)} />
          {/each}
        </PaletteSection>
      {/if}

      {#if allJobs.length > 0}
        <PaletteSection heading="Jobs">
          {#each allJobs as { job, parentRun } (job.id)}
            <PaletteJobItem {job} {parentRun} onSelect={() => selectJob(job)} />
          {/each}
        </PaletteSection>
      {/if}

      {#if filteredPools.length > 0}
        <PaletteSection heading="Runner Pools">
          {#each filteredPools as pool (pool.labels.slice().sort().join('|'))}
            <PalettePoolItem
              {pool}
              query={paletteStore.paletteQuery}
              onSelect={() => selectPool(pool)}
            />
          {/each}
        </PaletteSection>
      {/if}

      <!--
        Commands section: always rendered (Commands are not filtered by query — they
        are utility actions, not data rows). Conditional items use {#if} blocks so
        they are literally absent from the DOM when inactive (with shouldFilter={false},
        Bits UI will not hide them by visibility — they must not be in the DOM at all).
      -->
      <PaletteSection heading="Commands">
        <PaletteCommandItem label="Switch theme…" onSelect={enterThemeSubmenu} />
        <PaletteCommandItem
          label="Toggle dark mode"
          shortcut={['⌘', 'D']}
          onSelect={toggleDarkMode}
        />
        <PaletteCommandItem
          label="Toggle compact density"
          shortcut={['⌘', '\\']}
          onSelect={toggleDensity}
        />
        {#if uiStore.activePoolFilter !== null}
          <PaletteCommandItem label="Clear pool filter" onSelect={clearPoolFilter} />
        {/if}
        {#if uiStore.selectedRunId !== null}
          <PaletteCommandItem
            label="Close detail panel"
            shortcut={['Esc']}
            onSelect={closeDetailPanel}
          />
        {/if}
        <PaletteCommandItem label="Reconnect" onSelect={reconnect} />
      </PaletteSection>
    {/if}
  </Command.List>
</Command.Dialog>
