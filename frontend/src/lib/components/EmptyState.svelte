<script lang="ts">
  export interface EmptyStateProps {
    message?: string
  }

  let { message = 'Watching for runs.' }: EmptyStateProps = $props()

  const COLUMNS = ['Queued', 'Running', 'Completed'] as const
  const PLACEHOLDER_ROWS = [0, 1, 2] as const
</script>

<div class="flex flex-col items-center justify-center h-full gap-6" style="color: var(--text-dim);">
  <!-- Schematic preview: three faint dashed column groups -->
  <div class="flex gap-4">
    {#each COLUMNS as col (col)}
      <div
        data-empty-col
        class="flex flex-col gap-2 rounded border px-3 py-2 w-28"
        style="border-color: var(--border); opacity: 0.5;"
      >
        <!-- Column label -->
        <span class="text-xs font-mono uppercase tracking-wider" style="color: var(--text-dim);">
          {col}
        </span>
        <!-- Placeholder dot rows -->
        {#each PLACEHOLDER_ROWS as row (row)}
          <div
            data-empty-row
            class="font-mono text-xs"
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
  <p class="text-sm">{message}</p>
</div>
