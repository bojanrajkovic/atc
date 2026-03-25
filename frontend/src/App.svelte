<script lang="ts">
  const themes = ['warm', 'radar', 'violet', 'pink'] as const
  let currentTheme = $state<string>('warm')
  let isDark = $state(false)

  function setTheme(theme: string) {
    currentTheme = theme
    document.documentElement.setAttribute('data-theme', theme)
  }

  function toggleMode() {
    isDark = !isDark
    document.documentElement.setAttribute('data-mode', isDark ? 'dark' : 'light')
  }
</script>

<main class="min-h-screen flex flex-col items-center justify-center gap-8 p-8">
  <h1 class="text-4xl font-bold" style="color: var(--color-accent);">
    ATC — Actions Traffic Control
  </h1>

  <p style="color: var(--color-text-secondary);">
    Svelte 5 + Vite + Tailwind v4 + OKLCH Design System
  </p>

  <div class="flex gap-3">
    {#each themes as theme (theme)}
      <button
        class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
        style="
          background-color: {currentTheme === theme
          ? 'var(--color-accent)'
          : 'var(--color-surface-raised)'};
          color: {currentTheme === theme
          ? 'var(--color-surface-base)'
          : 'var(--color-text-primary)'};
          border: 1px solid var(--color-border-default);
        "
        onclick={() => setTheme(theme)}
      >
        {theme}
      </button>
    {/each}
  </div>

  <button
    class="px-4 py-2 rounded-lg text-sm font-medium transition-colors"
    style="
      background-color: var(--color-surface-raised);
      color: var(--color-text-primary);
      border: 1px solid var(--color-border-default);
    "
    onclick={toggleMode}
  >
    {isDark ? 'Light Mode' : 'Dark Mode'}
  </button>

  <div class="flex gap-4 mt-4">
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--color-status-success);"
      title="Success"
    ></div>
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--color-status-warning);"
      title="Warning"
    ></div>
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--color-status-error);"
      title="Error"
    ></div>
    <div
      class="w-16 h-16 rounded-lg"
      style="background-color: var(--color-status-info);"
      title="Info"
    ></div>
  </div>
</main>
