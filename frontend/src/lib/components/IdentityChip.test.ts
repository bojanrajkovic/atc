import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import { delay, HttpResponse, http } from 'msw'
import { setupServer } from 'msw/node'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import { withLocationHrefSpy } from '$lib/__tests__/location-spy'
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

  it('does not resurrect identity if the session goes unauthenticated while the fetch is in flight', async () => {
    server.use(
      http.get('/v1/auth/me', async () => {
        await delay(20)
        return HttpResponse.json(testIdentity)
      }),
    )
    render(IdentityChip)

    // Simulate a 401 elsewhere (e.g. a reconnect's /v1/state) resolving
    // before this component's own in-flight fetch does.
    connectionStore.status = 'unauthenticated'
    connectionStore.identity = null

    await new Promise((resolve) => setTimeout(resolve, 40))

    expect(connectionStore.identity).toBe(null)
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

    const navigatedTo = await withLocationHrefSpy(async () => {
      render(IdentityChip)
      await waitFor(() => screen.getByRole('button', { name: /log out/i }))
      await fireEvent.click(screen.getByRole('button', { name: /log out/i }))
      // Wait for the post-fetch navigation, not just the fetch itself —
      // window.location.href = '/' runs after the awaited POST resolves.
      await waitFor(() => expect(window.location.href).toBe('/'))
    })

    expect(logoutCalled).toBe(true)
    expect(navigatedTo).toBe('/')
  })
})
