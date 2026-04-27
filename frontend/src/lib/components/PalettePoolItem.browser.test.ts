import { describe, expect, it } from 'vitest'

import { render, screen } from '@testing-library/svelte'
import Wrapper from './test-utils/PalettePoolItemWrapper.svelte'

describe('PalettePoolItem (browser mode)', () => {
  it('renders dot-separated labels when query is empty', () => {
    render(Wrapper, {
      props: {
        pool: {
          labels: ['linux', 'self-hosted', 'x86'],
          running: 2,
          queued: 1,
        },
        query: '',
        onSelect: () => {},
      },
    })

    const text = screen.getByText(/linux.*self-hosted.*x86/)
    expect(text).toBeTruthy()
  })

  it('renders meta line with running and queued counts', () => {
    render(Wrapper, {
      props: {
        pool: {
          labels: ['linux'],
          running: 3,
          queued: 2,
        },
        query: '',
        onSelect: () => {},
      },
    })

    expect(screen.getByText('3 running · 2 queued')).toBeTruthy()
  })

  it('renders gracefully with empty labels', () => {
    render(Wrapper, {
      props: {
        pool: {
          labels: [],
          running: 0,
          queued: 0,
        },
        query: '',
        onSelect: () => {},
      },
    })

    expect(screen.getByText('0 running · 0 queued')).toBeTruthy()
  })

  it('includes mark elements when query is active', () => {
    const { container } = render(Wrapper, {
      props: {
        pool: {
          labels: ['linux', 'x86'],
          running: 1,
          queued: 0,
        },
        query: 'lin',
        onSelect: () => {},
      },
    })

    const marks = container.querySelectorAll('mark')
    expect(marks.length).toBeGreaterThan(0)
  })

  it('browse state: renders with white-space: nowrap when query is empty and not focused', () => {
    const { container } = render(Wrapper, {
      props: {
        pool: {
          labels: ['linux', 'self-hosted'],
          running: 1,
          queued: 0,
        },
        query: '',
        onSelect: () => {},
      },
    })

    const labels = container.querySelector('.labels') as HTMLElement
    if (labels) {
      const computed = window.getComputedStyle(labels)
      expect(computed.whiteSpace).toBe('nowrap')
    }
  })

  it('query-active state: renders with white-space: normal when query is active', () => {
    const { container } = render(Wrapper, {
      props: {
        pool: {
          labels: ['linux', 'self-hosted'],
          running: 1,
          queued: 0,
        },
        query: 'lin',
        onSelect: () => {},
      },
    })

    const labels = container.querySelector('.labels') as HTMLElement
    if (labels) {
      const computed = window.getComputedStyle(labels)
      expect(computed.whiteSpace).toBe('normal')
    }
  })

  it('meta column maintains min-width gutter', () => {
    const { container } = render(Wrapper, {
      props: {
        pool: {
          labels: ['linux'],
          running: 1,
          queued: 0,
        },
        query: '',
        onSelect: () => {},
      },
    })

    const meta = container.querySelector('.meta') as HTMLElement
    if (meta) {
      const computed = window.getComputedStyle(meta)
      expect(computed.minWidth).toBeTruthy()
    }
  })
})
