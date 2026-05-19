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
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

describe('ConnectionManager', () => {
  const baseUrl = 'http://localhost:3000'
  const server = setupConnectionTestServer()

  beforeAll(() => {
    server.listen()
    // Replace WebSocket constructor with our mock
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
    // Clear stores between tests
    runStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  describe('Success — Reconnect with exponential backoff', () => {
    it('transitions to reconnecting and retries after backoff delay', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      manager.connect()

      // Let microtasks and timers process
      await vi.runAllTimersAsync()

      // Get the mock WebSocket and close it
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      // Process the close event
      await Promise.resolve()

      // After WebSocket close, should be in reconnecting state
      expect(connectionStore.status).toBe('reconnecting')

      manager.destroy()
      vi.useRealTimers()
    })

    it('uses exponential backoff: 1s, 2s, 4s, 8s, capped at 30s', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      const timeoutSpy = vi.spyOn(global, 'setTimeout')

      manager.connect()

      // Let microtasks and timers process
      await vi.runAllTimersAsync()

      // Get the mock WebSocket and close it
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      // Process the close event
      await Promise.resolve()

      // At this point, a reconnect timer should have been scheduled
      // with the first backoff delay (1000ms)
      const calls = timeoutSpy.mock.calls
      const lastCall = calls[calls.length - 1]
      expect(lastCall?.[1]).toBe(1000) // First reconnect: 1s

      timeoutSpy.mockRestore()
      manager.destroy()
      vi.useRealTimers()
    })

    it('exponential backoff progression: 1s → 2s → 4s → 8s → 30s cap', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      const delays: number[] = []

      const timeoutSpy = vi.spyOn(global, 'setTimeout').mockImplementation((_cb, delay) => {
        delays.push(delay as number)
        // Return a fake timer ID (cast through unknown to avoid type errors)
        return 1 as unknown as ReturnType<typeof setTimeout>
      })

      manager.connect()

      // Let initial connection setup run
      await vi.runAllTimersAsync()

      // Trigger 6 consecutive disconnections to test the full backoff sequence
      const expectedDelays = [1000, 2000, 4000, 8000, 16000, 30000]

      for (let i = 0; i < 6; i++) {
        // Get current WebSocket
        const ws = MockWebSocket.getLastInstance()
        if (ws) {
          ws.close(1000)
        }

        // Process close event
        await Promise.resolve()

        // Let reconnect timer be scheduled
        await vi.runAllTimersAsync()

        // The last setTimeout call should be the reconnect delay
        const reconnectDelays = delays.filter((_, idx) => {
          // Filter to only reconnect timeouts (skip earlier connection attempts)
          return idx >= i
        })

        if (reconnectDelays.length > 0) {
          const lastDelay = reconnectDelays[reconnectDelays.length - 1]
          expect(lastDelay).toBe(expectedDelays[i])
        }
      }

      timeoutSpy.mockRestore()
      manager.destroy()
      vi.useRealTimers()
    })
  })

  describe('Success — Reconnect re-runs full connect sequence', () => {
    it('re-fetches state during reconnect instead of just reopening WebSocket', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      const snapshot1: StateSnapshot = {
        lastSeq: 5n,
        runs: [],
        jobs: [],
        runnerPoolCapacities: [],

        accessibleReposCount: 0n,
      }

      const snapshot2: StateSnapshot = {
        lastSeq: 10n,
        runs: [],
        jobs: [],
        runnerPoolCapacities: [],

        accessibleReposCount: 0n,
      }

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          stateRequestCount++
          return HttpResponse.json(snapshotToJSON(stateRequestCount === 1 ? snapshot1 : snapshot2))
        }),
      )

      manager.connect()
      await vi.runAllTimersAsync()
      expect(stateRequestCount).toBe(1)

      // Trigger disconnect by closing the WebSocket
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      // Process the close event
      await Promise.resolve()

      // Advance time past the first reconnect backoff delay (1000ms)
      vi.advanceTimersByTime(1001)
      await vi.runAllTimersAsync()

      // Verify state was re-fetched during reconnect
      expect(stateRequestCount).toBe(2)

      manager.destroy()
      vi.useRealTimers()
    })
  })

  describe('manager.reconnect() — manual trigger from command palette', () => {
    it('cancels pending reconnect timer and resets backoff counter', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      const timeoutSpy = vi.spyOn(global, 'clearTimeout')

      // Initial connect
      manager.connect()
      await vi.runAllTimersAsync()

      // Trigger a disconnect
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      await Promise.resolve()

      // Verify reconnecting state and attempt counter advanced
      expect(connectionStore.status).toBe('reconnecting')

      // Call the public reconnect() method
      manager.reconnect()

      // Verify attempt counter was reset
      expect(connectionStore.reconnectAttempt).toBe(0)

      manager.destroy()
      timeoutSpy.mockRestore()
      vi.useRealTimers()
    })

    it('closes existing WebSocket before re-connecting', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      manager.connect()
      await vi.runAllTimersAsync()

      // Verify connected state
      expect(connectionStore.status).toBe('connected')

      // Get current WS
      const previousWs = MockWebSocket.getLastInstance()

      // Call reconnect()
      manager.reconnect()
      await vi.runAllTimersAsync()

      // After reconnect, should have created a new WebSocket instance
      const newWs = MockWebSocket.getLastInstance()

      // They should be different instances (old one closed, new one created)
      expect(newWs).not.toBe(previousWs)

      manager.destroy()
      vi.useRealTimers()
    })

    it('aborts in-flight connect handshake when reconnect is called pre-open', async () => {
      // Regression: reconnect() previously nulled this.ws.onclose then close()d
      // the WebSocket. If a prior connect() was still awaiting open, that wait
      // depended on the temporary onclose rejector — with onclose nulled and
      // (in real browsers) onopen never firing on a closed socket, the original
      // connect() Promise stranded forever, leaking the async frame and
      // accumulating with each manual reconnect during a slow handshake.
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      // Start connect() but do NOT drain microtasks — the WebSocket is in
      // CONNECTING state, the open-wait Promise is pending.
      const connectPromise = manager.connect()
      let connectError: unknown = null
      connectPromise.catch((err) => {
        connectError = err
      })

      // Synchronously force a reconnect. The original handshake's open-wait
      // must reject (rather than strand) so the orphan async frame settles.
      manager.reconnect()

      await vi.runAllTimersAsync()

      expect(connectError).not.toBeNull()
      const err = connectError as Error & { name?: string }
      expect(err.name === 'AbortError' || /abort/i.test(err.message)).toBe(true)

      manager.destroy()
      vi.useRealTimers()
    })

    it('transitions to connecting state immediately', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      manager.connect()
      await vi.runAllTimersAsync()

      // Close to trigger reconnecting
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      await Promise.resolve()
      expect(connectionStore.status).toBe('reconnecting')

      // Call reconnect()
      manager.reconnect()

      // Should transition to connecting
      expect(connectionStore.status).toBe('connecting')

      manager.destroy()
      vi.useRealTimers()
    })
  })

  describe('Max reconnect attempts — give up after cap', () => {
    it('stops scheduling reconnect timers once attempt count reaches MAX_RECONNECT_ATTEMPTS', async () => {
      const { MAX_RECONNECT_ATTEMPTS } = await import('$lib/connection')

      // Force the state fetch to fail so `connect()` never reaches the
      // success path (where `reconnectAttempt` would be reset to 0) — the
      // primed counter has to survive into `handleDisconnect`.
      server.use(
        http.get('http://localhost:*/v1/state', () => new HttpResponse(null, { status: 500 })),
      )

      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      connectionStore.reconnectAttempt = MAX_RECONNECT_ATTEMPTS

      const timeoutSpy = vi.spyOn(global, 'setTimeout')
      const callsBefore = timeoutSpy.mock.calls.length

      manager.connect().catch(() => {})
      await vi.runAllTimersAsync()

      // State fetch returned 500 → connect's catch closed the WS and called
      // handleDisconnect. With the counter at the cap, no new timer should
      // have been scheduled and the indicator should be `disconnected`.
      const reconnectCalls = timeoutSpy.mock.calls.slice(callsBefore)
      expect(reconnectCalls.length).toBe(0)
      expect(connectionStore.status).toBe('disconnected')

      timeoutSpy.mockRestore()
      manager.destroy()
      vi.useRealTimers()
    })

    it('manager.reconnect() re-arms the loop after the cap', async () => {
      const { MAX_RECONNECT_ATTEMPTS } = await import('$lib/connection')

      // Same setup as the previous test — fail state fetch so the cap trips.
      server.use(
        http.get('http://localhost:*/v1/state', () => new HttpResponse(null, { status: 500 })),
      )

      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      connectionStore.reconnectAttempt = MAX_RECONNECT_ATTEMPTS

      manager.connect().catch(() => {})
      await vi.runAllTimersAsync()

      expect(connectionStore.status).toBe('disconnected')

      // Operator clicks the indicator → store fires requestReconnect →
      // ConnectionManager.svelte calls manager.reconnect(). Here we call it
      // directly since we're outside the Svelte effect.
      manager.reconnect()

      // Counter resets and a fresh connect cycle starts.
      expect(connectionStore.reconnectAttempt).toBe(0)
      expect(connectionStore.status).toBe('connecting')

      manager.destroy()
      vi.useRealTimers()
    })
  })
})
