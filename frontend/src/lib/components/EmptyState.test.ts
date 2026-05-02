import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import EmptyState from './EmptyState.svelte'

describe('EmptyState', () => {
  /**
   * AC1.1: renders with default caption when no message prop given
   */
  it('AC1.1 — renders default caption "Watching for runs."', () => {
    render(EmptyState)
    expect(screen.getByText('Watching for runs.')).toBeTruthy()
  })

  /**
   * AC1.2: custom message prop overrides the default caption
   */
  it('AC1.2 — custom message prop overrides default caption', () => {
    render(EmptyState, { props: { message: 'No matching runs.' } })
    expect(screen.getByText('No matching runs.')).toBeTruthy()
    expect(screen.queryByText('Watching for runs.')).toBeNull()
  })

  /**
   * AC1.3: schematic preview renders three labeled column groups (Queued,
   * Running, Completed), each containing placeholder dot rows
   */
  it('AC1.3 — renders Queued column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Queued')).toBeTruthy()
  })

  it('AC1.3 — renders Running column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Running')).toBeTruthy()
  })

  it('AC1.3 — renders Completed column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Completed')).toBeTruthy()
  })

  it('AC1.3 — renders three schematic column groups', () => {
    const { container } = render(EmptyState)
    const columns = container.querySelectorAll('[data-empty-col]')
    expect(columns.length).toBe(3)
  })

  it('AC1.3 — each column group contains three placeholder rows', () => {
    const { container } = render(EmptyState)
    const columns = container.querySelectorAll('[data-empty-col]')
    for (const col of columns) {
      const rows = col.querySelectorAll('[data-empty-row]')
      expect(rows.length).toBe(3)
    }
  })
})
