<script lang="ts">
  import { onMount } from 'svelte'
  import AriaLiveRegion from '$lib/components/AriaLiveRegion.svelte'
  import AppShell from '$lib/components/AppShell.svelte'
  import CommandPalette from '$lib/components/CommandPalette.svelte'
  import ConnectionManager from '$lib/components/ConnectionManager.svelte'
  import KanbanBoard from '$lib/components/KanbanBoard.svelte'
  import RunDetailPanel from '$lib/components/RunDetailPanel.svelte'
  import RovingFocusProvider from '$lib/components/roving/RovingFocusProvider.svelte'
  import { paletteStore } from '$lib/stores/palette.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'

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
    return () => window.removeEventListener('keydown', handleKeydown)
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
