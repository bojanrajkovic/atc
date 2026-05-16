import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  defaultSnapshot,
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

const PRE_SNAPSHOT_CAPS = [{ labels: ['hot-reload', 'pre-snapshot'], capacity: 11 }]
const SNAPSHOT_CAPS = [{ labels: ['from-snapshot'], capacity: 5 }]
const POST_SNAPSHOT_CAPS = [{ labels: ['hot-reload', 'post-snapshot'], capacity: 99 }]

describe('ConnectionManager — ConfigUpdate / ConfigReloadError', () => {
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

  it('pre-snapshot ConfigUpdate is applied on top of snapshot capacities', async () => {
    const manager = new ConnectionManager(baseUrl)

    let resolveStateFetch: (() => void) | undefined
    const statePromise = new Promise<void>((resolve) => {
      resolveStateFetch = () => resolve()
    })

    const snapshotWithCaps: StateSnapshot = {
      ...defaultSnapshot,
      runnerPoolCapacities: SNAPSHOT_CAPS,
    }

    server.use(
      http.get('http://localhost:*/v1/state', async () => {
        await statePromise
        return HttpResponse.json(snapshotToJSON(snapshotWithCaps))
      }),
    )

    const connectPromise = manager.connect()
    await new Promise((resolve) => setTimeout(resolve, 10))

    // Send a ConfigUpdate BEFORE the snapshot resolves. The connection
    // manager should stash it in the pendingConfigUpdate slot and apply it
    // after loadSnapshot.
    const ws = MockWebSocket.getLastInstance()!
    ws.receiveMessage(
      JSON.stringify({
        kind: 'ConfigUpdate',
        runnerPoolCapacities: PRE_SNAPSHOT_CAPS,
      }),
    )

    resolveStateFetch?.()
    await connectPromise

    // pendingConfigUpdate must have been applied after loadSnapshot — it's
    // the latest known capacities, not the snapshot's.
    expect(runStore.runnerPoolCapacities).toEqual(PRE_SNAPSHOT_CAPS)

    manager.destroy()
  })

  it('pre-snapshot ConfigReloadError is dropped silently', async () => {
    const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      const manager = new ConnectionManager(baseUrl)

      let resolveStateFetch: (() => void) | undefined
      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      const snapshotWithCaps: StateSnapshot = {
        ...defaultSnapshot,
        runnerPoolCapacities: SNAPSHOT_CAPS,
      }

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          await statePromise
          return HttpResponse.json(snapshotToJSON(snapshotWithCaps))
        }),
      )

      const connectPromise = manager.connect()
      await new Promise((resolve) => setTimeout(resolve, 10))

      const ws = MockWebSocket.getLastInstance()!
      ws.receiveMessage(
        JSON.stringify({
          kind: 'ConfigReloadError',
          reason: 'capacity must be >= 1',
        }),
      )

      resolveStateFetch?.()
      await connectPromise

      // Pre-snapshot ConfigReloadError frames are informational only and
      // dropped. console.warn must NOT have fired (the dispatcher's warn
      // for ConfigReloadError fires only in connected mode).
      expect(warnSpy).not.toHaveBeenCalled()
      expect(runStore.runnerPoolCapacities).toEqual(SNAPSHOT_CAPS)

      manager.destroy()
    } finally {
      warnSpy.mockRestore()
    }
  })

  it('post-snapshot ConfigUpdate applies immediately', async () => {
    const manager = new ConnectionManager(baseUrl)

    const snapshotWithCaps: StateSnapshot = {
      ...defaultSnapshot,
      runnerPoolCapacities: SNAPSHOT_CAPS,
    }

    server.use(
      http.get('http://localhost:*/v1/state', () =>
        HttpResponse.json(snapshotToJSON(snapshotWithCaps)),
      ),
    )

    await manager.connect()
    expect(runStore.runnerPoolCapacities).toEqual(SNAPSHOT_CAPS)

    // Send a live ConfigUpdate after the connection is established.
    const ws = MockWebSocket.getLastInstance()!
    ws.receiveMessage(
      JSON.stringify({
        kind: 'ConfigUpdate',
        runnerPoolCapacities: POST_SNAPSHOT_CAPS,
      }),
    )
    // ConfigUpdate is out-of-band — applied synchronously by the dispatcher,
    // but the WebSocket onmessage handler dispatches asynchronously (Promise
    // microtask). Yield once to let the message land.
    await Promise.resolve()
    await Promise.resolve()

    expect(runStore.runnerPoolCapacities).toEqual(POST_SNAPSHOT_CAPS)

    manager.destroy()
  })
})
