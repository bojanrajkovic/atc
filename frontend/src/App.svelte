<script lang="ts">
  const themes = ['warm', 'radar', 'violet', 'pink'] as const
  let currentTheme = $state<string>('radar') // default theme per .impeccable.md
  let isLight = $state(false) // dark-first

  function setTheme(theme: string) {
    currentTheme = theme
    document.documentElement.setAttribute('data-theme', theme)
  }

  function toggleMode() {
    isLight = !isLight
    if (isLight) {
      document.documentElement.setAttribute('data-mode', 'light')
    } else {
      document.documentElement.removeAttribute('data-mode')
    }
  }
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
          background-color: {currentTheme === theme ? 'var(--accent)' : 'var(--surface-raised)'};
          color: {currentTheme === theme ? 'var(--bg)' : 'var(--text)'};
          border: 1px solid var(--border);
        "
        onclick={() => setTheme(theme)}
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
    onclick={toggleMode}
  >
    {isLight ? 'Dark Mode' : 'Light Mode'}
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
