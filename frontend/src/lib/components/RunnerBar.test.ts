import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { RunnerPoolTotal } from '$lib/types/generated/RunnerPoolTotal'
import RunnerBar from './RunnerBar.svelte'

const bounded = (value: number): RunnerPoolTotal => ({ kind: 'Bounded', value })

describe('RunnerBar', () => {
  it('renders one RunnerPool per pool entry', () => {
    const pools = [
      {
        key: 'linux',
        label: 'linux',
        running: 3,
        queued: 0,
        total: bounded(10),
        isActiveFilter: false,
      },
      {
        key: 'windows',
        label: 'windows',
        running: 2,
        queued: 0,
        total: bounded(8),
        isActiveFilter: false,
      },
      {
        key: 'macos',
        label: 'macos',
        running: 1,
        queued: 0,
        total: bounded(5),
        isActiveFilter: false,
      },
    ]

    render(RunnerBar, {
      props: { pools },
    })

    const listitems = screen.getAllByRole('listitem')
    expect(listitems).toHaveLength(3)

    expect(screen.getByText('linux')).toBeTruthy()
    expect(screen.getByText('windows')).toBeTruthy()
    expect(screen.getByText('macos')).toBeTruthy()
  })

  it('renders empty state when no pools', () => {
    render(RunnerBar, {
      props: { pools: [] },
    })

    expect(screen.getByText('No active runners')).toBeTruthy()
  })

  it('renders single pool correctly', () => {
    const pools = [
      {
        key: 'linux',
        label: 'linux',
        running: 3,
        queued: 0,
        total: bounded(10),
        isActiveFilter: false,
      },
    ]

    render(RunnerBar, {
      props: { pools },
    })

    expect(screen.getAllByRole('listitem')).toHaveLength(1)
    expect(screen.getByText('linux')).toBeTruthy()
  })

  it('has accessible list label', () => {
    const pools = [
      {
        key: 'linux',
        label: 'linux',
        running: 3,
        queued: 0,
        total: bounded(10),
        isActiveFilter: false,
      },
    ]

    render(RunnerBar, {
      props: { pools },
    })

    expect(screen.getByRole('list', { name: /runner pools/i })).toBeTruthy()
  })

  it('pool with isActiveFilter=true gets is-active-filter class on its RunnerPool', () => {
    const pools = [
      {
        key: 'linux',
        label: 'linux',
        running: 3,
        queued: 0,
        total: bounded(10),
        isActiveFilter: true,
      },
      {
        key: 'windows',
        label: 'windows',
        running: 2,
        queued: 0,
        total: bounded(8),
        isActiveFilter: false,
      },
    ]

    const { container } = render(RunnerBar, { props: { pools } })

    const matching = container.querySelector(
      '[data-testid="runner-pool-linux"]',
    ) as HTMLElement | null
    const other = container.querySelector(
      '[data-testid="runner-pool-windows"]',
    ) as HTMLElement | null

    expect(matching).not.toBeNull()
    expect(other).not.toBeNull()
    expect(matching!.classList.contains('is-active-filter')).toBe(true)
    expect(other!.classList.contains('is-active-filter')).toBe(false)
  })

  it('when no pool has isActiveFilter=true, no RunnerPool gets is-active-filter class', () => {
    const pools = [
      {
        key: 'linux',
        label: 'linux',
        running: 3,
        queued: 0,
        total: bounded(10),
        isActiveFilter: false,
      },
      {
        key: 'windows',
        label: 'windows',
        running: 2,
        queued: 0,
        total: bounded(8),
        isActiveFilter: false,
      },
    ]

    const { container } = render(RunnerBar, { props: { pools } })

    expect(container.querySelectorAll('.is-active-filter').length).toBe(0)
  })
})
