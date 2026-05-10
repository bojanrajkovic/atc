import { render } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import AppShell from './AppShell.svelte'

describe('AppShell', () => {
  it('renders TopBar and a <main> slot wrapper', () => {
    const { container } = render(AppShell)
    // TopBar is the only structural prop AppShell guarantees beyond the
    // outer flex column. Identify it via the ATC logo it always renders.
    expect(container.querySelector('header')).not.toBeNull()
    expect(container.querySelector('main')).not.toBeNull()
  })

  it('outer wrapper sets `100dvh` viewport height and the bg-token background', () => {
    const { container } = render(AppShell)
    const wrapper = container.firstElementChild as HTMLElement
    expect(wrapper).not.toBeNull()
    expect(wrapper.className).toContain('h-dvh')
    expect(wrapper.className).toContain('flex-col')
    expect(wrapper.style.backgroundColor).toBe('var(--bg)')
  })

  it('main is a flex-1 overflow-auto scroll region', () => {
    const { container } = render(AppShell)
    const main = container.querySelector('main') as HTMLElement
    expect(main.className).toContain('flex-1')
    expect(main.className).toContain('overflow-auto')
  })
})
