import { render, screen, waitFor } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { withLocationHrefSpy } from '$lib/__tests__/location-spy'
import { connectionStore } from '$lib/stores/connection.svelte'
import LoginScreen from './LoginScreen.svelte'

describe('LoginScreen — popup-first re-auth on stale_authorization', () => {
  afterEach(() => {
    window.history.replaceState(null, '', '/')
    connectionStore.authReason = null
    vi.restoreAllMocks()
  })

  it('does not attempt a popup for auth_required — there is no prior session to silently refresh', () => {
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(null)
    connectionStore.authReason = 'auth_required'
    render(LoginScreen)

    expect(openSpy).not.toHaveBeenCalled()
  })

  it('opens a popup and calls retry() on session-refreshed, without navigating', async () => {
    const popup = { closed: false } as unknown as Window
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(popup)
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

  it('falls back to a full-page redirect when window.open returns null (no user activation)', async () => {
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
    const popup = { closed: false } as unknown as { closed: boolean }
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    const retrySpy = vi.spyOn(connectionStore, 'retry')

    connectionStore.authReason = 'stale_authorization'
    render(LoginScreen)

    popup.closed = true
    await vi.advanceTimersByTimeAsync(600)

    expect(retrySpy).not.toHaveBeenCalled()
    // The manual control is still there — abandonment falls back to the
    // ordinary login screen, not an infinite wait.
    expect(screen.getByRole('link', { name: /sign in with github/i })).toBeTruthy()

    vi.useRealTimers()
  })

  it('opens at most one popup even if a second stale signal arrives while one is in flight', async () => {
    const popup = { closed: false } as unknown as Window
    const openSpy = vi.spyOn(window, 'open').mockReturnValue(popup)

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
})
