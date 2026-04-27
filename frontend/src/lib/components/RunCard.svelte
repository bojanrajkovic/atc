<script lang="ts">
  import type { JobStats } from '$lib/stores/runs.svelte'
  import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
  import { computeDurationText } from '$lib/format/duration-text'
  import { resolveStatusKey, statusKeyToVar, type StatusKey } from '$lib/format/status-key'
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
</script>

<article
  class="run-card"
  data-run-id={run.id}
  data-status={run.status}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
>
  <JobHeader displayTitle={run.displayTitle} statusValue={statusKey} {durationText} />
  <JobMeta repo={run.repo} branch={run.branch} />
  <ProgressBar completed={jobStats.completed} total={jobStats.total} />
  <RunnerLabel summary={jobStats.runnerSummary} />
</article>
