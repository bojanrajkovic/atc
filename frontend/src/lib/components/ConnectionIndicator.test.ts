import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import ConnectionIndicator from './ConnectionIndicator.svelte'

describe('ConnectionIndicator', () => {
  describe('role="status" and aria-label', () => {
    it('renders with role="status" and aria-label for live state', () => {
      render(ConnectionIndicator, {
        props: {
          state: 'live',
          detail: 'Connected and receiving events',
        },
      })

      const status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('Connected and receiving events')
    })

    it('renders with role="status" and aria-label for stale state', () => {
      render(ConnectionIndicator, {
        props: {
          state: 'stale',
          detail: 'No events for 45s',
        },
      })

      const status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('No events for 45s')
    })

    it('renders with role="status" and aria-label for connecting state', () => {
      render(ConnectionIndicator, {
        props: {
          state: 'connecting',
          detail: 'Reconnecting...',
        },
      })

      const status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('Reconnecting...')
    })

    it('renders with role="status" and aria-label for disconnected state', () => {
      render(ConnectionIndicator, {
        props: {
          state: 'disconnected',
          detail: 'Disconnected',
        },
      })

      const status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('Disconnected')
    })
  })

  describe('live state', () => {
    it('uses success color with box-shadow glow', () => {
      const { container } = render(ConnectionIndicator, {
        props: {
          state: 'live',
          detail: 'Connected',
        },
      })

      // Find the inner dot (the relative span with rounded-full class)
      const dots = container.querySelectorAll('span.relative.inline-flex.h-3.w-3.rounded-full')
      expect(dots.length).toBeGreaterThan(0)

      const innerDot = dots[0]
      if (!innerDot) {
        throw new Error('Inner dot not found')
      }

      const style = innerDot.getAttribute('style')
      expect(style).toContain('--success')
      expect(style).toContain('box-shadow')
    })
  })

  describe('stale state', () => {
    it('uses running color without glow', () => {
      const { container } = render(ConnectionIndicator, {
        props: {
          state: 'stale',
          detail: 'No events for 45s',
        },
      })

      const dots = container.querySelectorAll('span.relative.inline-flex.h-3.w-3.rounded-full')
      const innerDot = dots[0]
      if (!innerDot) {
        throw new Error('Inner dot not found')
      }

      const style = innerDot.getAttribute('style')
      expect(style).toContain('--running')
      expect(style).not.toContain('box-shadow')
    })
  })

  describe('connecting state', () => {
    it('uses queued color with animate-ping element', () => {
      const { container } = render(ConnectionIndicator, {
        props: {
          state: 'connecting',
          detail: 'Reconnecting...',
        },
      })

      // Check for animate-ping element
      const pingElement = container.querySelector('span.animate-ping')
      expect(pingElement).toBeTruthy()
      expect(pingElement?.className).toContain('animate-ping')

      const style = pingElement?.getAttribute('style')
      expect(style).toContain('--queued')

      // Also verify inner dot has queued color
      const dots = container.querySelectorAll('span.relative.inline-flex.h-3.w-3.rounded-full')
      const innerDot = dots[0]
      if (!innerDot) {
        throw new Error('Inner dot not found')
      }

      const innerStyle = innerDot.getAttribute('style')
      expect(innerStyle).toContain('--queued')
    })
  })

  describe('disconnected state', () => {
    it('uses failed color', () => {
      const { container } = render(ConnectionIndicator, {
        props: {
          state: 'disconnected',
          detail: 'Disconnected',
        },
      })

      const dots = container.querySelectorAll('span.relative.inline-flex.h-3.w-3.rounded-full')
      const innerDot = dots[0]
      if (!innerDot) {
        throw new Error('Inner dot not found')
      }

      const style = innerDot.getAttribute('style')
      expect(style).toContain('--failed')
    })
  })

  describe('dynamic updates', () => {
    it('updates aria-label when detail prop changes', () => {
      const { unmount } = render(ConnectionIndicator, {
        props: {
          state: 'live',
          detail: 'Original detail',
        },
      })

      let status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('Original detail')

      // Unmount and remount with new props
      unmount()

      render(ConnectionIndicator, {
        props: {
          state: 'live',
          detail: 'Updated detail',
        },
      })

      status = screen.getByRole('status')
      expect(status.getAttribute('aria-label')).toBe('Updated detail')
    })
  })
})
