import { fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { withLocationHrefSpy } from '$lib/__tests__/location-spy'
import { connectionStore } from '$lib/stores/connection.svelte'
import LoginScreen from './LoginScreen.svelte'

// A minimal mutable stand-in for the Window returned by window.open — tests
// flip `.closed` to simulate the user abandoning the popup, and `cleanup()`
// calls `.close()` on it directly.
function mockPopup(): { closed: boolean; close: () => void } {
  const popup = {
    closed: false,
    close: () => {
      popup.closed = true
    },
  }
  return popup
}

describe('LoginScreen — popup-first re-auth', () => {
  afterEach(() => {
    window.history.replaceState(null, '', '/')
    connectionStore.authReason = null
    vi.restoreAllMocks()
  })

  it('attempts a popup for auth_required too — an active SPA session getting revoked may still have live user activation', () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(mockPopup() as unknown as Window)
    connectionStore.authReason = 'auth_required'
    render(LoginScreen)

    expect(openSpy).toHaveBeenCalledWith(
      '/v1/auth/github/login?popup=1',
      'atc-auth',
      'popup,width=640,height=760',
    )
  })

  it('does not auto-redirect for auth_required when window.open is blocked — a cold visitor should never be silently navigated to a GitHub consent screen', async () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(null)
    window.history.replaceState(null, '', '/?q=stuck-runs')

    connectionStore.authReason = 'auth_required'
    const navigatedTo = await withLocationHrefSpy(() => {
      render(LoginScreen)
    })

    expect(openSpy).toHaveBeenCalledOnce()
    expect(navigatedTo).toBe(null)
    expect(screen.getByRole('link', { name: /sign in with github/i })).toBeTruthy()
  })

  it('opens a popup and calls retry() on session-refreshed, without navigating', async () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(mockPopup() as unknown as Window)
    const retrySpy = vi.spyOn(connectionStore, 'retry')

    connectionStore.authReason = 'stale_authorization'
    render(LoginScreen)

    expect(openSpy).toHaveBeenCalledWith(
      '/v1/auth/github/login?popup=1',
      'atc-auth',
      'popup,width=640,height=760',
    )

    const navigatedTo = await withLocationHrefSpy(async () => {
      const channel = new BroadcastChannel('atc-auth')
      channel.postMessage('session-refreshed')
      channel.close()
      await waitFor(() => expect(retrySpy).toHaveBeenCalledOnce())
    })

    // The dashboard never navigated — only the popup did its own thing.
    expect(navigatedTo).toBe(null)
  })

  it('falls back to a full-page redirect for stale_authorization when window.open returns null (no user activation)', async () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(null)
    window.history.replaceState(null, '', '/?q=stuck-runs')

    connectionStore.authReason = 'stale_authorization'
    const navigatedTo = await withLocationHrefSpy(() => {
      render(LoginScreen)
    })

    expect(openSpy).toHaveBeenCalledOnce()
    expect(navigatedTo).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })

  it('degrades to the login screen (no retry, no hang) when the popup is closed without a message', async () => {
    vi.useFakeTimers()
    const popup = mockPopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    const retrySpy = vi.spyOn(connectionStore, 'retry')

    connectionStore.authReason = 'stale_authorization'
    render(LoginScreen)

    popup.closed = true
    // The poll notices at ~500ms and starts a 1s grace window (for a message
    // that might already be in flight) before actually tearing down.
    await vi.advanceTimersByTimeAsync(1600)

    expect(retrySpy).not.toHaveBeenCalled()
    // The manual control is still there — abandonment falls back to the
    // ordinary login screen, not an infinite wait.
    expect(screen.getByRole('link', { name: /sign in with github/i })).toBeTruthy()

    vi.useRealTimers()
  })

  it('opens at most one popup even if a second stale signal arrives while one is in flight', async () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(mockPopup() as unknown as Window)

    connectionStore.enterUnauthenticated('stale_authorization')
    render(LoginScreen)
    await tick()
    expect(openSpy).toHaveBeenCalledOnce()

    // A second, independent code path (e.g. the WS-probe path alongside the
    // state-fetch path) observes the same staleness and signals it the same
    // way. The store write is a no-op for an unchanged value, so this must
    // not re-open the popup.
    connectionStore.enterUnauthenticated('stale_authorization')
    await tick()

    expect(openSpy).toHaveBeenCalledOnce()
  })

  it('still calls retry() if session-refreshed arrives just after the poll notices the popup closed', async () => {
    vi.useFakeTimers()
    const popup = mockPopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    const retrySpy = vi.spyOn(connectionStore, 'retry')

    connectionStore.authReason = 'stale_authorization'
    render(LoginScreen)

    // The popup closes itself right as the poll checks (the realistic race:
    // window.close() flips .closed just ahead of the async
    // BroadcastChannel delivery of the message the same script already sent).
    popup.closed = true
    await vi.advanceTimersByTimeAsync(500)

    const channel = new BroadcastChannel('atc-auth')
    channel.postMessage('session-refreshed')
    channel.close()
    await vi.waitFor(() => expect(retrySpy).toHaveBeenCalledOnce())

    vi.useRealTimers()
  })

  it('never disables the manual link, even while a popup is in flight', async () => {
    vi.spyOn(window, 'open').mockReturnValue(mockPopup() as unknown as Window)

    connectionStore.authReason = 'auth_required'
    render(LoginScreen)
    await tick()

    const link = screen.getByRole('link', { name: /sign in with github/i })
    expect(link.getAttribute('aria-disabled')).not.toBe('true')
    expect(link.getAttribute('tabindex')).not.toBe('-1')
  })

  it('a manual click cancels an in-flight popup (closing it) rather than racing it for the single-slot flow cookie', async () => {
    const popup = mockPopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    window.history.replaceState(null, '', '/?q=stuck-runs')

    connectionStore.authReason = 'auth_required'
    render(LoginScreen)
    await tick()

    const link = screen.getByRole('link', { name: /sign in with github/i })
    const navigatedTo = await withLocationHrefSpy(() => fireEvent.click(link))

    expect(popup.closed).toBe(true)
    expect(navigatedTo).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })

  it('a manual click before any popup attempt (or after one already resolved) is a harmless no-op cancel', async () => {
    vi.spyOn(window, 'open').mockReturnValue(null)
    window.history.replaceState(null, '', '/?q=stuck-runs')

    connectionStore.authReason = 'auth_required'
    render(LoginScreen)
    await tick()

    const link = screen.getByRole('link', { name: /sign in with github/i })
    const navigatedTo = await withLocationHrefSpy(() => fireEvent.click(link))

    expect(navigatedTo).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })

  it('falls back to a full-page redirect if window.open throws synchronously', async () => {
    vi.spyOn(window, 'open').mockImplementation(() => {
      throw new Error('blocked by sandboxed iframe')
    })
    window.history.replaceState(null, '', '/?q=stuck-runs')

    connectionStore.authReason = 'stale_authorization'
    const navigatedTo = await withLocationHrefSpy(() => {
      render(LoginScreen)
    })

    expect(navigatedTo).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })
})
