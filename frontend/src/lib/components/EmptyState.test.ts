import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import EmptyState from './EmptyState.svelte'

describe('EmptyState', () => {
  it('renders default caption "Watching for runs."', () => {
    render(EmptyState)
    expect(screen.getByText('Watching for runs.')).toBeTruthy()
  })

  it('custom message prop overrides default caption', () => {
    render(EmptyState, { props: { message: 'No matching runs.' } })
    expect(screen.getByText('No matching runs.')).toBeTruthy()
    expect(screen.queryByText('Watching for runs.')).toBeNull()
  })

  it('renders Queued column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Queued')).toBeTruthy()
  })

  it('renders Running column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Running')).toBeTruthy()
  })

  it('renders Completed column group label', () => {
    render(EmptyState)
    expect(screen.getByText('Completed')).toBeTruthy()
  })

  it('renders three schematic column groups', () => {
    const { container } = render(EmptyState)
    const columns = container.querySelectorAll('[data-empty-col]')
    expect(columns.length).toBe(3)
  })

  it('each column group contains three placeholder rows', () => {
    const { container } = render(EmptyState)
    const columns = container.querySelectorAll('[data-empty-col]')
    for (const col of columns) {
      const rows = col.querySelectorAll('[data-empty-row]')
      expect(rows.length).toBe(3)
    }
  })
})
