import { render, screen } from '@testing-library/svelte'
import { afterEach, describe, expect, it } from 'vitest'
import LoginScreen from './LoginScreen.svelte'

describe('LoginScreen', () => {
  afterEach(() => {
    window.history.replaceState(null, '', '/')
  })

  it('renders a sign-in link to the login endpoint with return_to set to the current path', () => {
    window.history.replaceState(null, '', '/?q=stuck-runs')
    render(LoginScreen)

    const link = screen.getByRole('link', { name: /sign in with github/i })
    expect(link.getAttribute('href')).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/?q=stuck-runs')}`,
    )
  })

  it('defaults return_to to the bare path when there is no query string', () => {
    window.history.replaceState(null, '', '/')
    render(LoginScreen)

    const link = screen.getByRole('link', { name: /sign in with github/i })
    expect(link.getAttribute('href')).toBe(
      `/v1/auth/github/login?return_to=${encodeURIComponent('/')}`,
    )
  })

  it('renders the ATC mark', () => {
    render(LoginScreen)
    expect(screen.getByLabelText('ATC — Actions Traffic Control')).toBeTruthy()
  })
})
