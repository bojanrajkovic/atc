<script lang="ts">
  import * as Command from '$lib/components/ui/command'
  import { highlightMatches } from '$lib/format/highlight'

  export interface PoolDisplay {
    labels: readonly string[]
    running: number
    queued: number
  }

  export interface Props {
    pool: PoolDisplay
    query: string
    onSelect: () => void
  }

  let { pool, query, onSelect }: Props = $props()

  const labelText = $derived(pool.labels.join(' · '))
  const isQueryActive = $derived(query.trim().length > 0)
  const labelHtml = $derived(isQueryActive ? highlightMatches(labelText, query) : labelText)
</script>

<Command.Item
  value={`pool-${pool.labels.slice().sort().join('|')}`}
  keywords={[...pool.labels]}
  {onSelect}
  data-query-active={isQueryActive ? '' : undefined}
>
  <span class="icon">⊞</span>
  {#if isQueryActive}
    <span class="labels">{@html labelHtml}</span>
  {:else}
    <span class="labels">{labelText}</span>
  {/if}
  <span class="meta">{pool.running} running · {pool.queued} queued</span>
</Command.Item>

<style>
  /* Browse state: single-line truncation */
  .labels {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }

  /* Query-active OR focused: wrap text */
  :global([data-query-active]) .labels,
  :global([data-selected]) .labels {
    white-space: normal;
    overflow: visible;
    text-overflow: clip;
  }

  .meta {
    color: var(--text-quiet);
    margin-left: auto;
    min-width: 18ch;
    text-align: right;
    flex-shrink: 0;
  }

  /* mark element styling for highlights */
  .labels :global(mark) {
    background: var(--mark-bg);
    text-decoration: underline solid var(--mark-underline) 2px;
    text-underline-offset: 2px;
    color: inherit;
    padding: 0;
  }
</style>
