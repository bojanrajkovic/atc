<script lang="ts">
  let { label, count, headingId }: { label: string; count: number; headingId: string } = $props()
</script>

<div
  data-column-header
  class="sticky top-0 z-10 flex items-center gap-2 px-2 py-2 sm:px-0 sm:py-0"
  style="background-color: var(--bg);"
>
  <h2 id={headingId} class="text-xs font-semibold tracking-widest" style="color: var(--text-dim);">
    {label.toUpperCase()}
  </h2>
  <span class="text-xs font-medium" style="color: var(--text-dim);">{count}</span>
</div>
