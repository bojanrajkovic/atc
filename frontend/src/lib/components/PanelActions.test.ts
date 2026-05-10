import { fireEvent, render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'

import PanelActions from './PanelActions.svelte'

describe('PanelActions', () => {
  const htmlUrl = 'https://github.com/owner/repo/actions/runs/12345'

  // jsdom does not compute :focus-visible styles (the heuristic is browser-level);
  // computed-style verification lives in e2e/focus-rings.test.ts. Here we assert
  // both interactive elements are standard HTML elements with no inline outline
  // suppression, so the CSS rules can take effect in a real browser.
  it('close button is a focusable button with no inline outline suppression', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const button = screen.getByRole('button', { name: 'Close detail panel' })
    expect(button.tagName).toBe('BUTTON')
    expect(button.style.outline).not.toBe('none')
    expect(button.style.outlineStyle).not.toBe('none')
  })

  it('go-to-run link is a focusable anchor with no inline outline suppression', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const link = screen.getByRole('link', { name: 'Go to run' })
    expect(link.tagName).toBe('A')
    expect(link.style.outline).not.toBe('none')
    expect(link.style.outlineStyle).not.toBe('none')
  })

  it('renders the Go-to-run anchor with the correct href', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const link = screen.getByRole('link', { name: 'Go to run' })
    expect(link.getAttribute('href')).toBe(htmlUrl)
  })

  it('sets target="_blank" on the Go-to-run anchor', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const link = screen.getByRole('link', { name: 'Go to run' })
    expect(link.getAttribute('target')).toBe('_blank')
  })

  it('sets rel="noopener noreferrer" on the Go-to-run anchor', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const link = screen.getByRole('link', { name: 'Go to run' })
    expect(link.getAttribute('rel')).toBe('noopener noreferrer')
  })

  it('anchor accessible name is "Go to run"', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    // getByRole('link', { name: '...' }) would throw if accessible name did not match.
    // Querying here just proves the element is findable by that name.
    const link = screen.getByRole('link', { name: 'Go to run' })
    expect(link).toBeTruthy()
  })

  it('clicking the close button invokes the onClose callback', async () => {
    const onClose = vi.fn()
    render(PanelActions, { props: { htmlUrl, onClose } })

    const button = screen.getByRole('button', { name: 'Close detail panel' })
    await fireEvent.click(button)

    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('close button has aria-label="Close detail panel"', () => {
    render(PanelActions, { props: { htmlUrl, onClose: () => {} } })

    const button = screen.getByRole('button', { name: 'Close detail panel' })
    expect(button.getAttribute('aria-label')).toBe('Close detail panel')
  })
})
