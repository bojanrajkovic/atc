<script lang="ts">
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import { computeDurationText } from '$lib/format/duration-text'
  import {
    resolveStatusKey,
    statusKeyToVar,
    statusKeyToHumanLabel,
    type StatusKey,
  } from '$lib/format/status-key'
  import { uiStore } from '$lib/stores/ui.svelte'
  import JobHeader from './JobHeader.svelte'
  import JobMeta from './JobMeta.svelte'
  import ProgressBar from './ProgressBar.svelte'
  import RunnerLabel from './RunnerLabel.svelte'

  export interface RunCardProps {
    run: WorkflowRun
    jobStats: JobStats
  }

  let { run, jobStats }: RunCardProps = $props()

  const statusKey: StatusKey = $derived(resolveStatusKey(run))

  /**
   * State-aware duration. The static-Completed branch inside
   * computeDurationText does NOT read nowMs — so when `run` is a Completed
   * non-ActionRequired run, the short-circuit returns before `uiStore.nowMs`
   * is accessed and the derivation never registers nowMs as a dependency
   * (AC10.7 + AC12.7).
   */
  const durationText = $derived.by<string>(() => {
    if (run.status === 'Completed' && run.conclusion !== 'ActionRequired') {
      return computeDurationText(run, 0)
    }
    return computeDurationText(run, uiStore.nowMs)
  })

  /**
   * aria-label for the inner activator button (AC4.7).
   * Format: "{displayTitle}, {statusLabel}, {repo}·{branch}" when branch is
   * non-null, or "{displayTitle}, {statusLabel}, {repo}" when branch is null.
   */
  const ariaLabel = $derived.by<string>(() => {
    const statusLabel = statusKeyToHumanLabel(statusKey)
    const repoPart = run.branch != null ? `${run.repo}·${run.branch}` : run.repo
    return `${run.displayTitle}, ${statusLabel}, ${repoPart}`
  })

  /**
   * Handles activation of the inner button (click, or Enter/Space via native
   * button semantics). Sets both lastTriggerRunId (for Phase 6 focus
   * restoration) and selectedRunId (opens RunDetailPanel).
   * No custom keydown handler — native <button> fires click on Enter/Space.
   */
  function handleActivate() {
    uiStore.lastTriggerRunId = run.id
    uiStore.selectedRunId = run.id
  }
</script>

<article
  class="run-card"
  data-run-id={run.id}
  data-status={run.status}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
>
  <button class="run-card-activate" type="button" aria-label={ariaLabel} onclick={handleActivate}
  ></button>
  <JobHeader displayTitle={run.displayTitle} statusValue={statusKey} {durationText} />
  <JobMeta repo={run.repo} branch={run.branch} />
  <ProgressBar completed={jobStats.completed} total={jobStats.total} />
  <RunnerLabel summary={jobStats.runnerSummary} />
</article>

<style>
  /* Inner activator button — covers the entire card surface via absolute
     positioning. The article already has position: relative in app.css. */
  .run-card-activate {
    position: absolute;
    inset: 0;
    z-index: 1;
    background: transparent;
    border: 0;
    padding: 0;
    margin: 0;
    cursor: pointer;
  }

  .run-card-activate:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    border-radius: 8px;
  }
</style>
