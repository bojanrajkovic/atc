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
  import { runStore } from '$lib/stores/runs.svelte'
  import JobHeader from './JobHeader.svelte'
  import JobMeta from './JobMeta.svelte'
  import ProgressBar from './ProgressBar.svelte'
  import RunnerLabel from './RunnerLabel.svelte'
  import HoverPeekPopover from './HoverPeekPopover.svelte'

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
   * Step aggregations for the hover-peek popover (AC3.1).
   * Reads jobsByRunId derived from runStore to get the raw Job[] for this run.
   * "Completed" is the exact StepStatus variant confirmed from StepStatus.ts.
   */
  const jobs = $derived(runStore.jobsByRunId.get(run.id) ?? [])
  const stepsTotal = $derived(jobs.reduce((acc, j) => acc + j.steps.length, 0))
  const stepsCompleted = $derived(
    jobs.reduce((acc, j) => acc + j.steps.filter((s) => s.status === 'Completed').length, 0)
  )

  /** Reference to the article element — passed as anchor to HoverPeekPopover. */
  let articleEl: HTMLElement | undefined = $state()

  /** Whether the hover-peek popover is currently open. */
  let popoverOpen = $state(false)

  /**
   * Non-reactive hover debounce timer. Not in $state because the timer id
   * itself doesn't need to drive any reactive computation.
   */
  let hoverTimer: ReturnType<typeof setTimeout> | null = null

  /**
   * Whether the device supports hover (matches the media query).
   * Single source of truth at the card level — touch devices never
   * instantiate the timer or the popover (AC3.1).
   */
  let canHover = $state(false)

  /**
   * Media query subscription for hover capability.
   * Runs on mount; the returned arrow cleans up the listener on destroy.
   */
  $effect(() => {
    if (typeof window === 'undefined') return
    const mq = window.matchMedia('(hover: hover) and (pointer: fine)')
    canHover = mq.matches
    const handler = (e: MediaQueryListEvent) => {
      canHover = e.matches
    }
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  })

  /**
   * Cleanup effect — clears any pending hover timer on component destroy.
   * Outer body has no reactive reads so it runs once; the returned arrow
   * runs on unmount.
   */
  $effect(() => () => {
    if (hoverTimer !== null) clearTimeout(hoverTimer)
  })

  function handleMouseEnter() {
    if (!canHover) return
    if (hoverTimer !== null) clearTimeout(hoverTimer)
    hoverTimer = setTimeout(() => {
      popoverOpen = true
      hoverTimer = null
    }, 250)
  }

  function handleMouseLeave() {
    if (!canHover) return
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer)
      hoverTimer = null
    }
    popoverOpen = false
  }

  /**
   * Handles activation of the inner button (click, or Enter/Space via native
   * button semantics). Clears hover timer + closes popover synchronously,
   * then sets both lastTriggerRunId (for Phase 6 focus restoration) and
   * selectedRunId (opens RunDetailPanel).
   * No custom keydown handler — native <button> fires click on Enter/Space.
   */
  function handleActivate() {
    if (hoverTimer !== null) {
      clearTimeout(hoverTimer)
      hoverTimer = null
    }
    popoverOpen = false
    uiStore.lastTriggerRunId = run.id
    uiStore.selectedRunId = run.id
  }
</script>

<article
  class="run-card"
  bind:this={articleEl}
  data-run-id={run.id}
  data-status={run.status}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
  onmouseenter={handleMouseEnter}
  onmouseleave={handleMouseLeave}
>
  <button class="run-card-activate" type="button" aria-label={ariaLabel} onclick={handleActivate}
  ></button>
  <JobHeader displayTitle={run.displayTitle} statusValue={statusKey} {durationText} />
  <JobMeta repo={run.repo} branch={run.branch} />
  <ProgressBar completed={jobStats.completed} total={jobStats.total} />
  <RunnerLabel summary={jobStats.runnerSummary} />
</article>

{#if canHover}
  <HoverPeekPopover
    {run}
    statusLabel={statusKeyToHumanLabel(statusKey)}
    totalJobs={jobStats.total}
    {stepsCompleted}
    {stepsTotal}
    {durationText}
    runnerSummary={jobStats.runnerSummary}
    anchor={articleEl ?? null}
    bind:open={popoverOpen}
  />
{/if}

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
