<script lang="ts">
  import StatusIcon from './StatusIcon.svelte'
  import StepList from './StepList.svelte'
  import StepItem from './StepItem.svelte'
  import type { Job } from '$lib/types/generated/Job'
  import { resolveJobStatusKey, statusKeyToVar } from '$lib/format/status-key'
  import { computeStepStatusKey, computeStepDurationText } from '$lib/format/step-helpers'

  export interface Props {
    job: Job
    durationText: string
    /** Set to job.id to focus this block. Parent clears this after the block scrolls. */
    selectedJobId: bigint | null
    /** Called after scrollIntoView fires so the parent can clear its selectedJobId state. */
    onSelectedJobIdConsumed?: () => void
  }

  let { job, durationText, selectedJobId, onSelectedJobIdConsumed }: Props = $props()

  let blockEl: HTMLElement | undefined = $state()
  const statusKey = $derived(resolveJobStatusKey(job))

  $effect(() => {
    if (selectedJobId === job.id && blockEl !== undefined) {
      requestAnimationFrame(() => {
        blockEl?.scrollIntoView({ block: 'start', behavior: 'smooth' })
        onSelectedJobIdConsumed?.()
      })
    }
  })
</script>

<section
  class="job-block"
  bind:this={blockEl}
  id={`job-${job.id}`}
  data-status-key={statusKey}
  style="--status-color: var(--{statusKeyToVar(statusKey)});"
>
  <header class="job-header">
    <span class="status"><StatusIcon value={statusKey} /></span>
    <span class="name">{job.name}</span>
    <span class="duration">{durationText}</span>
  </header>
  {#if job.steps.length > 0}
    <StepList>
      {#each job.steps as step (step.number)}
        <StepItem
          name={step.name}
          statusKey={computeStepStatusKey(step)}
          durationText={computeStepDurationText(step)}
        />
      {/each}
    </StepList>
  {/if}
</section>

<style>
  .job-block {
    /* Flex column with gap (instead of margin-bottom on .job-header) so the
       header is vertically centered between the top and bottom dividers when
       the job has no steps — gap only applies between siblings, so an absent
       StepList contributes no spacing. */
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    padding: 0.75rem 1.5rem;
    border-top: 1px solid var(--border);
  }
  .job-header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .status {
    color: var(--status-color);
  }
  .name {
    flex: 1;
    font-weight: 500;
  }
  .duration {
    color: var(--text-dim);
    font-variant-numeric: tabular-nums;
  }
</style>
