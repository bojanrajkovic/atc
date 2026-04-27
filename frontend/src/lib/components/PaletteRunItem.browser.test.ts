import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import Wrapper from './test-utils/PaletteRunItemWrapper.svelte'

const queuedRun: WorkflowRun = {
  id: 1n,
  repo: 'actions/toolkit',
  branch: 'main',
  displayTitle: 'Test run',
  status: 'Queued',
  conclusion: null,
  createdAt: new Date().toISOString(),
  completedAt: null,
  queuedDurationMs: null,
  runDurationMs: null,
}

const inProgressRun: WorkflowRun = {
  ...queuedRun,
  id: 2n,
  status: 'InProgress',
  displayTitle: 'Deploy to prod',
}

const completedSuccessRun: WorkflowRun = {
  ...queuedRun,
  id: 3n,
  status: 'Completed',
  conclusion: 'Success',
  displayTitle: 'CI checks',
}

const completedFailureRun: WorkflowRun = {
  ...queuedRun,
  id: 4n,
  status: 'Completed',
  conclusion: 'Failure',
  displayTitle: 'Build failed',
}

const noBranchRun: WorkflowRun = {
  ...queuedRun,
  id: 5n,
  branch: null,
  displayTitle: 'Detached head',
}

describe('PaletteRunItem (browser)', () => {
  it('renders the run displayTitle', () => {
    render(Wrapper, {
      props: {
        run: queuedRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('Test run')).toBeTruthy()
  })

  it('renders repo · branch meta line', () => {
    render(Wrapper, {
      props: {
        run: inProgressRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('actions/toolkit')).toBeTruthy()
    expect(screen.getByText(/main/)).toBeTruthy()
  })

  it('renders repo only when branch is null', () => {
    const { container } = render(Wrapper, {
      props: {
        run: noBranchRun,
        onSelect: () => {},
      },
    })
    const html = container.innerHTML
    expect(html).toContain('actions/toolkit')
    expect(/·/.test(html)).toBe(false)
  })

  it('renders Queued icon', () => {
    render(Wrapper, {
      props: {
        run: queuedRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('◐')).toBeTruthy()
  })

  it('renders InProgress icon', () => {
    render(Wrapper, {
      props: {
        run: inProgressRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('▶')).toBeTruthy()
  })

  it('renders Success icon', () => {
    render(Wrapper, {
      props: {
        run: completedSuccessRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('✓')).toBeTruthy()
  })

  it('renders Failure icon', () => {
    render(Wrapper, {
      props: {
        run: completedFailureRun,
        onSelect: () => {},
      },
    })
    expect(screen.getByText('✗')).toBeTruthy()
  })
})
