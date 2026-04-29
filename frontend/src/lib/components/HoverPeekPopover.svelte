<script lang="ts">
  import * as Popover from '$lib/components/ui/popover'
  import StatusIcon from './StatusIcon.svelte'
  import { resolveStatusKey, statusKeyToVar } from '$lib/format/status-key'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

  /**
   * Refined Props: parent (RunCard, Task 3) is responsible for all aggregation.
   * The popover stays pure — it only renders what it's given.
   */
  export interface Props {
    run: WorkflowRun
    /** Human-readable status label resolved by the parent. */
    statusLabel: string
    /** Total number of jobs for this run. */
    totalJobs: number
    /** Number of steps completed across all jobs (steps-level, not jobs-level). */
    stepsCompleted: number
    /** Total number of steps across all jobs. */
    stepsTotal: number
    /** Pre-formatted duration string (e.g. "1:23") resolved by the parent. */
    durationText: string
    /** Runner summary string, or null when no runner info is available. */
    runnerSummary: string | null
    /** The element to anchor the popover to (the run card's article element). */
    anchor: HTMLElement | null
    /** Two-way bindable open state. Parent controls via hover timer. */
    open: boolean
  }

  let {
    run,
    statusLabel,
    totalJobs,
    stepsCompleted,
    stepsTotal,
    durationText,
    runnerSummary,
    anchor,
    open = $bindable(false),
  }: Props = $props()

  const statusKey = $derived(resolveStatusKey(run))
  const statusCssVar = $derived(`var(--${statusKeyToVar(statusKey)})`)
</script>

<Popover.Root bind:open>
  <Popover.Content
    side="right"
    align="start"
    customAnchor={anchor}
    class="hover-peek-popover"
    avoidCollisions={true}
    collisionPadding={8}
  >
    <div class="peek-row">
      <span class="status-icon-wrap" style="color: {statusCssVar};">
        <StatusIcon value={statusKey} />
      </span>
      <span class="status-label" style="color: {statusCssVar};">{statusLabel}</span>
    </div>
    <div class="peek-meta">
      <div>{totalJobs} {totalJobs === 1 ? 'job' : 'jobs'}</div>
      <div>{stepsCompleted} of {stepsTotal} steps complete</div>
      <div>Duration: {durationText}</div>
      {#if runnerSummary != null}
        <div>Runner: {runnerSummary}</div>
      {/if}
    </div>
  </Popover.Content>
</Popover.Root>

<style>
  /* The popover content portals to document.body — use :global to reach it. */
  :global(.hover-peek-popover) {
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    padding: 0.75rem;
    min-width: 12rem;
    box-shadow: 0 4px 12px oklch(0 0 0 / 0.2);
  }

  .peek-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.5rem;
    font-weight: 500;
  }

  .peek-meta > div {
    color: var(--text-dim);
    font-size: 0.875rem;
    line-height: 1.5;
  }
</style>
