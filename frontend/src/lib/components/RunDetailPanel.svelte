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
  import { getRovingContext } from '$lib/components/roving/context'

  const ctx = getRovingContext()

  // Read the run reactively
  const run = $derived(
    uiStore.selectedRunId !== null ? runStore.runs.get(uiStore.selectedRunId) : undefined
  )
  const jobs = $derived(
    uiStore.selectedRunId !== null ? (runStore.jobsByRunId.get(uiStore.selectedRunId) ?? []) : []
  )

  // AC2.9 missing-run fallback effect — clears selectedRunId when the id
  // references a run not present in runStore.runs (stale id, evicted run, etc.)
  // Path A (AC7.2 fix): when the evicted run is also the panel's trigger source,
  // call ctx.restoreFocusToInitial() directly here, bypassing Bits UI's
  // onCloseAutoFocus path. That path requires focus to be inside the Sheet's
  // FocusScope at close time, but {#if run} collapses the dialog content (removing
  // the close button from the DOM) before the FocusScope can see focus inside —
  // so onCloseAutoFocus never fires in this scenario and we must restore focus
  // ourselves. We also null lastTriggerRunId so the onCloseAutoFocus handler (if
  // it does fire) treats this as a no-trigger close and falls through to Bits UI
  // default, rather than re-running restoreFocusToInitial a second time.
  $effect(() => {
    if (uiStore.selectedRunId !== null && runStore.runs.get(uiStore.selectedRunId) === undefined) {
      const evictedId = uiStore.selectedRunId
      uiStore.selectedRunId = null
      if (uiStore.lastTriggerRunId === evictedId) {
        uiStore.lastTriggerRunId = null
        void ctx.restoreFocusToInitial()
      }
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

<Sheet.Root open={run !== undefined} onOpenChange={handleOpenChange}>
  <Sheet.Content
    side="right"
    showCloseButton={false}
    class="run-detail-panel data-[side=right]:sm:max-w-xl"
    escapeKeydownBehavior="defer-otherwise-close"
    interactOutsideBehavior="defer-otherwise-close"
    onCloseAutoFocus={(event) => {
      // Guard: only fire when truly closing (selectedRunId has been cleared by handleOpenChange).
      if (uiStore.selectedRunId !== null) return

      // Prevent Bits UI's built-in focus restoration unconditionally. The default
      // restoration tries to focus the original opener element, but in the evicted-
      // trigger case (AC7.2 / Path A) that element is gone, which would land focus
      // on <body>. We always supply our own restoration path below (or return with
      // focus already placed by the AC2.9 $effect's restoreFocusToInitial call).
      event.preventDefault()

      // Read and consume the trigger.
      const triggerId = uiStore.lastTriggerRunId
      uiStore.lastTriggerRunId = null

      // No trigger recorded (AC7.5) or already consumed by the AC2.9 $effect's
      // Path A restoration — nothing to do, focus is already correctly placed.
      if (triggerId === null) return
      const trigger = document.querySelector<HTMLElement>(
        `.run-card[data-run-id="${triggerId}"] .run-card-activate`
      )
      if (trigger !== null) {
        // Happy path. AC7.1.
        trigger.focus()
      } else {
        // Trigger was evicted while panel was open. AC7.2 / AC7.4.
        ctx.restoreFocusToInitial()
      }
    }}
  >
    {#if run}
      {@const statusKey = resolveStatusKey(run)}
      <div class="panel-top">
        <PanelHeader
          {statusKey}
          statusLabel={statusKeyToHumanLabel(statusKey)}
          title={run.displayTitle}
        />
        <PanelActions htmlUrl={run.htmlUrl} onClose={() => (uiStore.selectedRunId = null)} />
      </div>
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
      <div class="job-blocks min-h-0 flex-1 overflow-y-auto">
        {#each jobs as job (job.id)}
          <JobBlock
            {job}
            durationText={computeJobDurationText(job, uiStore.nowMs)}
            selectedJobId={uiStore.selectedJobId}
            onSelectedJobIdConsumed={handleSelectedJobIdConsumed}
          />
        {/each}
      </div>
    {/if}
  </Sheet.Content>
</Sheet.Root>

<style>
  .panel-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 1rem;
  }
</style>
