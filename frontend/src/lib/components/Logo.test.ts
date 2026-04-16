import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import Logo from './Logo.svelte'

describe('Logo', () => {
  it('renders ATC text', () => {
    render(Logo)
    const element = screen.getByText('ATC')
    expect(element).toBeTruthy()
    expect(element.textContent).toBe('ATC')
  })

  it('has monospace font class', () => {
    render(Logo)
    const element = screen.getByText('ATC')
    expect(element.className).toContain('font-mono')
  })

  it('has aria-label with full name', () => {
    render(Logo)
    const element = screen.getByLabelText(/actions traffic control/i)
    expect(element).toBeTruthy()
    expect(element.getAttribute('aria-label')).toMatch(/actions traffic control/i)
  })
})
