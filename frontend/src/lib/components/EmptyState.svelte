<script lang="ts">
  export interface EmptyStateProps {
    message?: string
  }

  let { message = 'Watching for runs.' }: EmptyStateProps = $props()

  const COLUMNS = ['Queued', 'Running', 'Completed'] as const
  const PLACEHOLDER_ROWS = [0, 1, 2] as const
</script>

<div class="flex flex-col items-center justify-center h-full gap-8" style="color: var(--text-dim);">
  <!-- Schematic preview: three faint dashed column groups -->
  <div class="flex gap-5 sm:gap-6">
    {#each COLUMNS as col (col)}
      <div
        data-empty-col
        class="flex flex-col gap-2 rounded border border-dashed px-4 py-3 sm:gap-3 sm:px-5 sm:py-4"
        style="border-color: var(--border); opacity: 0.5;"
      >
        <!-- Column label -->
        <span
          class="text-xs sm:text-sm font-mono uppercase tracking-wider"
          style="color: var(--text-dim);"
        >
          {col}
        </span>
        <!-- Placeholder dot rows -->
        {#each PLACEHOLDER_ROWS as row (row)}
          <div
            data-empty-row
            class="font-mono text-sm sm:text-base whitespace-nowrap"
            style="color: var(--text-dim);"
            aria-hidden="true"
          >
            · · · · · · · ·
          </div>
        {/each}
      </div>
    {/each}
  </div>
  <!-- Caption -->
  <p class="text-base sm:text-lg text-center max-w-md px-4">{message}</p>
</div>
