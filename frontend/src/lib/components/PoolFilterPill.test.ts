import { fireEvent, render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import PoolFilterPill from './PoolFilterPill.svelte'

describe('PoolFilterPill', () => {
  // jsdom does not compute :focus-visible styles (the heuristic is browser-level);
  // computed-style verification lives in e2e/focus-rings.test.ts. Here we assert
  // the clear button is a proper interactive element (type="button", aria-label,
  // no inline outline suppression) so CSS rules can take effect in a real browser.
  it('clear button is a focusable interactive element with no inline outline suppression', () => {
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · x86',
        onClear: vi.fn(),
      },
    })

    const button = screen.getByRole('button', { name: 'Clear pool filter' })
    // Must be a button (keyboard-activatable)
    expect(button.tagName).toBe('BUTTON')
    // Must not carry inline outline:none that would override CSS
    expect(button.style.outline).not.toBe('none')
    expect(button.style.outlineStyle).not.toBe('none')
  })
  it('renders labelText content', () => {
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · self-hosted · x86',
        onClear: vi.fn(),
      },
    })

    expect(screen.getByText('linux · self-hosted · x86')).toBeTruthy()
    expect(screen.getByText('Filtering by')).toBeTruthy()
  })

  it('clear button has aria-label "Clear pool filter"', () => {
    render(PoolFilterPill, {
      props: {
        labelText: 'linux · x86',
        onClear: vi.fn(),
      },
    })

    const button = screen.getByRole('button', { name: 'Clear pool filter' })
    expect(button).toBeTruthy()
  })

  it('clicking the clear button invokes onClear', async () => {
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
