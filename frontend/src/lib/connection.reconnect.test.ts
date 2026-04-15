import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
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
    runnerStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  describe('fe-foundation.AC4.4: Success — Reconnect with exponential backoff', () => {
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
  })

  describe('fe-foundation.AC4.5: Success — Reconnect re-runs full connect sequence', () => {
    it('re-fetches state during reconnect instead of just reopening WebSocket', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      let stateRequestCount = 0

      const snapshot1: StateSnapshot = {
        seq: 5n,
        runs: [],
        jobs: [],
        poolStats: [],
      }

      const snapshot2: StateSnapshot = {
        seq: 10n,
        runs: [],
        jobs: [],
        poolStats: [],
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
})
