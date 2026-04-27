import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'

import Wrapper from './test-utils/PaletteCommandItemWrapper.svelte'

describe('PaletteCommandItem (browser mode)', () => {
  it('renders label text inside Command context', () => {
    render(Wrapper, {
      props: {
        label: 'Copy run URL',
        onSelect: () => {},
      },
    })

    expect(screen.getByText('Copy run URL')).toBeTruthy()
  })

  it('renders icon glyph when provided', () => {
    const { container } = render(Wrapper, {
      props: {
        label: 'Copy',
        icon: '📋',
        onSelect: () => {},
      },
    })

    expect(container.querySelector('.icon')).toBeTruthy()
    expect(screen.getByText('📋')).toBeTruthy()
  })

  it('does not render icon span when icon is not provided', () => {
    const { container } = render(Wrapper, {
      props: {
        label: 'Command',
        onSelect: () => {},
      },
    })

    expect(container.querySelector('.icon')).toBeFalsy()
  })

  it('renders shortcut chips as kbd elements when provided', () => {
    const { container } = render(Wrapper, {
      props: {
        label: 'Open palette',
        shortcut: ['⌘', 'K'],
        onSelect: () => {},
      },
    })

    const kbdElements = container.querySelectorAll('kbd')
    expect(kbdElements.length).toBe(2)
    expect(kbdElements[0]?.textContent).toBe('⌘')
    expect(kbdElements[1]?.textContent).toBe('K')
  })

  it('does not render shortcut span when shortcut is not provided', () => {
    const { container } = render(Wrapper, {
      props: {
        label: 'Command',
        onSelect: () => {},
      },
    })

    expect(container.querySelector('.shortcut')).toBeFalsy()
  })

  it('renders with all properties together', () => {
    const { container } = render(Wrapper, {
      props: {
        label: 'Copy URL',
        icon: '🔗',
        shortcut: ['⌘', 'C'],
        onSelect: () => {},
      },
    })

    expect(screen.getByText('Copy URL')).toBeTruthy()
    expect(screen.getByText('🔗')).toBeTruthy()

    const kbdElements = container.querySelectorAll('kbd')
    expect(kbdElements.length).toBe(2)
  })
})
