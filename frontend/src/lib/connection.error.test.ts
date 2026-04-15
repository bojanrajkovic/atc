import { HttpResponse, http } from 'msw'
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

  describe('fe-foundation.AC4.7: Failure — State fetch failure triggers reconnect', () => {
    it('transitions to reconnecting on state fetch 500 error', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({ error: 'Server error' }, { status: 500 })
        }),
      )

      await manager.connect()

      // After state fetch fails, should transition to reconnecting, not stay in connecting
      expect(connectionStore.status).toBe('reconnecting')
      expect(connectionStore.reconnectAttempt).toBeGreaterThan(0)

      manager.destroy()
      vi.useRealTimers()
    })

    it('does not leave app in connecting state on fetch failure', async () => {
      vi.useFakeTimers()

      const manager = new ConnectionManager(baseUrl)

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json({ error: 'Server error' }, { status: 500 })
        }),
      )

      await manager.connect()

      // Status should NOT be "connecting" after fetch failure
      expect(connectionStore.status).not.toBe('connecting')

      manager.destroy()
      vi.useRealTimers()
    })
  })
})
