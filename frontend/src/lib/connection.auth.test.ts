import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { MockWebSocket, setupConnectionTestServer } from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'

describe('ConnectionManager — 401-aware connection states', () => {
  const baseUrl = 'http://localhost:3000'
  const server = setupConnectionTestServer()

  beforeAll(() => {
    server.listen()
    Object.defineProperty(globalThis, 'WebSocket', {
      value: MockWebSocket,
      writable: true,
      configurable: true,
    })
  })

  beforeEach(() => {
    MockWebSocket.clearAll()
  })

  afterEach(() => {
    server.resetHandlers()
    runStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    connectionStore.authReason = null
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  describe('state-fetch 401', () => {
    it('transitions to unauthenticated with auth_required and makes no further attempts', async () => {
      vi.useFakeTimers()
      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({ reason: 'auth_required' }, { status: 401 })
        }),
      )

      await manager.connect()

      expect(connectionStore.status).toBe('unauthenticated')
      expect(connectionStore.authReason).toBe('auth_required')
      // No backoff scheduled — a pending reconnect timer would still be
      // sitting behind fake timers.
      expect(vi.getTimerCount()).toBe(0)

      manager.destroy()
      vi.useRealTimers()
    })

    it('transitions to unauthenticated with stale_authorization', async () => {
      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({ reason: 'stale_authorization' }, { status: 401 })
        }),
      )

      await manager.connect()

      expect(connectionStore.status).toBe('unauthenticated')
      expect(connectionStore.authReason).toBe('stale_authorization')

      manager.destroy()
    })

    it('falls back to auth_required on a malformed 401 body', async () => {
      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({}, { status: 401 })
        }),
      )

      await manager.connect()

      expect(connectionStore.status).toBe('unauthenticated')
      expect(connectionStore.authReason).toBe('auth_required')

      manager.destroy()
    })
  })

  describe('WS-open failure — auth probe', () => {
    it('probes /v1/state and enters unauthenticated when the probe 401s', async () => {
      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          stateRequestCount++
          return HttpResponse.json({ reason: 'auth_required' }, { status: 401 })
        }),
      )

      const connectPromise = manager.connect().catch(() => {})
      // Close the socket before it opens (readyState still CONNECTING).
      const ws = MockWebSocket.getLastInstance()
      ws?.close()
      await connectPromise
      await vi.waitFor(() => expect(connectionStore.status).toBe('unauthenticated'))

      expect(connectionStore.authReason).toBe('auth_required')
      expect(stateRequestCount).toBe(1)

      manager.destroy()
    })

    it('falls back to normal backoff when the probe is not a 401', async () => {
      vi.useFakeTimers()
      const manager = new ConnectionManager(baseUrl)

      const connectPromise = manager.connect().catch(() => {})
      const ws = MockWebSocket.getLastInstance()
      ws?.close()
      await connectPromise
      await vi.waitFor(() => expect(connectionStore.status).toBe('reconnecting'))

      expect(connectionStore.authReason).toBe(null)

      manager.destroy()
      vi.useRealTimers()
    })
  })

  describe('non-401 failures — regression', () => {
    it('a 500 on state fetch still backs off, not unauthenticated', async () => {
      vi.useFakeTimers()
      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({ error: 'Server error' }, { status: 500 })
        }),
      )

      await manager.connect()

      expect(connectionStore.status).toBe('reconnecting')
      expect(connectionStore.authReason).toBe(null)

      manager.destroy()
      vi.useRealTimers()
    })
  })

  describe('recovery via retry()', () => {
    it('manager.reconnect() after unauthenticated re-runs the full connect sequence', async () => {
      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          stateRequestCount++
          if (stateRequestCount === 1) {
            return HttpResponse.json({ reason: 'stale_authorization' }, { status: 401 })
          }
          return HttpResponse.json({
            lastSeq: 0,
            runs: [],
            jobs: [],
            runnerPoolCapacities: [],
            displayTtlSeconds: 0,
          })
        }),
      )

      await manager.connect()
      expect(connectionStore.status).toBe('unauthenticated')

      // connectionStore.retry() clears the reason and bumps
      // reconnectRequested; ConnectionManager.svelte's effect (browser-mode
      // tested separately) is what turns that counter into this call.
      connectionStore.retry()
      expect(connectionStore.authReason).toBe(null)
      manager.reconnect()
      await vi.waitFor(() => expect(connectionStore.status).toBe('connected'))

      expect(stateRequestCount).toBe(2)

      manager.destroy()
    })
  })
})
