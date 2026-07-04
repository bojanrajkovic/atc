import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { HttpResponse, http } from 'msw'
import { setupServer } from 'msw/node'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { connectionStore } from '$lib/stores/connection.svelte'
import IdentityChip from './IdentityChip.svelte'

const testIdentity = {
  login: 'octocat',
  repoCount: 3,
  reposRefreshedAt: '2026-07-04T00:00:00Z',
  stale: false,
}

describe('IdentityChip', () => {
  const server = setupServer()

  beforeAll(() => server.listen())
  afterEach(() => {
    cleanup()
    server.resetHandlers()
    connectionStore.status = 'disconnected'
    connectionStore.identity = null
  })
  afterAll(() => server.close())

  beforeEach(() => {
    connectionStore.status = 'connected'
  })

  it('renders nothing before the probe resolves', () => {
    server.use(http.get('/v1/auth/me', () => new Promise(() => {})))
    render(IdentityChip)
    expect(screen.queryByText(/log out/i)).toBeNull()
  })

  it('shows the login name and a logout control on a 200 response', async () => {
    server.use(http.get('/v1/auth/me', () => HttpResponse.json(testIdentity)))
    render(IdentityChip)

    await waitFor(() => screen.getByText('octocat'))
    expect(screen.getByRole('button', { name: /log out/i })).toBeTruthy()
  })

  it('renders nothing on a 401 (session raced ahead of the probe)', async () => {
    server.use(
      http.get('/v1/auth/me', () =>
        HttpResponse.json({ reason: 'auth_required' }, { status: 401 }),
      ),
    )
    render(IdentityChip)

    await waitFor(() => expect(connectionStore.identity).toBe(null))
    expect(screen.queryByText(/log out/i)).toBeNull()
  })

  it('renders nothing on a 404 (mode=none — the endpoint is not mounted)', async () => {
    server.use(http.get('/v1/auth/me', () => new HttpResponse(null, { status: 404 })))
    render(IdentityChip)

    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(connectionStore.identity).toBe(null)
    expect(screen.queryByText(/log out/i)).toBeNull()
  })

  it('does not re-probe on a later reconnect (one-shot)', async () => {
    let requestCount = 0
    server.use(
      http.get('/v1/auth/me', () => {
        requestCount++
        return HttpResponse.json(testIdentity)
      }),
    )
    render(IdentityChip)
    await waitFor(() => screen.getByText('octocat'))
    expect(requestCount).toBe(1)

    connectionStore.status = 'reconnecting'
    connectionStore.status = 'connected'
    await new Promise((resolve) => setTimeout(resolve, 10))

    expect(requestCount).toBe(1)
  })

  it('logout posts to the logout endpoint then hard-reloads to /', async () => {
    server.use(http.get('/v1/auth/me', () => HttpResponse.json(testIdentity)))
    let logoutCalled = false
    server.use(
      http.post('/v1/auth/github/logout', () => {
        logoutCalled = true
        return new HttpResponse(null, { status: 204 })
      }),
    )

    // Observe the href write via a getter/setter pair — a setter alone
    // shadows the real href with `undefined`, which breaks the identity
    // fetch's relative /v1/auth/me URL resolution.
    let hrefSet: string | null = null
    const originalLocation = window.location
    Object.defineProperty(window, 'location', {
      configurable: true,
      value: {
        ...originalLocation,
        get href() {
          return hrefSet ?? originalLocation.href
        },
        set href(v: string) {
          hrefSet = v
        },
      },
    })

    try {
      render(IdentityChip)
      await waitFor(() => screen.getByRole('button', { name: /log out/i }))
      await fireEvent.click(screen.getByRole('button', { name: /log out/i }))

      await waitFor(() => expect(logoutCalled).toBe(true))
      await vi.waitFor(() => expect(hrefSet).toBe('/'))
    } finally {
      Object.defineProperty(window, 'location', { configurable: true, value: originalLocation })
    }
  })
})
