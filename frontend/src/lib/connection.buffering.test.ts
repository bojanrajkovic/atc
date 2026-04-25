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
import { runnerStore } from '$lib/stores/runners.svelte'
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
    runnerStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    MockWebSocket.clearAll()
  })

  afterAll(() => {
    server.close()
  })

  describe('fe-foundation.AC4.8: Edge — Events buffered during state fetch are replayed after seq filtering', () => {
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
          poolStatsAfter: null,
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

      // The buffered event should have been dispatched (seq 10 >= snapshot seq 5)
      eventDispatcher.flush()

      // Verify the run from the buffered event is in the store
      expect(runStore.runs.has(999n)).toBe(true)

      manager.destroy()
    })

    it('discards buffered events with seq < snapshot.seq', async () => {
      const manager = new ConnectionManager(baseUrl)

      const snapshotWithSeq: StateSnapshot = {
        seq: 10n,
        runs: [],
        jobs: [],
        poolStats: [],
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

      // Get the mock WebSocket and send an event with seq < snapshot.seq (should be discarded)
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
          poolStatsAfter: null,
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

      // The stale event (seq 5 < snapshot seq 10) should NOT have been dispatched
      expect(runStore.runs.has(888n)).toBe(false)

      manager.destroy()
    })

    it('dispatches buffered events with seq >= snapshot.seq', async () => {
      const manager = new ConnectionManager(baseUrl)

      const snapshotWithSeq: StateSnapshot = {
        seq: 10n,
        runs: [],
        jobs: [],
        poolStats: [],
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

      // Get the mock WebSocket and send an event with seq >= snapshot.seq (should be dispatched)
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
          poolStatsAfter: null,
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

      // The fresh event (seq 10 >= snapshot seq 10) should have been dispatched
      expect(runStore.runs.has(777n)).toBe(true)

      manager.destroy()
    })
  })

  describe('live-pool-stats.AC2.4: Success — Pre-connect buffer applies sidecars after snapshot', () => {
    it('buffers Job events with poolStatsAfter sidecars and applies them after snapshot load', async () => {
      const manager = new ConnectionManager(baseUrl)
      let resolveStateFetch: (() => void) | undefined

      const statePromise = new Promise<void>((resolve) => {
        resolveStateFetch = () => resolve()
      })

      server.use(
        http.get('http://localhost:*/v1/state', async () => {
          await statePromise
          return HttpResponse.json(snapshotToJSON(defaultSnapshot))
        }),
      )

      const connectPromise = manager.connect()

      // Give the WebSocket time to connect
      await new Promise((resolve) => setTimeout(resolve, 10))

      // Get the mock WebSocket and send two Job events with poolStatsAfter sidecars while state fetch is paused
      const ws = MockWebSocket.getLastInstance()
      if (ws) {
        // First Job event with poolStatsAfter = P1
        const event1: SeqEvent = {
          seq: 10n,
          event: {
            type: 'Job',
            data: {
              jobId: 100n,
              runId: 1n,
              org: 'org',
              repo: 'repo',
              name: 'test-job-1',
              createdAt: new Date().toISOString(),
              startedAt: null,
              completedAt: null,
              action: {
                type: 'Queued',
                data: {
                  labels: ['ubuntu-latest'],
                  steps: [],
                },
              },
            },
          },
          poolStatsAfter: [
            {
              labels: ['ubuntu-latest'],
              queued: 1,
              running: 0,
              groupName: 'GitHub Actions',
              isElastic: true,
              total: null,
            },
          ],
        }

        ws.receiveMessage(
          JSON.stringify(event1, (_key, value) => {
            if (typeof value === 'bigint') {
              return value.toString()
            }
            return value
          }),
        )

        // Second Job event with poolStatsAfter = P2
        const event2: SeqEvent = {
          seq: 11n,
          event: {
            type: 'Job',
            data: {
              jobId: 101n,
              runId: 1n,
              org: 'org',
              repo: 'repo',
              name: 'test-job-2',
              createdAt: new Date().toISOString(),
              startedAt: null,
              completedAt: null,
              action: {
                type: 'InProgress',
                data: {
                  runner: null,
                  labels: ['ubuntu-latest'],
                  steps: [],
                },
              },
            },
          },
          poolStatsAfter: [
            {
              labels: ['ubuntu-latest'],
              queued: 0,
              running: 1,
              groupName: 'GitHub Actions',
              isElastic: true,
              total: null,
            },
          ],
        }

        ws.receiveMessage(
          JSON.stringify(event2, (_key, value) => {
            if (typeof value === 'bigint') {
              return value.toString()
            }
            return value
          }),
        )
      }

      // Resolve the state fetch, triggering buffer replay
      resolveStateFetch?.()

      await connectPromise

      // After buffer flush, runnerStore.pools should equal P2 (last sidecar wins)
      const expectedP2 = [
        {
          labels: ['ubuntu-latest'],
          queued: 0,
          running: 1,
          groupName: 'GitHub Actions',
          isElastic: true,
          total: null,
        },
      ]

      expect(runnerStore.pools).toEqual(expectedP2)

      manager.destroy()
    })
  })
})
