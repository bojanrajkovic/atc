import { fireEvent, render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it } from 'vitest'
import { withLocationHrefSpy } from '$lib/__tests__/location-spy'
import LoginScreen from './LoginScreen.svelte'

describe('LoginScreen', () => {
  afterEach(() => {
    window.history.replaceState(null, '', '/')
  })

  it('navigates to the login endpoint with return_to set to the current path on click', async () => {
    window.history.replaceState(null, '', '/?q=stuck-runs')
    render(LoginScreen)

    const navigatedTo = await withLocationHrefSpy(() =>
      fireEvent.click(screen.getByRole('link', { name: /sign in with github/i })),
    )

    expect(navigatedTo).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })

  it('recomputes return_to from the URL at click time, not at mount', async () => {
    window.history.replaceState(null, '', '/?q=stuck-runs')
    render(LoginScreen)

    // URL changes after mount (e.g. a stale ?run= deep link stripped while
    // this screen stayed mounted) — the click must reflect this, not the
    // URL captured when the component first rendered.
    window.history.replaceState(null, '', '/')

    const navigatedTo = await withLocationHrefSpy(() =>
      fireEvent.click(screen.getByRole('link', { name: /sign in with github/i })),
    )

    expect(navigatedTo).toBe(`/v1/auth/github/login?return_to=${encodeURIComponent('/')}`)
  })

  it('renders the ATC mark', () => {
    render(LoginScreen)
    expect(screen.getByLabelText('ATC — Actions Traffic Control')).toBeTruthy()
  })
})
