<script lang="ts">
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import { formatDuration, parseIso } from '$lib/format/duration'
  import { resolveStatusKey, type StatusKey } from '$lib/format/status-key'
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
  const statusColor = $derived(resolveStatusColorVar(statusKey))

  /**
   * State-aware duration.
   * Static-Completed branch MUST NOT read uiStore.nowMs — that is the
   * mechanism by which Completed cards stop subscribing to the wall-clock
   * tick (see AC10.7 + AC12.7 fake-timer proof).
   */
  const durationText = $derived.by<string>(() => {
    if (run.status === 'Completed' && run.conclusion !== 'ActionRequired') {
      if (run.runStartedAt === null) return '\u2014'
      return formatDuration({
        kind: 'static',
        startMs: parseIso(run.runStartedAt),
        endMs: parseIso(run.updatedAt),
      })
    }
    const nowMs = uiStore.nowMs
    if (run.status === 'Queued') {
      return `waiting ${formatDuration({
        kind: 'live',
        startMs: parseIso(run.createdAt),
        nowMs,
      })}`
    }
    if (run.status === 'InProgress') {
      const startIso = run.runStartedAt ?? run.createdAt
      return formatDuration({ kind: 'live', startMs: parseIso(startIso), nowMs })
    }
    // Completed + ActionRequired
    return `awaiting action ${formatDuration({
      kind: 'live',
      startMs: parseIso(run.updatedAt),
      nowMs,
    })}`
  })

  function resolveStatusColorVar(key: StatusKey): string {
    switch (key) {
      case 'Queued':
        return 'var(--queued)'
      case 'InProgress':
        return 'var(--running)'
      case 'Success':
        return 'var(--success)'
      case 'Failure':
        return 'var(--failed)'
      case 'Cancelled':
        return 'var(--cancelled)'
      case 'TimedOut':
        return 'var(--timed-out)'
      case 'ActionRequired':
        return 'var(--action-required)'
      case 'StartupFailure':
        return 'var(--failed)'
      case 'Stale':
        return 'var(--neutral)'
      case 'Neutral':
        return 'var(--neutral)'
      case 'Skipped':
        return 'var(--neutral)'
    }
  }
</script>

<article
  class="run-card"
  data-run-id={run.id}
  data-status={run.status}
  style="--status-color: {statusColor};"
>
  <JobHeader displayTitle={run.displayTitle} statusValue={statusKey} {durationText} />
  <JobMeta repo={run.repo} branch={run.branch} />
  <ProgressBar completed={jobStats.completed} total={jobStats.total} />
  <RunnerLabel summary={jobStats.runnerSummary} />
</article>

<style>
  .run-card {
    position: relative;
    padding: 12px 14px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    overflow: hidden;
  }

  .run-card::before {
    content: '';
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 3px;
    background: var(--status-color);
  }
</style>
