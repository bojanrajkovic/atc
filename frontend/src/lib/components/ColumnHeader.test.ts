import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import ColumnHeader from './ColumnHeader.svelte'

describe('ColumnHeader', () => {
  it('renders label uppercased in heading with correct id', () => {
    render(ColumnHeader, {
      props: {
        label: 'queued',
        count: 3,
        headingId: 'column-queued',
      },
    })

    const heading = screen.getByRole('heading', { level: 2 })
    expect(heading).toBeTruthy()
    expect(heading.id).toBe('column-queued')
    expect(heading.textContent).toContain('QUEUED')
  })

  it('renders count badge as plain text (AC2.1)', () => {
    render(ColumnHeader, {
      props: {
        label: 'queued',
        count: 3,
        headingId: 'column-queued',
      },
    })

    const countBadge = screen.getByText('3')
    expect(countBadge).toBeTruthy()
  })

  it('does not use role="status" for count badge (AC2.1)', () => {
    render(ColumnHeader, {
      props: {
        label: 'queued',
        count: 3,
        headingId: 'column-queued',
      },
    })

    const statusElements = screen.queryAllByRole('status')
    expect(statusElements).toHaveLength(0)
  })

  it('renders zero count badge when count is 0 (AC2.2)', () => {
    render(ColumnHeader, {
      props: {
        label: 'completed',
        count: 0,
        headingId: 'column-completed',
      },
    })

    const countBadge = screen.getByText('0')
    expect(countBadge).toBeTruthy()
  })

  it('renders count with different statuses', () => {
    const { unmount } = render(ColumnHeader, {
      props: {
        label: 'running',
        count: 5,
        headingId: 'column-running',
      },
    })

    const heading = screen.getByRole('heading', { level: 2 })
    expect(heading.textContent).toContain('RUNNING')
    expect(screen.getByText('5')).toBeTruthy()

    unmount()

    render(ColumnHeader, {
      props: {
        label: 'completed',
        count: 10,
        headingId: 'column-completed',
      },
    })

    const heading2 = screen.getByRole('heading', { level: 2 })
    expect(heading2.textContent).toContain('COMPLETED')
    expect(screen.getByText('10')).toBeTruthy()
  })
})
