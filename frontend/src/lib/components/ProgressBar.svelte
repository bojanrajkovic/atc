<script lang="ts">
  let { completed, total, label }: { completed: number; total: number; label?: string } = $props()

  const ratio = $derived(total > 0 ? Math.min(completed / total, 1) : 0)
  const displayLabel = $derived(label ?? `Jobs ${completed} of ${total}`)
  const ariaValueText = $derived(total === 0 ? 'No jobs' : undefined)
</script>

<div class="run-card-progress">
  <div
    class="progress-track relative h-1 w-full overflow-hidden rounded-full"
    style="background: var(--surface-raised);"
    role="progressbar"
    aria-valuenow={completed}
    aria-valuemin={0}
    aria-valuemax={total}
    aria-valuetext={ariaValueText}
  >
    <div
      class="progress-fill absolute inset-0"
      style="background: var(--status-color); transform: scaleX({ratio}); transform-origin: 0 50%;"
    ></div>
  </div>
  <span class="progress-label text-xs text-muted-foreground tabular-nums">{displayLabel}</span>
</div>
