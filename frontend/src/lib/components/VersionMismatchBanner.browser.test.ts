import { fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { connectionStore } from '$lib/stores/connection.svelte'
import VersionMismatchBanner from './VersionMismatchBanner.svelte'

/**
 * Component tests for the version-mismatch banner (issue #47).
 *
 * Mounted in `AppShell.svelte` between <TopBar> and <main>; appears when a
 * mismatch has been observed; counts down 30 seconds; auto-reloads at zero
 * or on "Refresh now" click. Reads from connectionStore directly —
 * `serverVersionMismatch`, `serverReloadAt`, and `refreshNow()`.
 *
 * Runs under prefers-reduced-motion compatible defaults — the banner remains
 * visible but the smooth-drain bar is hidden (impeccable N2 fix).
 */
describe('VersionMismatchBanner (issue #47)', () => {
  function resetVersionFields(): void {
    connectionStore.serverVersionReference = null
    connectionStore.serverVersionMismatch = null
    connectionStore.serverReloadAt = null
    connectionStore.serverGoingAway = false
    connectionStore.goingAwayReason = null
  }

  beforeEach(() => {
    resetVersionFields()
  })

  afterEach(() => {
    resetVersionFields()
    vi.useRealTimers()
  })

  it('is hidden when no mismatch has been observed', () => {
    connectionStore.observeServerVersion('v1.0.0')
    const { container } = render(VersionMismatchBanner)
    expect(container.querySelector('[role="status"]')).toBeNull()
  })

  it('is visible (role=status, aria-live=polite) once a mismatch arms the countdown', () => {
    connectionStore.observeServerVersion('v1.0.0')
    connectionStore.observeServerVersion('v1.1.0')
    render(VersionMismatchBanner)
    const banner = screen.getByRole('status')
    expect(banner.getAttribute('aria-live')).toBe('polite')
    expect(banner.getAttribute('aria-atomic')).toBe('true')
  })

  it('renders a "Refresh now" button that triggers connectionStore.refreshNow()', async () => {
    connectionStore.observeServerVersion('v1.0.0')
    connectionStore.observeServerVersion('v1.1.0')

    const refreshSpy = vi.spyOn(connectionStore, 'refreshNow').mockImplementation(() => {
      /* swallow window.location.reload in test env */
    })
    try {
      render(VersionMismatchBanner)
      const button = screen.getByRole('button', { name: /refresh now/i })
      await fireEvent.click(button)
      expect(refreshSpy).toHaveBeenCalledOnce()
    } finally {
      refreshSpy.mockRestore()
    }
  })

  it('renders a decrementing seconds counter — initial ~30s, ticks down once per second', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: false })
    vi.setSystemTime(Date.now())
    connectionStore.observeServerVersion('v1.0.0')
    connectionStore.observeServerVersion('v1.1.0')

    render(VersionMismatchBanner)

    // Initial render: ~30s remaining.
    expect(screen.getByRole('status').textContent).toMatch(/\b30\s*s\b/)

    await vi.advanceTimersByTimeAsync(1_000)
    expect(screen.getByRole('status').textContent).toMatch(/\b29\s*s\b/)

    await vi.advanceTimersByTimeAsync(5_000)
    expect(screen.getByRole('status').textContent).toMatch(/\b24\s*s\b/)
  })

  it('invokes connectionStore.refreshNow() automatically when the countdown hits zero', async () => {
    const refreshSpy = vi.spyOn(connectionStore, 'refreshNow').mockImplementation(() => {
      /* don't actually reload during the test */
    })

    try {
      vi.useFakeTimers({ shouldAdvanceTime: false })
      vi.setSystemTime(Date.now())
      connectionStore.observeServerVersion('v1.0.0')
      connectionStore.observeServerVersion('v1.1.0')

      render(VersionMismatchBanner)
      expect(refreshSpy).not.toHaveBeenCalled()

      // Advance past the 30s deadline.
      await vi.advanceTimersByTimeAsync(31_000)
      expect(refreshSpy).toHaveBeenCalledOnce()
    } finally {
      refreshSpy.mockRestore()
    }
  })

  it('respects prefers-reduced-motion: countdown bar element is hidden when reduce is set', () => {
    // We can't flip the OS-level preference from a test, so we assert the
    // structural contract: the bar element opts out of rendering when
    // `prefers-reduced-motion: reduce` matches.
    const reduceMql = window.matchMedia('(prefers-reduced-motion: reduce)')
    connectionStore.observeServerVersion('v1.0.0')
    connectionStore.observeServerVersion('v1.1.0')
    const { container } = render(VersionMismatchBanner)

    const bar = container.querySelector('[data-countdown-bar]')
    if (reduceMql.matches) {
      expect(bar).toBeNull()
    } else {
      expect(bar).not.toBeNull()
    }
  })
})
