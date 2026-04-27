<script lang="ts">
  import ConnectionManager from '$lib/components/ConnectionManager.svelte'
  import AppShell from '$lib/components/AppShell.svelte'
  import KanbanBoard from '$lib/components/KanbanBoard.svelte'
  import CommandPalette from '$lib/components/CommandPalette.svelte'
</script>

<ConnectionManager />
<AppShell>
  <KanbanBoard />
</AppShell>
<CommandPalette />
