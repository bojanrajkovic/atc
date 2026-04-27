<script lang="ts">
  import { statusKeyToVar, type StatusKey } from '$lib/format/status-key'

  export interface Props {
    statusKey: StatusKey
    statusLabel: string // e.g., "Failed", "In progress", human-readable
    title: string // run.displayTitle
  }

  let { statusKey, statusLabel, title }: Props = $props()
</script>

<header
  class="panel-header"
  data-status-key={statusKey}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
>
  <span class="eyebrow">
    <span class="dot" aria-hidden="true"></span>
    <span class="label">{statusLabel}</span>
  </span>
  <h2 class="title">{title}</h2>
</header>

<style>
  .panel-header {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 1rem 1.5rem 0.75rem;
  }
  .eyebrow {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    font-size: 0.875rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
  }
  .dot {
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--status-color);
  }
  .label {
    color: var(--status-color);
  }
  .title {
    margin: 0;
    font-size: 1.25rem;
    font-weight: 600;
    color: var(--text);
  }
</style>
