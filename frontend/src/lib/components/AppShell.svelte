<script lang="ts">
  import ConfigReloadErrorBanner from './ConfigReloadErrorBanner.svelte'
  import TopBar from './TopBar.svelte'
  import VersionMismatchBanner from './VersionMismatchBanner.svelte'
  import type { Snippet } from 'svelte'

  let { children }: { children?: Snippet } = $props()
</script>

<div class="flex flex-col h-dvh overflow-hidden" style="background-color: var(--bg);">
  <TopBar />
  <ConfigReloadErrorBanner />
  <VersionMismatchBanner />
  <main class="flex-1 overflow-auto">
    {#if children}
      {@render children()}
    {/if}
  </main>
</div>
