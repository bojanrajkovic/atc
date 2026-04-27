<script lang="ts">
  export interface Props {
    children?: import('svelte').Snippet
  }

  let { children }: Props = $props()
</script>

<ol class="step-list">
  {@render children?.()}
</ol>

<style>
  .step-list {
    position: relative;
    margin: 0;
    padding: 0 0 0 1.5rem;
    list-style: none;
  }
  .step-list::before {
    content: '';
    position: absolute;
    left: 0.5rem;
    top: 0.5rem;
    bottom: 0.5rem;
    width: 2px;
    background: var(--border);
    pointer-events: none;
  }
</style>
