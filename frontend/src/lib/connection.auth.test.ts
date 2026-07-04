import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { createMockRun } from '$lib/test-utils/factories'

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

    it.each([
      [{ reason: 'stale_authorization' }, 'stale_authorization'],
      [{}, 'auth_required'], // malformed body falls back to the conservative reason
    ] as const)('parses 401 body %o into reason %s', async (body, expectedReason) => {
      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => HttpResponse.json(body, { status: 401 })),
      )

      await manager.connect()

      expect(connectionStore.status).toBe('unauthenticated')
      expect(connectionStore.authReason).toBe(expectedReason)

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

    it('falls back to normal backoff when the probe hangs with no response', async () => {
      const manager = new ConnectionManager(baseUrl)

      server.use(
        // Never resolves — simulates a proxy/load balancer that accepts the
        // request but never answers. Without AUTH_PROBE_TIMEOUT_MS bounding
        // the probe's fetch, this would leave the connection stuck at
        // 'connecting' forever instead of falling through to backoff.
        http.get('http://localhost:*/v1/state', () => new Promise(() => {})),
      )

      const connectPromise = manager.connect().catch(() => {})
      const ws = MockWebSocket.getLastInstance()
      ws?.close()
      await connectPromise

      await vi.waitFor(() => expect(connectionStore.status).toBe('reconnecting'), { timeout: 8000 })
      expect(connectionStore.authReason).toBe(null)

      manager.destroy()
    }, 15_000)
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

  describe('stale connect cycle vs. a fresh one', () => {
    it('a slow-to-parse 401 body does not clobber a reconnect that already succeeded', async () => {
      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      // Isolate the exact race window the fix guards: cycle A's fetch has
      // already resolved with a 401 (so it's past the abort-cancels-fetch
      // point) and is now awaiting the body parse when cycle B starts.
      const originalParseAuthReason = (
        manager as unknown as { parseAuthReason: (res: Response) => Promise<string> }
      ).parseAuthReason.bind(manager)
      const parseAuthReasonSpy = vi
        .spyOn(
          manager as unknown as { parseAuthReason: (res: Response) => Promise<string> },
          'parseAuthReason',
        )
        .mockImplementation(async (res: Response) => {
          if (res.status === 401) {
            await new Promise((resolve) => setTimeout(resolve, 30))
          }
          return originalParseAuthReason(res)
        })

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          stateRequestCount++
          if (stateRequestCount === 1) {
            return HttpResponse.json({ reason: 'auth_required' }, { status: 401 })
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

      const cycleA = manager.connect().catch(() => {})
      // Give cycle A's fetch time to resolve with the 401 and enter the
      // (now-delayed) parseAuthReason call before starting cycle B.
      await new Promise((resolve) => setTimeout(resolve, 10))
      manager.reconnect() // cycle B: aborts cycle A's signal, opens a fresh WS

      await vi.waitFor(() => expect(connectionStore.status).toBe('connected'), { timeout: 1000 })
      await cycleA // let cycle A's delayed 401 continuation finish running

      // Cycle A's aborted continuation must not have overwritten cycle B's
      // successful connect, nor closed cycle B's WebSocket out from under it.
      expect(connectionStore.status).toBe('connected')
      expect(connectionStore.authReason).toBe(null)

      parseAuthReasonSpy.mockRestore()
      manager.destroy()
    }, 10_000)
  })

  describe('entering unauthenticated clears cached run data', () => {
    it('discards previously loaded runs so a revoked repo does not stay on screen', async () => {
      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          stateRequestCount++
          if (stateRequestCount === 1) {
            return HttpResponse.json(
              snapshotToJSON({
                lastSeq: 5n,
                runs: [createMockRun()],
                jobs: [],
                runnerPoolCapacities: [],
                displayTtlSeconds: 0,
              }),
            )
          }
          return HttpResponse.json({ reason: 'stale_authorization' }, { status: 401 })
        }),
      )

      await manager.connect()
      expect(runStore.runs.size).toBe(1)

      manager.reconnect()
      await vi.waitFor(() => expect(connectionStore.status).toBe('unauthenticated'))

      expect(runStore.runs.size).toBe(0)

      manager.destroy()
    })
  })
})
