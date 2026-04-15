<script lang="ts">
  import { uiStore } from '$lib/stores/ui.svelte'

  const themes = ['warm', 'radar', 'violet', 'pink'] as const
</script>

<main class="min-h-screen flex flex-col items-center justify-center gap-8 p-8">
  <h1 class="text-4xl font-bold" style="color: var(--accent);">ATC — Actions Traffic Control</h1>

  <p style="color: var(--text-dim);">Svelte 5 + Vite + Tailwind v4 + OKLCH Design System</p>

  <div class="flex gap-3">
    {#each themes as theme (theme)}
      <button
        type="button"
        class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
        style="
          background-color: {uiStore.theme === theme ? 'var(--accent)' : 'var(--surface-raised)'};
          color: {uiStore.theme === theme ? 'var(--bg)' : 'var(--text)'};
          border: 1px solid var(--border);
        "
        onclick={() => uiStore.theme = theme}
      >
        {theme}
      </button>
    {/each}
  </div>

  <button
    type="button"
    class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
    style="
      background-color: var(--surface-raised);
      color: var(--text);
      border: 1px solid var(--border);
    "
    onclick={() => uiStore.mode = uiStore.mode === 'dark' ? 'light' : 'dark'}
  >
    {uiStore.mode === 'dark' ? 'Light Mode' : 'Dark Mode'}
  </button>

  <div class="flex gap-4 mt-4">
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--success);"
      title="Success"
    ></div>
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--running);"
      title="Running"
    ></div>
    <div class="w-16 h-16 rounded-lg" style="background-color: var(--failed);" title="Failed"></div>
    <div class="w-16 h-16 rounded-lg" style="background-color: var(--queued);" title="Queued"></div>
  </div>
</main>
