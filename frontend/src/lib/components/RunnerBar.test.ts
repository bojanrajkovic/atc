import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import RunnerBar from './RunnerBar.svelte'

describe('RunnerBar', () => {
  it('renders one RunnerPool per pool entry', () => {
    const pools = [
      { label: 'linux', running: 3, queued: 0, total: 10, isElastic: false },
      { label: 'windows', running: 2, queued: 0, total: 8, isElastic: false },
      { label: 'macos', running: 1, queued: 0, total: 5, isElastic: false },
    ]

    const { container } = render(RunnerBar, {
      props: { pools },
    })

    const listitems = container.querySelectorAll('[role="listitem"]')
    expect(listitems).toHaveLength(3)

    expect(screen.getByText('linux')).toBeTruthy()
    expect(screen.getByText('windows')).toBeTruthy()
    expect(screen.getByText('macos')).toBeTruthy()
  })

  it('renders empty state when no pools', () => {
    render(RunnerBar, {
      props: { pools: [] },
    })

    const emptyText = screen.getByText('No pools')
    expect(emptyText).toBeTruthy()
  })

  it('renders single pool correctly', () => {
    const pools = [{ label: 'linux', running: 3, queued: 0, total: 10, isElastic: false }]

    const { container } = render(RunnerBar, {
      props: { pools },
    })

    const listitems = container.querySelectorAll('[role="listitem"]')
    expect(listitems).toHaveLength(1)

    expect(screen.getByText('linux')).toBeTruthy()
  })

  it('has accessible list label', () => {
    const pools = [{ label: 'linux', running: 3, queued: 0, total: 10, isElastic: false }]

    render(RunnerBar, {
      props: { pools },
    })

    const list = screen.getByRole('list', { name: /runner pools/i })
    expect(list).toBeTruthy()
  })
})
