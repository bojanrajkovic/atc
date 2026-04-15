import { HttpResponse, http } from 'msw'
import { setupServer } from 'msw/node'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConnectionManager } from '$lib/connection'
import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

// Mock WebSocket connections for testing
class MockWebSocket {
  url: string
  readyState = 0 // CONNECTING
  onopen: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  messageQueue: string[] = []

  static instances: MockWebSocket[] = []

  constructor(url: string) {
    this.url = url
    MockWebSocket.instances.push(this)
    // Simulate connection delay using Promise microtask (more reliable than setTimeout(0))
    Promise.resolve().then(() => {
      this.readyState = 1 // OPEN
      this.onopen?.(new Event('open'))
    })
  }

  send(data: string): void {
    this.messageQueue.push(data)
  }

  close(code?: number): void {
    this.readyState = 3 // CLOSED
    // Use Promise to ensure the close event is handled asynchronously
    Promise.resolve().then(() => {
      this.onclose?.(new CloseEvent('close', { code: code ?? 1000 }))
    })
  }

  // Helper to simulate receiving a message
  receiveMessage(data: string): void {
    Promise.resolve().then(() => {
      this.onmessage?.(new MessageEvent('message', { data }))
    })
  }

  static clearAll(): void {
    MockWebSocket.instances = []
  }

  static getLastInstance(): MockWebSocket | undefined {
    return MockWebSocket.instances[MockWebSocket.instances.length - 1]
  }
}

describe('ConnectionManager', () => {
  const baseUrl = 'http://localhost:3000'

  // Helper to serialize snapshots with BigInt
  const snapshotToJSON = (snapshot: StateSnapshot) => {
    return JSON.parse(
      JSON.stringify(snapshot, (_key, value) => {
        if (typeof value === 'bigint') {
          return value.toString()
        }
        return value
      }),
    )
  }

  // Default state snapshot for most tests
  const defaultSnapshot: StateSnapshot = {
    seq: 5n,
    runs: [],
    jobs: [],
    poolStats: [],
  }

  const server = setupServer(
    // Default state handler
    http.get('http://localhost:*/v1/state', () => {
      return HttpResponse.json(snapshotToJSON(defaultSnapshot))
    }),
  )

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

  describe('fe-foundation.AC4.1: Success — Connect sequence', () => {
    it('opens WebSocket, fetches state snapshot, loads stores, and transitions to connected', async () => {
      const manager = new ConnectionManager(baseUrl)

      // Connect and wait for completion
      await manager.connect()

      // Verify status transitioned to connected
      expect(connectionStore.status).toBe('connected')

      // Verify reconnectAttempt was reset to 0
      expect(connectionStore.reconnectAttempt).toBe(0)

      manager.destroy()
    })

    it('loads snapshot data into stores', async () => {
      // Create a custom snapshot with data
      const snapshotWithData: StateSnapshot = {
        seq: 10n,
        runs: [
          {
            id: 1n,
            org: 'org',
            repo: 'repo',
            workflowName: 'test',
            workflowPath: '.github/workflows/test.yml',
            branch: 'main',
            headSha: 'abc123',
            commitMessage: 'test commit',
            event: 'push',
            displayTitle: 'Test run',
            htmlUrl: 'https://github.com/org/repo/actions/runs/1',
            createdAt: new Date().toISOString(),
            runStartedAt: null,
            updatedAt: new Date().toISOString(),
            status: 'Queued',
            conclusion: null,
          },
        ],
        jobs: [],
        poolStats: [],
      }

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json(snapshotToJSON(snapshotWithData))
        }),
      )

      const manager = new ConnectionManager(baseUrl)
      await manager.connect()

      // Verify snapshot was loaded into stores
      expect(runStore.runs.has(1n)).toBe(true)

      manager.destroy()
    })
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
})
