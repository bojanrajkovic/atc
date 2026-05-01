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
    <!-- Title row: icon + displayTitle (mirrors playground hover-popover-title) -->
    <h4 class="peek-title" style="--status-color: {statusCssVar};">
      <StatusIcon value={statusKey} />
      <span class="peek-title-text">{run.displayTitle}</span>
    </h4>

    <!-- Label/value rows (mirrors playground .hover-popover-row structure) -->
    <div class="peek-rows">
      <div class="peek-row">
        <span class="peek-label">Status</span>
        <strong class="peek-value status-label">{statusLabel}</strong>
      </div>
      <div class="peek-row">
        <span class="peek-label">Jobs</span>
        <strong class="peek-value">{totalJobs}</strong>
      </div>
      <div class="peek-row">
        <span class="peek-label">Steps complete</span>
        <strong class="peek-value">{stepsCompleted}/{stepsTotal}</strong>
      </div>
      <div class="peek-row">
        <span class="peek-label">Duration</span>
        <strong class="peek-value">{durationText}</strong>
      </div>
      {#if runnerSummary != null}
        <div class="peek-row">
          <span class="peek-label">Runner</span>
          <strong class="peek-value">{runnerSummary}</strong>
        </div>
      {/if}
    </div>

    <!-- Keyboard hint footer (mirrors playground .hover-popover-hint) -->
    <div class="peek-hint">
      Click for full panel · <kbd>Enter</kbd> to open
    </div>
  </Popover.Content>
</Popover.Root>

<style>
  /* The popover content portals to document.body — use :global to reach it. */
  :global(.hover-peek-popover) {
    background: var(--surface-raised);
    border: 1px solid var(--border);
    border-radius: 0.5rem;
    padding: 0.625rem 0.75rem;
    min-width: 14rem;
    max-width: 17.5rem;
    box-shadow: 0 4px 12px oklch(0 0 0 / 0.2);
    font-size: 0.75rem;
  }

  /* Title row */
  .peek-title {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    margin: 0 0 0.375rem;
    font-size: 0.8125rem;
    font-weight: 600;
    color: var(--text);
    line-height: 1.2;
  }

  .peek-title-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Label/value rows */
  .peek-rows {
    display: flex;
    flex-direction: column;
    gap: 0.125rem;
  }

  .peek-row {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 0.5rem;
    margin: 0.125rem 0;
    font-variant-numeric: tabular-nums;
  }

  .peek-label {
    color: var(--text-dim);
    white-space: nowrap;
  }

  .peek-value {
    color: var(--text);
    font-weight: 500;
    text-align: right;
  }

  .status-label {
    color: var(--status-color);
  }

  /* Keyboard hint footer */
  .peek-hint {
    margin-top: 0.5rem;
    padding-top: 0.375rem;
    border-top: 1px solid var(--border);
    font-size: 0.625rem;
    color: var(--text-quiet);
    letter-spacing: 0.04em;
  }

  .peek-hint kbd {
    font-family: var(--mono);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
    font-size: 0.625rem;
  }
</style>
