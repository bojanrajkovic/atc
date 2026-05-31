import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMockRun } from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  describe('Atomic loadSnapshot', () => {
    it('should replace all runs and jobs when loading a snapshot', () => {
      // Set up initial state
      runStore.applyRunEvent({
        runId: 50n,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Old Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        runAttempt: 1,
        action: { type: 'Requested' },
      })

      runStore.applyJobEvent({
        jobId: 500n,
        runId: 50n,
        org: 'org',
        repo: 'repo',
        name: 'Old Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        runAttempt: 1,
        action: { type: 'Queued', data: { labels: [], steps: [] } },
      })

      // Verify initial state
      expect(runStore.runs.size).toBe(1)
      expect(runStore.jobsByRun.size).toBe(1)

      // Load new snapshot
      const newRun = createMockRun({
        id: 51n,
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        headSha: 'newsha',
        commitMessage: 'New msg',
        displayTitle: 'New Run',
        status: 'InProgress',
        htmlUrl: 'newurl',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:05Z',
      })

      const newJob: Job = {
        id: 501n,
        name: 'New Job',
        runId: 51n,
        status: 'Queued',
        conclusion: null,
        runner: null,
        labels: ['ubuntu-latest'],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: null,
        completedAt: null,
        runAttempt: 1,
      }

      runStore.loadSnapshot([newRun], [newJob])

      // Verify old state is gone
      expect(runStore.runs.has(50n)).toBe(false)
      expect(runStore.runs.has(51n)).toBe(true)
      expect(runStore.runs.size).toBe(1)

      // Verify new state is loaded
      const loadedRun = runStore.runs.get(51n)!
      expect(loadedRun.displayTitle).toBe('New Run')
      expect(loadedRun.status).toBe('InProgress')

      // Verify jobs are grouped correctly
      expect(runStore.jobsByRun.has(51n)).toBe(true)
      const jobs = runStore.jobsByRun.get(51n)!
      expect(jobs.length).toBe(1)
      expect(jobs[0]!.id).toBe(501n)
      expect(jobs[0]!.name).toBe('New Job')
    })

    it('should handle loading snapshot with multiple runs and jobs', () => {
      const run1 = createMockRun({
        id: 60n,
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        headSha: 'sha1',
        commitMessage: 'msg1',
        event: 'push',
        displayTitle: 'Run 1',
        status: 'Completed',
        conclusion: 'Success',
        htmlUrl: 'url1',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:15Z',
      })

      const run2 = createMockRun({
        id: 61n,
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'develop',
        headSha: 'sha2',
        commitMessage: 'msg2',
        event: 'pull_request',
        displayTitle: 'Run 2',
        status: 'InProgress',
        htmlUrl: 'url2',
        createdAt: '2025-01-02T01:00:00Z',
        runStartedAt: '2025-01-02T01:00:05Z',
        updatedAt: '2025-01-02T01:00:10Z',
      })

      const job1: Job = {
        id: 600n,
        name: 'Job 1',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Success',
        runner: { id: 1n, name: 'Runner1', groupName: null },
        labels: ['ubuntu-latest'],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:15Z',
        runAttempt: 1,
      }

      const job2: Job = {
        id: 601n,
        name: 'Job 2',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Failure',
        runner: { id: 2n, name: 'Runner2', groupName: null },
        labels: [],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:20Z',
        runAttempt: 1,
      }

      const job3: Job = {
        id: 602n,
        name: 'Job 3',
        runId: 61n,
        status: 'Queued',
        conclusion: null,
        runner: null,
        labels: [],
        steps: [],
        createdAt: '2025-01-02T01:00:00Z',
        startedAt: null,
        completedAt: null,
        runAttempt: 1,
      }

      runStore.loadSnapshot([run1, run2], [job1, job2, job3])

      // Verify runs
      expect(runStore.runs.size).toBe(2)
      expect(runStore.runs.get(60n)!.displayTitle).toBe('Run 1')
      expect(runStore.runs.get(61n)!.displayTitle).toBe('Run 2')

      // Verify jobs are grouped correctly by run ID
      expect(runStore.jobsByRun.get(60n)!.length).toBe(2)
      expect(runStore.jobsByRun.get(61n)!.length).toBe(1)
      expect(runStore.jobsByRun.get(60n)!.map((j) => j.id)).toContain(600n)
      expect(runStore.jobsByRun.get(60n)!.map((j) => j.id)).toContain(601n)
      expect(runStore.jobsByRun.get(61n)!.map((j) => j.id)).toContain(602n)
    })
  })

  describe('Sort order stability on snapshot reload', () => {
    it('queuedRuns maintains same order after reload with same runs in different input order', () => {
      // Create runs with identical createdAt (to test tie-breaker stability)
      const run1 = createMockRun({
        id: 200n,
        displayTitle: 'Run 1',
        status: 'Queued',
        htmlUrl: 'url1',
        createdAt: '2026-04-16T09:00:00Z',
        updatedAt: '2026-04-16T09:00:00Z',
      })

      const run2 = createMockRun({
        id: 201n,
        headSha: 'sha2',
        commitMessage: 'msg2',
        displayTitle: 'Run 2',
        status: 'Queued',
        htmlUrl: 'url2',
        createdAt: '2026-04-16T09:00:00Z',
        updatedAt: '2026-04-16T09:00:00Z',
      })

      const run3 = createMockRun({
        id: 202n,
        headSha: 'sha3',
        commitMessage: 'msg3',
        displayTitle: 'Run 3',
        status: 'Queued',
        htmlUrl: 'url3',
        createdAt: '2026-04-16T09:00:00Z',
        updatedAt: '2026-04-16T09:00:00Z',
      })

      // Load snapshot in forward order
      runStore.loadSnapshot([run1, run2, run3], [])
      const firstOrderIds = runStore.queuedRuns.map((r) => r.id)

      // Load snapshot again in reverse order
      runStore.loadSnapshot([run3, run2, run1], [])
      const secondOrderIds = runStore.queuedRuns.map((r) => r.id)

      // Order should be identical (based on id tie-breaker, not Map iteration order)
      expect(firstOrderIds).toEqual(secondOrderIds)
      expect(firstOrderIds).toEqual([200n, 201n, 202n])
    })

    it('inProgressRuns maintains same order after reload with same runs in different input order', () => {
      const run1 = createMockRun({
        id: 210n,
        displayTitle: 'Run 1',
        status: 'InProgress',
        htmlUrl: 'url1',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:05Z',
      })

      const run2 = createMockRun({
        id: 211n,
        headSha: 'sha2',
        commitMessage: 'msg2',
        displayTitle: 'Run 2',
        status: 'InProgress',
        htmlUrl: 'url2',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:05Z',
      })

      const run3 = createMockRun({
        id: 212n,
        headSha: 'sha3',
        commitMessage: 'msg3',
        displayTitle: 'Run 3',
        status: 'InProgress',
        htmlUrl: 'url3',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:05Z',
      })

      // Load snapshot in forward order
      runStore.loadSnapshot([run1, run2, run3], [])
      const firstOrderIds = runStore.inProgressRuns.map((r) => r.id)

      // Load snapshot again in reverse order
      runStore.loadSnapshot([run3, run2, run1], [])
      const secondOrderIds = runStore.inProgressRuns.map((r) => r.id)

      // Order should be identical (descending id tie-breaker)
      expect(firstOrderIds).toEqual(secondOrderIds)
      expect(firstOrderIds).toEqual([212n, 211n, 210n])
    })

    it('completedRuns maintains same order after reload with same runs in different input order', () => {
      const run1 = createMockRun({
        id: 220n,
        displayTitle: 'Run 1',
        status: 'Completed',
        conclusion: 'Success',
        htmlUrl: 'url1',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:15Z',
      })

      const run2 = createMockRun({
        id: 221n,
        headSha: 'sha2',
        commitMessage: 'msg2',
        displayTitle: 'Run 2',
        status: 'Completed',
        conclusion: 'Success',
        htmlUrl: 'url2',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:15Z',
      })

      const run3 = createMockRun({
        id: 222n,
        headSha: 'sha3',
        commitMessage: 'msg3',
        displayTitle: 'Run 3',
        status: 'Completed',
        conclusion: 'Success',
        htmlUrl: 'url3',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:15Z',
      })

      // Load snapshot in forward order
      runStore.loadSnapshot([run1, run2, run3], [])
      const firstOrderIds = runStore.completedRuns.map((r) => r.id)

      // Load snapshot again in reverse order
      runStore.loadSnapshot([run3, run2, run1], [])
      const secondOrderIds = runStore.completedRuns.map((r) => r.id)

      // Order should be identical (descending id tie-breaker)
      expect(firstOrderIds).toEqual(secondOrderIds)
      expect(firstOrderIds).toEqual([222n, 221n, 220n])
    })
  })
})
