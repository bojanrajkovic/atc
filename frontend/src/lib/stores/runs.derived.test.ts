import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  // AC3.4: Derived filters work correctly
  describe('AC3.4: Derived column filters', () => {
    it('should filter queuedRuns correctly', () => {
      const queued1 = 20n
      const queued2 = 21n
      const inProgress = 22n

      // Add queued runs
      runStore.applyRunEvent({
        runId: queued1,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 1',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      runStore.applyRunEvent({
        runId: queued2,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 2',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      // Add in-progress run
      runStore.applyRunEvent({
        runId: inProgress,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 3',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      runStore.applyRunEvent({
        runId: inProgress,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 3',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:05Z',
        action: { type: 'InProgress' },
      })

      expect(runStore.queuedRuns.length).toBe(2)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued1)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued2)
    })

    it('should filter inProgressRuns correctly', () => {
      const inProgress1 = 30n
      const inProgress2 = 31n
      const queued = 32n

      runStore.applyRunEvent({
        runId: inProgress1,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:05Z',
        action: { type: 'InProgress' },
      })

      runStore.applyRunEvent({
        runId: inProgress2,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:10Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      runStore.applyRunEvent({
        runId: queued,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      expect(runStore.inProgressRuns.length).toBe(2)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress1)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress2)
      expect(runStore.inProgressRuns.map((r) => r.id)).not.toContain(queued)
    })

    it('should filter completedRuns correctly', () => {
      const completed1 = 40n
      const completed2 = 41n
      const inProgress = 42n

      runStore.applyRunEvent({
        runId: completed1,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:15Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      })

      runStore.applyRunEvent({
        runId: completed2,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:20Z',
        action: { type: 'Completed', data: { conclusion: 'Failure' } },
      })

      runStore.applyRunEvent({
        runId: inProgress,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      expect(runStore.completedRuns.length).toBe(2)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed1)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed2)
      expect(runStore.completedRuns.map((r) => r.id)).not.toContain(inProgress)
    })
  })
})
