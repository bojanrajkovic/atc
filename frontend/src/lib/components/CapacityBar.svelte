<script lang="ts">
  let { used, total }: { used: number; total: number } = $props()

  const pct = $derived(total > 0 ? Math.min(Math.round((used / total) * 100), 100) : 0)

  const color = $derived(
    pct >= 100 ? 'var(--failed)' : pct >= 70 ? 'var(--running)' : 'var(--success)'
  )
</script>

<div
  class="h-2 w-full rounded-full overflow-hidden"
  style="background-color: var(--surface-raised);"
  role="meter"
  aria-valuenow={used}
  aria-valuemin={0}
  aria-valuemax={total}
  aria-label="Pool capacity"
>
  <div
    class="h-full rounded-full transition-all duration-300"
    style="width: {pct}%; background-color: {color};"
  ></div>
</div>
