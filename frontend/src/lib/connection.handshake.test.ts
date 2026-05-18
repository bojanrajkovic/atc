import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import { MockWebSocket, setupConnectionTestServer } from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'

function resetVersionFields(): void {
  connectionStore.serverVersionReference = null
  connectionStore.serverVersionMismatch = null
  connectionStore.serverReloadAt = null
  connectionStore.serverGoingAway = false
  connectionStore.goingAwayReason = null
}

/**
 * Pre-snapshot handshake routing (issue #47).
 *
 * ServerHello and GoingAway frames can arrive on the WS BEFORE the snapshot
 * fetch completes — ServerHello is in fact GUARANTEED to be the first frame
 * on every connection. The pre-snapshot switch in `connection.ts` must route
 * both variants to the right place, and must NOT buffer them into
 * `preConnectBuffer` (which is committed-events-only, seq-keyed).
 */
describe('ConnectionManager — pre-snapshot ServerHello / GoingAway routing (issue #47)', () => {
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
    resetVersionFields()
  })

  afterEach(() => {
    server.resetHandlers()
    runStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    resetVersionFields()
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  it('ServerHello received pre-snapshot sets connectionStore.serverVersionReference', async () => {
    const manager = new ConnectionManager(baseUrl)

    // Start the connect cycle but DON'T await — we want to inject a frame
    // between onopen (microtask) and the snapshot fetch completing.
    const connectPromise = manager.connect()

    // Let onopen fire.
    await new Promise((resolve) => setTimeout(resolve, 10))

    // Push a ServerHello as the first frame.
    const ws = MockWebSocket.getLastInstance()
    expect(ws, 'expected a MockWebSocket instance').toBeTruthy()
    ws!.receiveMessage(JSON.stringify({ kind: 'ServerHello', version: 'v1.0.0' }))

    // Let the snapshot path complete.
    await connectPromise

    expect(connectionStore.serverVersionReference).toBe('v1.0.0')
    expect(connectionStore.serverVersionMismatch).toBeNull()
    expect(connectionStore.serverReloadAt).toBeNull()

    manager.destroy()
  })

  it('GoingAway received pre-snapshot sets connectionStore.serverGoingAway', async () => {
    const manager = new ConnectionManager(baseUrl)
    const connectPromise = manager.connect()

    await new Promise((resolve) => setTimeout(resolve, 10))

    const ws = MockWebSocket.getLastInstance()
    expect(ws, 'expected a MockWebSocket instance').toBeTruthy()
    ws!.receiveMessage(JSON.stringify({ kind: 'GoingAway', reason: 'server shutdown' }))

    await connectPromise

    expect(connectionStore.serverGoingAway).toBe(true)
    expect(connectionStore.goingAwayReason).toBe('server shutdown')

    manager.destroy()
  })

  it('connectionStore.serverGoingAway resets to false on the next successful connected transition', async () => {
    // Pre-set the going-away flag as if from a prior connection cycle.
    connectionStore.serverGoingAway = true
    connectionStore.goingAwayReason = 'server shutdown'

    const manager = new ConnectionManager(baseUrl)
    await manager.connect()

    expect(connectionStore.status).toBe('connected')
    expect(connectionStore.serverGoingAway).toBe(false)
    expect(connectionStore.goingAwayReason).toBeNull()

    manager.destroy()
  })
})
