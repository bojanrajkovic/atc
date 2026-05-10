<script lang="ts">
  export interface Props {
    children?: import('svelte').Snippet
  }

  let { children }: Props = $props()
</script>

<dl class="meta-grid">
  {@render children?.()}
</dl>

<style>
  .meta-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1rem;
    padding: 1rem 1.5rem;
    margin: 0;
    border-top: 1px solid var(--border);
  }
</style>
