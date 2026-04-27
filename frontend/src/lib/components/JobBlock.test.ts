import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import { createMockJob } from '$lib/test-utils/factories'
import type { Step } from '$lib/types/generated/Step'
import JobBlock from './JobBlock.svelte'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeStep(overrides: Partial<Step> = {}): Step {
  return {
    number: 1n,
    name: 'test step',
    status: 'Queued',
    conclusion: null,
    startedAt: null,
    completedAt: null,
    ...overrides,
  }
}

// ---------------------------------------------------------------------------
// interactivity.AC2.1 (partial) — flat list of job blocks with header + step list
// ---------------------------------------------------------------------------

describe('JobBlock', () => {
  it('interactivity.AC2.1 sets id="job-{job.id}" on the section element', () => {
    const job = createMockJob({ id: 42n })
    const { container } = render(JobBlock, {
      props: { job, durationText: '1:23', selectedJobId: null },
    })

    const section = container.querySelector('section.job-block')
    expect(section).not.toBeNull()
    expect(section!.id).toBe('job-42')
  })

  it('interactivity.AC2.1 renders the job name in the header', () => {
    const job = createMockJob({ name: 'Run tests' })
    render(JobBlock, {
      props: { job, durationText: '0:45', selectedJobId: null },
    })

    expect(screen.getByText('Run tests')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders the durationText in the header', () => {
    const job = createMockJob({ status: 'InProgress' })
    render(JobBlock, {
      props: { job, durationText: '3:14', selectedJobId: null },
    })

    expect(screen.getByText('3:14')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders the status icon glyph for the job status', () => {
    const job = createMockJob({ status: 'Completed', conclusion: 'Success' })
    render(JobBlock, {
      props: { job, durationText: '2:00', selectedJobId: null },
    })

    // StatusIcon for Success renders ✓
    expect(screen.getByText('✓')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders one StepItem per step in job.steps', () => {
    const job = createMockJob({
      steps: [
        makeStep({ number: 1n, name: 'Checkout code', status: 'Completed', conclusion: 'Success' }),
        makeStep({ number: 2n, name: 'Install deps', status: 'Completed', conclusion: 'Success' }),
        makeStep({ number: 3n, name: 'Run tests', status: 'InProgress' }),
      ],
    })
    render(JobBlock, {
      props: { job, durationText: '1:00', selectedJobId: null },
    })

    expect(screen.getByText('Checkout code')).toBeTruthy()
    expect(screen.getByText('Install deps')).toBeTruthy()
    expect(screen.getByText('Run tests')).toBeTruthy()
  })

  it('interactivity.AC2.1 renders no step items when job.steps is empty', () => {
    const job = createMockJob({ steps: [] })
    const { container } = render(JobBlock, {
      props: { job, durationText: '—', selectedJobId: null },
    })

    // step-list ol is present but empty
    const ol = container.querySelector('ol.step-list')
    expect(ol).not.toBeNull()
    expect(ol!.children).toHaveLength(0)
  })

  it('interactivity.AC2.1 sets data-status-key attribute to the resolved status key', () => {
    const job = createMockJob({ status: 'Completed', conclusion: 'Failure' })
    const { container } = render(JobBlock, {
      props: { job, durationText: '0:30', selectedJobId: null },
    })

    const section = container.querySelector('section.job-block')
    expect(section!.getAttribute('data-status-key')).toBe('Failure')
  })

  it('interactivity.AC2.1 sets --status-color inline style to the correct CSS var', () => {
    const job = createMockJob({ status: 'Queued', conclusion: null })
    const { container } = render(JobBlock, {
      props: { job, durationText: '—', selectedJobId: null },
    })

    const section = container.querySelector('section.job-block') as HTMLElement
    expect(section.getAttribute('style')).toContain('--status-color: var(--queued)')
  })
})
