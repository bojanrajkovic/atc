import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import ProgressBar from './ProgressBar.svelte'

describe('ProgressBar', () => {
  describe('renders with correct ARIA attributes and scaleX transform', () => {
    it('renders progressbar with correct aria-valuenow, aria-valuemin, aria-valuemax', () => {
      render(ProgressBar, { props: { completed: 3, total: 5 } })
      const progressbar = screen.getByRole('progressbar')
      expect(progressbar?.getAttribute('aria-valuenow')).toBe('3')
      expect(progressbar?.getAttribute('aria-valuemin')).toBe('0')
      expect(progressbar?.getAttribute('aria-valuemax')).toBe('5')
    })

    it('renders fill with correct scaleX(0.6) for 3/5 ratio', () => {
      const { container } = render(ProgressBar, { props: { completed: 3, total: 5 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(0.6)')
    })

    it('applies transform-origin 0 50% for left-edge fill growth', () => {
      const { container } = render(ProgressBar, { props: { completed: 3, total: 5 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('transform-origin: 0 50%')
    })
  })

  describe('renders correct label', () => {
    it('renders label "Jobs 3 of 5" for completed/total values', () => {
      render(ProgressBar, { props: { completed: 3, total: 5 } })
      expect(screen.getByText('Jobs 3 of 5')).toBeTruthy()
    })
  })

  describe('handles empty state without crash', () => {
    it('renders without crashing when total === 0', () => {
      render(ProgressBar, { props: { completed: 0, total: 0 } })
      const progressbar = screen.getByRole('progressbar', {})
      expect(progressbar).toBeTruthy()
    })

    it('renders scaleX(0) when total === 0', () => {
      const { container } = render(ProgressBar, { props: { completed: 0, total: 0 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(0)')
    })

    it('renders label "Jobs 0 of 0" when empty', () => {
      render(ProgressBar, { props: { completed: 0, total: 0 } })
      expect(screen.getByText('Jobs 0 of 0')).toBeTruthy()
    })

    it('renders aria-valuetext="No jobs" when total === 0', () => {
      render(ProgressBar, { props: { completed: 0, total: 0 } })
      const progressbar = screen.getByRole('progressbar')
      expect(progressbar?.getAttribute('aria-valuetext')).toBe('No jobs')
    })
  })

  describe('clamps fill to prevent overflow', () => {
    it('renders scaleX(1) when completed === total', () => {
      const { container } = render(ProgressBar, { props: { completed: 5, total: 5 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(1)')
    })

    it('track has overflow-hidden class to prevent visual overflow', () => {
      const { container } = render(ProgressBar, { props: { completed: 5, total: 5 } })
      const track = container.querySelector('.progress-track')
      expect(track?.className).toContain('overflow-hidden')
    })
  })

  describe('boundary values render correctly', () => {
    it('renders scaleX(0) when completed is 0 of 10', () => {
      const { container } = render(ProgressBar, { props: { completed: 0, total: 10 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(0)')
    })

    it('renders scaleX(0.4) when completed is 4 of 10', () => {
      const { container } = render(ProgressBar, { props: { completed: 4, total: 10 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(0.4)')
    })

    it('renders scaleX(1) when completed is 10 of 10', () => {
      const { container } = render(ProgressBar, { props: { completed: 10, total: 10 } })
      const fill = container.querySelector('.progress-fill')
      expect(fill?.getAttribute('style')).toContain('scaleX(1)')
    })
  })

  describe('custom label support', () => {
    it('uses default label when label prop not provided', () => {
      render(ProgressBar, { props: { completed: 2, total: 8 } })
      expect(screen.getByText('Jobs 2 of 8')).toBeTruthy()
    })

    it('uses custom label when label prop is provided', () => {
      render(ProgressBar, { props: { completed: 2, total: 8, label: 'Custom Progress' } })
      expect(screen.getByText('Custom Progress')).toBeTruthy()
    })
  })

  describe('progressbar role and required attributes', () => {
    it('uses role="progressbar" not role="meter"', () => {
      const { container } = render(ProgressBar, { props: { completed: 1, total: 5 } })
      const progressbar = container.querySelector('[role="progressbar"]')
      expect(progressbar).toBeTruthy()
    })

    it('does not have aria-valuetext when total > 0', () => {
      render(ProgressBar, { props: { completed: 3, total: 5 } })
      const progressbar = screen.getByRole('progressbar', {})
      expect(progressbar?.getAttribute('aria-valuetext')).toBeNull()
    })
  })
})
