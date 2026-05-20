<script lang="ts">
  import { onMount } from 'svelte'
  import AriaLiveRegion from '$lib/components/AriaLiveRegion.svelte'
  import AppShell from '$lib/components/AppShell.svelte'
  import CommandPalette from '$lib/components/CommandPalette.svelte'
  import ConnectionManager from '$lib/components/ConnectionManager.svelte'
  import KanbanBoard from '$lib/components/KanbanBoard.svelte'
  import RunDetailPanel from '$lib/components/RunDetailPanel.svelte'
  import RovingFocusProvider from '$lib/components/roving/RovingFocusProvider.svelte'
  import { connectionStore } from '$lib/stores/connection.svelte'
  import { paletteStore } from '$lib/stores/palette.svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import { formatUrlForRunId, parseRunIdFromUrl } from '$lib/url-state'

  // URL ↔ selectedRunId sync (issue #38). See docs/architecture/frontend-app.md
  // § App Shell URL sync for the canonical write-up of the three-piece plumbing
  // and the two-flag loop guard.
  let initialRunId: bigint | null = parseRunIdFromUrl(window.location.href)
  let initialUrlPending = $state(true)

  // Outbound: selectedRunId → URL. Suppressed via initialUrlPending until the
  // hydration effect has consumed the buffered initialRunId — without the
  // guard, the first run with selectedRunId === null would strip ?run= before
  // hydration ever fires. The semantic run-id comparison kills popstate-
  // induced echoes (and tolerates non-canonical encoding of other query
  // params; a string-equality guard would treat `?q=my%20term` and
  // `?q=my+term` as different and add a spurious entry on hydration).
  $effect(() => {
    if (initialUrlPending) return
    const currentRunId = parseRunIdFromUrl(window.location.href)
    if (currentRunId === uiStore.selectedRunId) return
    history.pushState(null, '', formatUrlForRunId(uiStore.selectedRunId, window.location.href))
  })

  // Hydration: on the first connectionStore.status === 'connected' (the
  // moment runStore.runs is guaranteed to reflect the server snapshot),
  // apply initialRunId if the run exists, else replaceState-strip the param.
  // initialRunId is one-shot — nulled after consumption so reconnects don't
  // re-trigger.
  $effect(() => {
    if (connectionStore.status !== 'connected') return
    if (initialRunId !== null) {
      if (runStore.runs.has(initialRunId)) {
        uiStore.selectedRunId = initialRunId
      } else {
        history.replaceState(null, '', formatUrlForRunId(null, window.location.href))
      }
      initialRunId = null
    }
    initialUrlPending = false
  })

  onMount(() => {
    function handleKeydown(e: KeyboardEvent) {
      if (!(e.metaKey || e.ctrlKey)) return

      // Allow shortcuts from inside the palette input (data-slot="command-input").
      // Block from other editable contexts so future text inputs don't fire chord
      // shortcuts that conflict with normal typing.
      const target = e.target as HTMLElement | null
      const inPaletteInput = target?.closest('[data-slot="command-input"]') !== null
      if (!inPaletteInput && target?.matches?.('input, textarea, [contenteditable="true"]')) {
        return
      }

      // Cmd+D toggles dark mode. preventDefault is required because Cmd+D is the
      // browser's "bookmark this page" shortcut and would otherwise win even when
      // the palette is open. close() routes through the store so subMenu resets
      // alongside paletteOpen — a direct write to paletteOpen would leave the
      // theme submenu sticky and reopen the palette into stale state.
      if (e.key === 'd') {
        e.preventDefault()
        uiStore.mode = uiStore.mode === 'dark' ? 'light' : 'dark'
        if (paletteStore.paletteOpen) paletteStore.close()
        return
      }

      // Cmd+\ toggles compact density.
      if (e.key === '\\') {
        e.preventDefault()
        uiStore.density = uiStore.density === 'comfortable' ? 'compact' : 'comfortable'
        if (paletteStore.paletteOpen) paletteStore.close()
        return
      }

      // Cmd+K toggles the palette.
      if (e.key === 'k') {
        e.preventDefault()
        paletteStore.toggle()
        return
      }
    }
    window.addEventListener('keydown', handleKeydown)

    // Inbound: popstate → selectedRunId. Stale ids (run no longer in the
    // store) are scrubbed via replaceState (no extra history entry), and the
    // panel is closed so URL and panel state stay in sync — otherwise the
    // user could be on URL `/` while still seeing run B, and a refresh or
    // copied link would silently lose that selection. The semantic loop
    // guard in the outbound effect ensures the synchronous follow-up assign
    // is a no-op.
    //
    // lastTriggerRunId is updated alongside selectedRunId so RunDetailPanel's
    // onCloseAutoFocus restores focus to a card matching the *displayed* run,
    // not whichever card the user most recently clicked before back/forward.
    function handlePopstate() {
      const parsed = parseRunIdFromUrl(window.location.href)
      if (parsed === uiStore.selectedRunId) return
      if (parsed === null) {
        uiStore.lastTriggerRunId = null
        uiStore.selectedRunId = null
        return
      }
      if (runStore.runs.has(parsed)) {
        uiStore.lastTriggerRunId = parsed
        uiStore.selectedRunId = parsed
        return
      }
      history.replaceState(null, '', formatUrlForRunId(null, window.location.href))
      uiStore.lastTriggerRunId = null
      uiStore.selectedRunId = null
    }
    window.addEventListener('popstate', handlePopstate)

    return () => {
      window.removeEventListener('keydown', handleKeydown)
      window.removeEventListener('popstate', handlePopstate)
    }
  })
</script>

<ConnectionManager />
<AriaLiveRegion />
<RovingFocusProvider>
  <AppShell>
    <KanbanBoard />
  </AppShell>
  <CommandPalette />
  <RunDetailPanel />
</RovingFocusProvider>
