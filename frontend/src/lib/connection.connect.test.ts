import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest'
import {
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { ConnectionManager } from '$lib/connection'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
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

  describe('live-pool-stats.AC2.3: Success — Snapshot seeds runnerStore.pools', () => {
    it('loads snapshot poolStats into runnerStore.pools', async () => {
      const poolStats: RunnerPoolStats[] = [
        {
          labels: ['ubuntu-latest'],
          queued: 2,
          running: 1,
          groupName: 'GitHub Actions',
          isElastic: true,
          total: null,
        },
        {
          labels: ['self-hosted', 'linux'],
          queued: 0,
          running: 1,
          groupName: 'Custom Runners',
          isElastic: false,
          total: 4,
        },
      ]

      const snapshotWithPools: StateSnapshot = {
        seq: 5n,
        runs: [],
        jobs: [],
        poolStats,
      }

      server.use(
        http.get('http://localhost:*/v1/state', () => {
          return HttpResponse.json(snapshotToJSON(snapshotWithPools))
        }),
      )

      const manager = new ConnectionManager(baseUrl)
      await manager.connect()

      // Verify runnerStore.pools equals the snapshot poolStats
      expect(runnerStore.pools).toEqual(poolStats)

      // Belt-and-suspenders: verify connected status
      expect(connectionStore.status).toBe('connected')

      manager.destroy()
    })
  })
})
