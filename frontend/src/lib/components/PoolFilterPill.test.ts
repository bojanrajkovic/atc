import { fireEvent, render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import PoolFilterPill from './PoolFilterPill.svelte'

describe('PoolFilterPill', () => {
  it('AC5.3: renders labelText content', () => {
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · self-hosted · x86',
        onClear: vi.fn(),
      },
    })

    expect(screen.getByText('linux · self-hosted · x86')).toBeTruthy()
    expect(screen.getByText('Filtering by')).toBeTruthy()
  })

  it('AC5.3: clear button has aria-label "Clear pool filter"', () => {
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · x86',
        onClear: vi.fn(),
      },
    })

    const button = screen.getByRole('button', { name: 'Clear pool filter' })
    expect(button).toBeTruthy()
  })

  it('AC5.3: clicking the clear button invokes onClear', async () => {
    const onClear = vi.fn()
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · x86',
        onClear,
      },
    })

    const button = screen.getByRole('button', { name: 'Clear pool filter' })
    await fireEvent.click(button)

    expect(onClear).toHaveBeenCalledOnce()
  })
})
