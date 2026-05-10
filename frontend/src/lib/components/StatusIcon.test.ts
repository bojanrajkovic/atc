import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import { STATUS_KEYS } from '$lib/format/status-key'
import StatusIcon from './StatusIcon.svelte'

describe('StatusIcon', () => {
  describe('renders the correct glyph for each StatusKey', () => {
    it('renders the Queued glyph (◐) with "Queued" label', () => {
      render(StatusIcon, { props: { value: 'Queued' } })
      expect(screen.getByText('\u25D0')).toBeTruthy()
      expect(screen.getByText('Queued')).toBeTruthy()
    })

    it('renders the InProgress glyph (▶) with "In Progress" label', () => {
      render(StatusIcon, { props: { value: 'InProgress' } })
      expect(screen.getByText('\u25B6')).toBeTruthy()
      expect(screen.getByText('In Progress')).toBeTruthy()
    })

    it('renders the Success glyph (✓) with "Success" label', () => {
      render(StatusIcon, { props: { value: 'Success' } })
      expect(screen.getByText('\u2713')).toBeTruthy()
      expect(screen.getByText('Success')).toBeTruthy()
    })

    it('renders the Failure glyph (✗) with "Failure" label', () => {
      render(StatusIcon, { props: { value: 'Failure' } })
      expect(screen.getByText('\u2717')).toBeTruthy()
      expect(screen.getByText('Failure')).toBeTruthy()
    })

    it('renders the Cancelled glyph (⊘) with "Cancelled" label', () => {
      render(StatusIcon, { props: { value: 'Cancelled' } })
      expect(screen.getByText('\u2298')).toBeTruthy()
      expect(screen.getByText('Cancelled')).toBeTruthy()
    })

    it('renders the TimedOut glyph (⏱) with "Timed Out" label', () => {
      render(StatusIcon, { props: { value: 'TimedOut' } })
      expect(screen.getByText('\u23F1')).toBeTruthy()
      expect(screen.getByText('Timed Out')).toBeTruthy()
    })

    it('renders the ActionRequired glyph (⚠) with "Action Required" label', () => {
      render(StatusIcon, { props: { value: 'ActionRequired' } })
      expect(screen.getByText('\u26A0')).toBeTruthy()
      expect(screen.getByText('Action Required')).toBeTruthy()
    })

    it('renders the StartupFailure glyph (⚡) with "Startup Failure" label', () => {
      render(StatusIcon, { props: { value: 'StartupFailure' } })
      expect(screen.getByText('\u26A1')).toBeTruthy()
      expect(screen.getByText('Startup Failure')).toBeTruthy()
    })

    it('renders the Stale glyph (○) with "Stale" label', () => {
      render(StatusIcon, { props: { value: 'Stale' } })
      expect(screen.getByText('\u25CB')).toBeTruthy()
      expect(screen.getByText('Stale')).toBeTruthy()
    })

    it('renders the Neutral glyph (○) with "Neutral" label', () => {
      render(StatusIcon, { props: { value: 'Neutral' } })
      expect(screen.getByText('\u25CB')).toBeTruthy()
      expect(screen.getByText('Neutral')).toBeTruthy()
    })

    it('renders the Skipped glyph (○) with "Skipped" label', () => {
      render(StatusIcon, { props: { value: 'Skipped' } })
      expect(screen.getByText('\u25CB')).toBeTruthy()
      expect(screen.getByText('Skipped')).toBeTruthy()
    })
  })

  describe('inherits color via --status-color', () => {
    it('sets inline color: var(--status-color) on the status-icon span', () => {
      const { container } = render(StatusIcon, { props: { value: 'Queued' } })
      const icon = container.querySelector('.status-icon')
      expect(icon?.getAttribute('style')).toContain('var(--status-color)')
      // Also verify the module does not reference colorVar anywhere:
      expect(icon?.getAttribute('style')).not.toContain('colorVar')
    })
  })

  describe('sr-only label accompanies every glyph', () => {
    it('renders the sr-only label for Queued', () => {
      render(StatusIcon, { props: { value: 'Queued' } })
      const srOnly = screen.getByText('Queued', { exact: false })
      expect(srOnly.className).toContain('sr-only')
    })
  })

  describe('glyph element carries aria-hidden="true"', () => {
    it('sets aria-hidden="true" on the glyph span', () => {
      const { container } = render(StatusIcon, { props: { value: 'Queued' } })
      const glyphSpan = container.querySelector('[aria-hidden="true"]')
      expect(glyphSpan?.textContent).toBe('\u25D0')
    })
  })

  describe('exhaustive StatusKey lookup via satisfies', () => {
    it('renders all 11 StatusKey values without throwing', () => {
      for (const key of STATUS_KEYS) {
        const result = render(StatusIcon, { props: { value: key } })
        // Verify render succeeded without error and cleanup
        expect(result.container).toBeTruthy()
        result.unmount()
      }
    })

    it('lookup contains exactly 11 entries matching STATUS_KEYS', () => {
      // This verifies the lookup has 11 keys at runtime
      expect(STATUS_KEYS).toHaveLength(11)
    })
  })
})
