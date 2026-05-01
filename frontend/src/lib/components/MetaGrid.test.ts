import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import MetaGridWrapper from './test-utils/MetaGridWrapper.svelte'

describe('MetaGrid', () => {
  it('interactivity.AC2.1 renders all three MetaCell children in source order', () => {
    render(MetaGridWrapper, {
      props: {
        cells: [
          { label: 'Commit', value: 'abc1234' },
          { label: 'Event', value: 'push' },
          { label: 'Runner', value: 'ubuntu-latest' },
        ],
      },
    })

    const labels = screen.getAllByText(/Commit|Event|Runner/)
    expect(labels).toHaveLength(3)

    // Assert source order is preserved — non-null assertions safe: length asserted above
    expect(labels[0]!.textContent).toBe('Commit')
    expect(labels[1]!.textContent).toBe('Event')
    expect(labels[2]!.textContent).toBe('Runner')
  })

  it('interactivity.AC2.1 renders with dl semantics', () => {
    const { container } = render(MetaGridWrapper, {
      props: {
        cells: [{ label: 'Commit', value: 'abc1234' }],
      },
    })

    const dl = container.querySelector('dl')
    expect(dl).not.toBeNull()
  })
})
