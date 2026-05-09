import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import {
  defaultSnapshot,
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
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

  describe('Edge — Events buffered during state fetch are replayed after seq filtering', () => {
    it('buffers events arriving while state fetch is in progress', async () => {
      const manager = new ConnectionManager(baseUrl)
      let resolveStateFetch: (() => void) | undefined

      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          // Delay the state response to simulate in-progress fetch
          await statePromise
          return HttpResponse.json(snapshotToJSON(defaultSnapshot))
        }),
      )

      const connectPromise = manager.connect()

      // Give the WebSocket time to connect
      await new Promise((resolve) => setTimeout(resolve, 10))

      // Get the mock WebSocket and send an event while state fetch is in progress
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        const event: SeqEvent = {
          seq: 10n,
          event: {
            type: 'Run',
            data: {
              runId: 999n,
              org: 'org',
              repo: 'repo',
              workflowName: 'test',
              workflowPath: '.github/workflows/test.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test commit',
              triggerEvent: 'push',
              displayTitle: 'Test run',
              htmlUrl: 'https://github.com/org/repo/actions/runs/999',
              createdAt: new Date().toISOString(),
              runStartedAt: null,
              updatedAt: new Date().toISOString(),
              action: {
                type: 'Requested',
              },
            } as RunEventEnvelope,
          },
        }
        ws.receiveMessage(
          JSON.stringify(event, (_key, value) => {
            if (typeof value === 'bigint') {
              return value.toString()
            }
            return value
          }),
        )
      }

      // Resolve the state fetch
      resolveStateFetch?.()

      await connectPromise

      // The buffered event should have been dispatched (seq 10 > snapshot lastSeq 5)
      eventDispatcher.flush()

      // Verify the run from the buffered event is in the store
      expect(runStore.runs.has(999n)).toBe(true)

      manager.destroy()
    })

    it('discards buffered events with seq <= snapshot.lastSeq', async () => {
      const manager = new ConnectionManager(baseUrl)

      const snapshotWithSeq: StateSnapshot = {
        lastSeq: 10n,
        runs: [],
        jobs: [],
      }

      let resolveStateFetch: (() => void) | undefined

      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          await statePromise
          return HttpResponse.json(snapshotToJSON(snapshotWithSeq))
        }),
      )

      const connectPromise = manager.connect()

      // Give the WebSocket time to connect
      await new Promise((resolve) => setTimeout(resolve, 10))

      // Get the mock WebSocket and send an event with seq <= snapshot.lastSeq (should be discarded)
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        const staleEvent: SeqEvent = {
          seq: 5n,
          event: {
            type: 'Run',
            data: {
              runId: 888n,
              org: 'org',
              repo: 'repo',
              workflowName: 'test',
              workflowPath: '.github/workflows/test.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test commit',
              triggerEvent: 'push',
              displayTitle: 'Stale run',
              htmlUrl: 'https://github.com/org/repo/actions/runs/888',
              createdAt: new Date().toISOString(),
              runStartedAt: null,
              updatedAt: new Date().toISOString(),
              action: {
                type: 'Requested',
              },
            } as RunEventEnvelope,
          },
        }
        ws.receiveMessage(
          JSON.stringify(staleEvent, (_key, value) => {
            if (typeof value === 'bigint') {
              return value.toString()
            }
            return value
          }),
        )
      }

      // Resolve the state fetch
      resolveStateFetch?.()

      await connectPromise

      eventDispatcher.flush()

      // The stale event (seq 5 <= snapshot lastSeq 10) should NOT have been dispatched
      expect(runStore.runs.has(888n)).toBe(false)

      manager.destroy()
    })

    it('discards buffered events with seq === lastSeq', async () => {
      const manager = new ConnectionManager(baseUrl)

      const snapshotWithSeq: StateSnapshot = {
        lastSeq: 10n,
        runs: [],
        jobs: [],
      }

      let resolveStateFetch: (() => void) | undefined

      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          await statePromise
          return HttpResponse.json(snapshotToJSON(snapshotWithSeq))
        }),
      )

      const connectPromise = manager.connect()

      // Give the WebSocket time to connect
      await new Promise((resolve) => setTimeout(resolve, 10))

      // Get the mock WebSocket and send an event with seq === snapshot.lastSeq (should be discarded)
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        const freshEvent: SeqEvent = {
          seq: 10n,
          event: {
            type: 'Run',
            data: {
              runId: 777n,
              org: 'org',
              repo: 'repo',
              workflowName: 'test',
              workflowPath: '.github/workflows/test.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test commit',
              triggerEvent: 'push',
              displayTitle: 'Fresh run',
              htmlUrl: 'https://github.com/org/repo/actions/runs/777',
              createdAt: new Date().toISOString(),
              runStartedAt: null,
              updatedAt: new Date().toISOString(),
              action: {
                type: 'Requested',
              },
            } as RunEventEnvelope,
          },
        }
        ws.receiveMessage(
          JSON.stringify(freshEvent, (_key, value) => {
            if (typeof value === 'bigint') {
              return value.toString()
            }
            return value
          }),
        )
      }

      // Resolve the state fetch
      resolveStateFetch?.()

      await connectPromise

      eventDispatcher.flush()

      // The event (seq 10 === lastSeq 10) should be DISCARDED — equality no longer replays
      expect(runStore.runs.has(777n)).toBe(false)

      manager.destroy()
    })

    it('replays buffered event with seq=1 when snapshot lastSeq=0 (pre-increment cold-start fix)', async () => {
      const manager = new ConnectionManager(baseUrl)

      const emptySnapshot: StateSnapshot = {
        lastSeq: 0n,
        runs: [],
        jobs: [],
      }

      let resolveStateFetch: (() => void) | undefined
      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          await statePromise
          return HttpResponse.json(snapshotToJSON(emptySnapshot))
        }),
      )

      const connectPromise = manager.connect()
      await new Promise((resolve) => setTimeout(resolve, 10))

      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        // seq=1 is the first broadcast seq with pre-increment counter
        const firstEvent: SeqEvent = {
          seq: 1n,
          event: {
            type: 'Run',
            data: {
              runId: 42n,
              org: 'org',
              repo: 'repo',
              workflowName: 'test',
              workflowPath: '.github/workflows/test.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'first commit',
              triggerEvent: 'push',
              displayTitle: 'First run',
              htmlUrl: 'https://github.com/org/repo/actions/runs/42',
              createdAt: new Date().toISOString(),
              runStartedAt: null,
              updatedAt: new Date().toISOString(),
              action: { type: 'Requested' },
            } as RunEventEnvelope,
          },
        }
        ws.receiveMessage(
          JSON.stringify(firstEvent, (_key, value) =>
            typeof value === 'bigint' ? value.toString() : value,
          ),
        )
      }

      resolveStateFetch?.()
      await connectPromise
      eventDispatcher.flush()

      // seq=1 > lastSeq=0 → DISPATCHED (pre-increment cold-start race correctly handled)
      expect(runStore.runs.has(42n)).toBe(true)

      manager.destroy()
    })
  })
})
