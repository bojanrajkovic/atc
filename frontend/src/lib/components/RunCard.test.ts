import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { RunId } from '$lib/types/generated/RunId'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import RunCard from './RunCard.svelte'

// Test helper to create mock WorkflowRun objects
function createMockRun(overrides: Partial<WorkflowRun> = {}): WorkflowRun {
  const baseId: RunId = 123n
  return {
    id: baseId,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'Test Workflow',
    workflowPath: '.github/workflows/test.yml',
    branch: 'main',
    headSha: 'abc123def456',
    commitMessage: 'Test commit',
    event: 'push',
    displayTitle: 'Test Run',
    status: 'Queued',
    conclusion: null,
    htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/123',
    createdAt: '2024-01-01T00:00:00Z',
    runStartedAt: null,
    updatedAt: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('RunCard', () => {
  it('renders displayTitle as visible text (AC4.1)', () => {
    const run = createMockRun({ displayTitle: 'Test Workflow Run' })
    render(RunCard, {
      props: { run },
    })

    const title = screen.getByText('Test Workflow Run')
    expect(title).toBeTruthy()
  })

  it('renders status indicator with Queued status (AC4.2)', () => {
    const run = createMockRun({ status: 'Queued' })
    render(RunCard, {
      props: { run },
    })

    // Check for Queued glyph (○)
    const glyph = screen.getByText('\u25CB')
    expect(glyph).toBeTruthy()

    // Check for sr-only text
    const srOnly = screen.getByText('Queued')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('renders status indicator with InProgress status (AC4.2)', () => {
    const run = createMockRun({ status: 'InProgress' })
    render(RunCard, {
      props: { run },
    })

    // Check for InProgress glyph (▶)
    const glyph = screen.getByText('\u25B6')
    expect(glyph).toBeTruthy()

    // Check for sr-only text
    const srOnly = screen.getByText('In Progress')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('renders status indicator with Completed status (AC4.2)', () => {
    const run = createMockRun({ status: 'Completed' })
    render(RunCard, {
      props: { run },
    })

    // Check for Completed glyph (●)
    const glyph = screen.getByText('\u25CF')
    expect(glyph).toBeTruthy()

    // Check for sr-only text
    const srOnly = screen.getByText('Completed')
    expect(srOnly).toBeTruthy()
    expect(srOnly.className).toContain('sr-only')
  })

  it('applies correct color variable for Queued status (AC4.2)', () => {
    const run = createMockRun({ status: 'Queued' })
    render(RunCard, {
      props: { run },
    })

    const indicator = screen.getByText('\u25CB').parentElement
    expect(indicator?.style.color).toBe('var(--queued)')
  })

  it('applies correct color variable for InProgress status (AC4.2)', () => {
    const run = createMockRun({ status: 'InProgress' })
    render(RunCard, {
      props: { run },
    })

    const indicator = screen.getByText('\u25B6').parentElement
    expect(indicator?.style.color).toBe('var(--running)')
  })

  it('applies correct color variable for Completed status (AC4.2)', () => {
    const run = createMockRun({ status: 'Completed' })
    render(RunCard, {
      props: { run },
    })

    const indicator = screen.getByText('\u25CF').parentElement
    expect(indicator?.style.color).toBe('var(--text-dim)')
  })

  it('has data-run-id attribute for test targeting', () => {
    const run = createMockRun({ id: 456n })
    const { container } = render(RunCard, {
      props: { run },
    })

    const element = container.querySelector('[data-run-id]')
    expect(element).toBeTruthy()
    expect(element?.getAttribute('data-run-id')).toBe('456')
  })
})
