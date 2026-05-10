<script lang="ts">
  let { summary }: { summary: string | null } = $props()
</script>

{#if summary !== null}
  <div class="run-card-runner truncate font-mono text-xs">
    <span aria-hidden="true">⊞</span>
    {summary}
  </div>
{/if}
