<script lang="ts">
  import { onMount } from 'svelte'
  import ConnectionManager from '$lib/components/ConnectionManager.svelte'
  import AppShell from '$lib/components/AppShell.svelte'
  import KanbanBoard from '$lib/components/KanbanBoard.svelte'
  import CommandPalette from '$lib/components/CommandPalette.svelte'
  import RunDetailPanel from '$lib/components/RunDetailPanel.svelte'
  import { paletteStore } from '$lib/stores/palette.svelte'

  onMount(() => {
    function handleKeydown(e: KeyboardEvent) {
      if (!((e.metaKey || e.ctrlKey) && e.key === 'k')) return
      const target = e.target as HTMLElement | null
      // Allow Cmd+K from inside the palette (which has data-slot="command-input") to toggle/close it.
      // Block Cmd+K from other editable contexts so future inputs don't double-toggle.
      if (target && target.closest('[data-slot="command-input"]') === null) {
        if (target.matches?.('input, textarea, [contenteditable="true"]')) return
      }
      e.preventDefault()
      paletteStore.toggle()
    }
    window.addEventListener('keydown', handleKeydown)
    return () => window.removeEventListener('keydown', handleKeydown)
  })
</script>

<ConnectionManager />
<AppShell>
  <KanbanBoard />
</AppShell>
<CommandPalette />
<RunDetailPanel />
