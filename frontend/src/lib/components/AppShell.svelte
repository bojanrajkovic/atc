<script lang="ts">
  import TopBar from './TopBar.svelte'
  import type { Snippet } from 'svelte'

  let { children }: { children?: Snippet } = $props()
</script>

<div class="flex flex-col h-dvh overflow-hidden" style="background-color: var(--bg);">
  <TopBar />
  <main class="flex-1 overflow-auto">
    {#if children}
      {@render children()}
    {/if}
  </main>
</div>
