import { fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import { connectionStore } from '$lib/stores/connection.svelte'
import ConfigReloadErrorBanner from './ConfigReloadErrorBanner.svelte'

/**
 * Component tests for the config-reload-error admin banner (issue #203).
 *
 * Mounted in `AppShell.svelte` adjacent to <VersionMismatchBanner>; appears
 * when the server emits a `ConfigReloadError` WireFrame post-snapshot and
 * the dispatcher has called `connectionStore.markConfigReloadError(reason)`.
 * Manually dismissible; auto-dismisses 60s after the most recent mark via
 * a store-side timer (the component is a pure reactive renderer).
 *
 * Uses the same structural prefers-reduced-motion assertion shape as
 * `VersionMismatchBanner.browser.test.ts:108-123` — the global `app.css`
 * reset uses `!important` on `prefers-reduced-motion`, making property-
 * level assertions fragile. We assert presence/absence of a motion-bearing
 * data-attribute marker.
 */
describe('ConfigReloadErrorBanner (issue #203)', () => {
  beforeEach(() => {
    connectionStore.dismissConfigReloadError()
  })

  afterEach(() => {
    connectionStore.dismissConfigReloadError()
  })

  it('is hidden when configReloadError is null', () => {
    const { container } = render(ConfigReloadErrorBanner)
    expect(container.querySelector('[role="status"]')).toBeNull()
  })

  it('is visible with the reason text, role=status, aria-live=polite, aria-atomic=true', () => {
    connectionStore.markConfigReloadError('capacity must be >= 1')
    render(ConfigReloadErrorBanner)

    const banner = screen.getByRole('status')
    expect(banner.getAttribute('aria-live')).toBe('polite')
    expect(banner.getAttribute('aria-atomic')).toBe('true')
    expect(banner.getAttribute('aria-label')).toBeTruthy()
    expect(banner.textContent).toContain('capacity must be >= 1')
  })

  it('renders the failed-state ✗ glyph (color+symbol duality per .impeccable.md)', () => {
    connectionStore.markConfigReloadError('boom')
    const { container } = render(ConfigReloadErrorBanner)
    const glyph = container.querySelector('[data-banner-glyph]')
    expect(glyph).not.toBeNull()
    expect(glyph?.textContent).toBe('✗')
  })

  it('tints the surface with --failed (no side stripe)', () => {
    connectionStore.markConfigReloadError('boom')
    const { container } = render(ConfigReloadErrorBanner)
    const banner = container.querySelector('[role="status"]') as HTMLElement | null
    expect(banner).not.toBeNull()
    // Inline style — easier to assert than computed style, which the
    // browser may normalize.
    expect(banner!.getAttribute('style')).toContain('--failed')
  })

  it('clicking the Dismiss button clears configReloadError and hides the banner', async () => {
    connectionStore.markConfigReloadError('boom')
    const { container } = render(ConfigReloadErrorBanner)

    const dismiss = screen.getByRole('button', { name: /dismiss/i })
    await fireEvent.click(dismiss)

    expect(connectionStore.configReloadError).toBeNull()
    expect(container.querySelector('[role="status"]')).toBeNull()
  })

  it('respects prefers-reduced-motion: motion-bearing element is absent when reduce matches', () => {
    // Same structural assertion shape as VersionMismatchBanner.browser.test.ts:108-123.
    // The component gates entrance-animation participation behind a
    // `prefers-reduced-motion` check; assert the marker rather than
    // computed-CSS properties (the app.css `!important` reset would make
    // property assertions fragile).
    const reduceMql = window.matchMedia('(prefers-reduced-motion: reduce)')
    connectionStore.markConfigReloadError('boom')
    const { container } = render(ConfigReloadErrorBanner)

    const motion = container.querySelector('[data-banner-motion]')
    if (reduceMql.matches) {
      expect(motion).toBeNull()
    } else {
      expect(motion).not.toBeNull()
    }
  })
})
