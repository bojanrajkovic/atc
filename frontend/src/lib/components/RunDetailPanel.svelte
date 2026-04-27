<script lang="ts">
  import * as Sheet from '$lib/components/ui/sheet'
  import PanelHeader from './PanelHeader.svelte'
  import PanelActions from './PanelActions.svelte'
  import MetaGrid from './MetaGrid.svelte'
  import MetaCell from './MetaCell.svelte'
  import JobBlock from './JobBlock.svelte'
  import { uiStore } from '$lib/stores/ui.svelte'
  import { runStore } from '$lib/stores/runs.svelte'
  import { resolveStatusKey, statusKeyToHumanLabel } from '$lib/format/status-key'
  import { computeDurationText, computeJobDurationText } from '$lib/format/duration-text'
  import { formatTimestamp } from '$lib/format/timestamp'
  import { summarizeRunners } from '$lib/format/runners'

  // Read the run reactively
  const run = $derived(
    uiStore.selectedRunId !== null ? runStore.runs.get(uiStore.selectedRunId) : undefined
  )
  const jobs = $derived(
    uiStore.selectedRunId !== null ? (runStore.jobsByRunId.get(uiStore.selectedRunId) ?? []) : []
  )

  // AC2.9 missing-run fallback effect — clears selectedRunId when the id
  // references a run not present in runStore.runs (stale id, evicted run, etc.)
  $effect(() => {
    if (uiStore.selectedRunId !== null && runStore.runs.get(uiStore.selectedRunId) === undefined) {
      uiStore.selectedRunId = null
    }
  })

  // When Sheet fires onOpenChange(false) (Esc / click-outside / X button),
  // clear selectedRunId so the {#if run} block unmounts the Sheet.
  function handleOpenChange(open: boolean) {
    if (!open) uiStore.selectedRunId = null
  }

  // Called by JobBlock after its scroll RAF fires, so the parent clears
  // selectedJobId and doesn't re-scroll on subsequent re-renders.
  function handleSelectedJobIdConsumed() {
    uiStore.selectedJobId = null
  }
</script>

{#if run}
  <Sheet.Root open={true} onOpenChange={handleOpenChange}>
    <Sheet.Content side="right" class="run-detail-panel">
      {@const statusKey = resolveStatusKey(run)}
      <PanelHeader
        {statusKey}
        statusLabel={statusKeyToHumanLabel(statusKey)}
        title={run.displayTitle}
      />
      <PanelActions htmlUrl={run.htmlUrl} onClose={() => (uiStore.selectedRunId = null)} />
      <MetaGrid>
        <MetaCell label="Commit" value={run.headSha.slice(0, 7)} />
        <MetaCell label="Event" value={run.event} />
        <MetaCell label="Triggered by" value={run.commitMessage?.split('\n')[0] ?? null} />
        <MetaCell
          label="Started"
          value={run.runStartedAt ? formatTimestamp(run.runStartedAt) : null}
        />
        <MetaCell label="Duration" value={computeDurationText(run, uiStore.nowMs)} />
        <MetaCell label="Runner" value={summarizeRunners(jobs)} />
      </MetaGrid>
      <div class="job-blocks">
        {#each jobs as job (job.id)}
          <JobBlock
            {job}
            durationText={computeJobDurationText(job, uiStore.nowMs)}
            selectedJobId={uiStore.selectedJobId}
            onSelectedJobIdConsumed={handleSelectedJobIdConsumed}
          />
        {/each}
      </div>
    </Sheet.Content>
  </Sheet.Root>
{/if}
