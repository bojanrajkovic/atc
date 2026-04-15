<script lang="ts">
  import * as Popover from '$lib/components/ui/popover'
  import * as ToggleGroup from '$lib/components/ui/toggle-group'
  import { Toggle } from '$lib/components/ui/toggle'
  import { uiStore } from '$lib/stores/ui.svelte'

  const themes = [
    { value: 'warm', hue: 70 },
    { value: 'radar', hue: 155 },
    { value: 'violet', hue: 280 },
    { value: 'pink', hue: 310 },
  ] as const
</script>

<Popover.Root>
  <Popover.Trigger>
    {#snippet child({ props })}
      <button
        {...props}
        class="inline-flex items-center justify-center h-8 w-8 rounded-lg transition-colors"
        style="color: var(--text-dim); background: transparent;"
        aria-label="Settings"
      >
        <!-- Gear icon (inline SVG) -->
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <path
            d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
          />
          <circle cx="12" cy="12" r="3" />
        </svg>
      </button>
    {/snippet}
  </Popover.Trigger>
  <Popover.Content class="w-56" align="end">
    <div class="flex flex-col gap-3">
      <!-- Theme selector -->
      <div>
        <p class="text-xs font-medium mb-2" style="color: var(--text-dim);">Theme</p>
        <ToggleGroup.Root
          type="single"
          value={uiStore.theme}
          onValueChange={(v) => {
            if (v) uiStore.theme = v as typeof uiStore.theme
          }}
          aria-label="Select theme"
        >
          {#each themes as theme (theme.value)}
            <ToggleGroup.Item
              value={theme.value}
              aria-label={theme.value}
              class="h-6 w-6 rounded-full p-0"
            >
              <span
                class="inline-block h-4 w-4 rounded-full"
                style="background-color: oklch(55% 0.15 {theme.hue});"
              ></span>
            </ToggleGroup.Item>
          {/each}
        </ToggleGroup.Root>
      </div>

      <!-- Mode toggle -->
      <div class="flex items-center justify-between">
        <p class="text-xs font-medium" style="color: var(--text-dim);">Light mode</p>
        <Toggle
          pressed={uiStore.mode === 'light'}
          onPressedChange={(pressed) => {
            uiStore.mode = pressed ? 'light' : 'dark'
          }}
          aria-label="Toggle light mode"
          size="sm"
        />
      </div>

      <!-- Density toggle -->
      <div class="flex items-center justify-between">
        <p class="text-xs font-medium" style="color: var(--text-dim);">Compact</p>
        <Toggle
          pressed={uiStore.density === 'compact'}
          onPressedChange={(pressed) => {
            uiStore.density = pressed ? 'compact' : 'comfortable'
          }}
          aria-label="Toggle compact density"
          size="sm"
        />
      </div>
    </div>
  </Popover.Content>
</Popover.Root>
