import { describe, expect, it } from 'vitest'

import { render, screen } from '@testing-library/svelte'
import PaletteCommandItem from './PaletteCommandItem.svelte'

describe('PaletteCommandItem (browser mode)', () => {
  it('renders label text inside Command context', () => {
    render(PaletteCommandItem, {
      props: {
        label: 'Copy run URL',
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    expect(screen.getByText('Copy run URL')).toBeTruthy()
  })

  it('renders icon glyph when provided', () => {
    const { container } = render(PaletteCommandItem, {
      props: {
        label: 'Copy',
        icon: '📋',
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    expect(container.querySelector('.icon')).toBeTruthy()
    expect(screen.getByText('📋')).toBeTruthy()
  })

  it('does not render icon span when icon is not provided', () => {
    const { container } = render(PaletteCommandItem, {
      props: {
        label: 'Command',
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    expect(container.querySelector('.icon')).toBeFalsy()
  })

  it('renders shortcut chips as kbd elements when provided', () => {
    const { container } = render(PaletteCommandItem, {
      props: {
        label: 'Open palette',
        shortcut: ['⌘', 'K'],
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    const kbdElements = container.querySelectorAll('kbd')
    expect(kbdElements.length).toBe(2)
    expect(kbdElements[0].textContent).toBe('⌘')
    expect(kbdElements[1].textContent).toBe('K')
  })

  it('does not render shortcut span when shortcut is not provided', () => {
    const { container } = render(PaletteCommandItem, {
      props: {
        label: 'Command',
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    expect(container.querySelector('.shortcut')).toBeFalsy()
  })

  it('renders with all properties together', () => {
    const { container } = render(PaletteCommandItem, {
      props: {
        label: 'Copy URL',
        icon: '🔗',
        shortcut: ['⌘', 'C'],
        onSelect: () => {},
      },
      context: new Map([['cmdk-root', { value: 'test', open: true }]]),
    })

    expect(screen.getByText('Copy URL')).toBeTruthy()
    expect(screen.getByText('🔗')).toBeTruthy()

    const kbdElements = container.querySelectorAll('kbd')
    expect(kbdElements.length).toBe(2)
  })
})
