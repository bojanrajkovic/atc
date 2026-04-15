<script lang="ts">
  import ConnectionManager from '$lib/components/ConnectionManager.svelte'
  import AppShell from '$lib/components/AppShell.svelte'
</script>

<ConnectionManager />
<AppShell>
  <!-- Content area: future KanbanBoard goes here -->
  <div class="flex items-center justify-center h-full" style="color: var(--text-dim);">
    <p class="text-sm">Dashboard content coming soon</p>
  </div>
</AppShell>
