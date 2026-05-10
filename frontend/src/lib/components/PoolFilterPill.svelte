<script lang="ts">
  export interface Props {
    /** Sorted, dot-separated labels for display, e.g., "linux · self-hosted · x86" */
    labelText: string
    onClear: () => void
  }

  let { labelText, onClear }: Props = $props()
</script>

<div class="pool-filter-pill">
  <span class="prefix">Filtering by</span>
  <span class="labels">{labelText}</span>
  <button type="button" class="clear" onclick={onClear} aria-label="Clear pool filter"> ✕ </button>
</div>

<style>
  .pool-filter-pill {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.375rem 0.75rem;
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 9999px;
    font-size: 0.875rem;
  }
  .prefix {
    color: var(--text-dim);
  }
  .labels {
    color: var(--text);
    font-family: monospace;
  }
  .clear {
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 0.125rem 0.375rem;
    color: var(--text-dim);
    border-radius: 9999px;
  }
  .clear:hover {
    color: var(--text);
    background: var(--border);
  }
  .clear:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
