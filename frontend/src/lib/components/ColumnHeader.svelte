<script lang="ts">
  let { label, count, headingId }: { label: string; count: number; headingId: string } = $props()
</script>

<div class="flex items-center gap-2">
  <h2 id={headingId} class="text-xs font-semibold tracking-widest" style="color: var(--text-dim);">
    {label.toUpperCase()}
  </h2>
  <span class="text-xs font-medium" style="color: var(--text-dim);" aria-hidden="true">{count}</span
  >
</div>
