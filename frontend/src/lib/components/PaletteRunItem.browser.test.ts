import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import { createMockRun } from '$lib/test-utils/factories'
import Wrapper from './test-utils/PaletteRunItemWrapper.svelte'

const queuedRun = createMockRun({
  id: 1n,
  repo: 'actions/toolkit',
  branch: 'main',
  displayTitle: 'Test run',
  status: 'Queued',
})

const inProgressRun = createMockRun({
  id: 2n,
  repo: 'actions/toolkit',
  branch: 'main',
  status: 'InProgress',
  displayTitle: 'Deploy to prod',
})

const completedSuccessRun = createMockRun({
  id: 3n,
  repo: 'actions/toolkit',
  branch: 'main',
  status: 'Completed',
  conclusion: 'Success',
  displayTitle: 'CI checks',
})

const completedFailureRun = createMockRun({
  id: 4n,
  repo: 'actions/toolkit',
  branch: 'main',
  status: 'Completed',
  conclusion: 'Failure',
  displayTitle: 'Build failed',
})

const noBranchRun = createMockRun({
  id: 5n,
  repo: 'actions/toolkit',
  branch: null,
  displayTitle: 'Detached head',
})

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
    expect(screen.getByText(/actions\/toolkit.*main/)).toBeTruthy()
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
