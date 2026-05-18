import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import {
  defaultSnapshot,
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'

/**
 * Pre-snapshot close-race regression test (issue #47, DoD #5 / AC10).
 *
 * Before the abort fix, `handleDisconnect()` did NOT abort the in-flight
 * `/v1/state` fetch. If the socket closed during the fetch:
 *   1. `ws.onclose` fires → `handleDisconnect()` runs → schedule reconnect
 *   2. The original `connect()` call could still complete its fetch path and
 *      set `connectionStore.status = 'connected'` against a now-dead socket.
 *
 * `GoingAway` (also from this PR) makes this path likely on every redeploy
 * because the server's graceful-shutdown close arrives right around the
 * frontend's next-reconnect snapshot fetch. The fix: invalidate the connect
 * cycle on close by calling `this.abortController?.abort()` in
 * `handleDisconnect()` BEFORE scheduling the reconnect timer. The fetch
 * chain already bails on `signal.aborted`.
 */
describe('ConnectionManager — pre-snapshot close race (issue #47 abort fix)', () => {
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
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  it('socket close during /v1/state fetch must NOT land status=connected against the dead socket', async () => {
    // Delay the snapshot fetch so we can close the WS while it's in flight.
    server.use(
      http.get('http://localhost:*/v1/state', async () => {
        await new Promise((resolve) => setTimeout(resolve, 150))
        return HttpResponse.json(snapshotToJSON(defaultSnapshot))
      }),
    )

    const manager = new ConnectionManager(baseUrl)

    // Start the connect cycle and capture the promise — do NOT await yet.
    const connectPromise = manager.connect()

    // Give onopen a microtask to fire and the fetch a moment to start.
    await new Promise((resolve) => setTimeout(resolve, 20))

    // Close the socket mid-fetch. Without the abort fix the fetch keeps going.
    const ws = MockWebSocket.getLastInstance()
    expect(ws, 'expected a MockWebSocket instance').toBeTruthy()
    ws!.close(1001)

    // Let everything settle: fetch completes (or aborts), reconnect timer runs.
    await connectPromise.catch(() => {
      /* connect() may reject when aborted; that's part of the contract */
    })
    await new Promise((resolve) => setTimeout(resolve, 250))

    // After a mid-fetch close, status MUST be 'reconnecting' (handleDisconnect
    // scheduled a retry) and MUST NOT be 'connected' (the original fetch must
    // not have landed against the dead socket).
    expect(connectionStore.status).not.toBe('connected')
    expect(connectionStore.status).toBe('reconnecting')

    manager.destroy()
  })

  it('destroy() during /v1/state fetch already aborts (regression guard for the existing destroy path)', async () => {
    // Pre-existing behavior: destroy() aborts in-flight fetches. This test
    // pins that behavior so the new handleDisconnect abort doesn't accidentally
    // break the destroy path.
    server.use(
      http.get('http://localhost:*/v1/state', async () => {
        await new Promise((resolve) => setTimeout(resolve, 150))
        return HttpResponse.json(snapshotToJSON(defaultSnapshot))
      }),
    )

    const manager = new ConnectionManager(baseUrl)
    const connectPromise = manager.connect()

    await new Promise((resolve) => setTimeout(resolve, 20))
    manager.destroy()

    await connectPromise.catch(() => {})
    await new Promise((resolve) => setTimeout(resolve, 200))

    expect(connectionStore.status).toBe('disconnected')
  })
})
