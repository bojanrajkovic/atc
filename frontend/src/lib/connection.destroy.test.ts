import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { MockWebSocket, setupConnectionTestServer } from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'

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

  describe('fe-foundation.AC4.6: Success — destroy() closes WebSocket and clears timers', () => {
    it('closes WebSocket when destroy is called', async () => {
      const manager = new ConnectionManager(baseUrl)

      manager.connect()
      await new Promise((resolve) => setTimeout(resolve, 10))

      expect(connectionStore.status).toBe('connected')

      manager.destroy()

      expect(connectionStore.status).toBe('disconnected')
    })

    it('clears reconnect timer when destroy is called', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)
      const clearTimeoutSpy = vi.spyOn(global, 'clearTimeout')

      manager.connect()
      // Advance timers to let the connection complete
      await vi.runAllTimersAsync()

      // Get the mock WebSocket and close it
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      // Flush microtasks to let the close event handler run
      await Promise.resolve()

      // Advance time a bit for the reconnect timer to be scheduled
      vi.advanceTimersByTime(10)

      // A reconnect timer should have been scheduled
      manager.destroy()

      // Verify clearTimeout was called
      expect(clearTimeoutSpy).toHaveBeenCalled()

      clearTimeoutSpy.mockRestore()
      vi.useRealTimers()
    })

    it('prevents reconnect from firing after destroy', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      manager.connect()
      // Advance timers to let the connection complete
      await vi.runAllTimersAsync()

      // Get the mock WebSocket and close it
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        ws.close(1000)
      }

      // Advance time a bit for the close event to process
      vi.advanceTimersByTime(10)

      manager.destroy()

      // Advance timers past when the reconnect would have fired
      vi.advanceTimersByTime(5000)

      // Status should still be disconnected (not reconnecting)
      expect(connectionStore.status).toBe('disconnected')

      vi.useRealTimers()
    })
  })
})
