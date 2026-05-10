<script lang="ts">
  import StatusIcon from './StatusIcon.svelte'
  import { statusKeyToVar, type StatusKey } from '$lib/format/status-key'

  export interface Props {
    name: string
    statusKey: StatusKey
    durationText: string
  }

  let { name, statusKey, durationText }: Props = $props()
</script>

<li
  class="step-item"
  data-status-key={statusKey}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
>
  <span class="status"><StatusIcon value={statusKey} /></span>
  <span class="name">{name}</span>
  <span class="duration">{durationText}</span>
</li>

<style>
  .step-item {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    padding: 0.25rem 0;
  }
  .status {
    color: var(--status-color);
  }
  .name {
    flex: 1;
  }
  .duration {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
</style>
