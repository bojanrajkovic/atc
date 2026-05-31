import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  createMockJob,
  createMockJobEvent,
  createMockRun,
  createMockRunEvent,
  createMockRunner,
} from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
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
      runStore.applyRunEvent(createMockRunEvent({ runId: 50n, displayTitle: 'Old Run' }))
      runStore.applyJobEvent(createMockJobEvent({ jobId: 500n, runId: 50n, name: 'Old Job' }))

      // Verify initial state
      expect(runStore.runs.size).toBe(1)
      expect(runStore.jobsByRun.size).toBe(1)

      // Load new snapshot
      const newRun = createMockRun({
        id: 51n,
        displayTitle: 'New Run',
        status: 'InProgress',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:05Z',
      })

      const newJob = createMockJob({
        id: 501n,
        name: 'New Job',
        runId: 51n,
        labels: ['ubuntu-latest'],
        createdAt: '2025-01-02T00:00:00Z',
      })

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
        displayTitle: 'Run 1',
        status: 'Completed',
        conclusion: 'Success',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:15Z',
      })

      const run2 = createMockRun({
        id: 61n,
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'develop',
        event: 'pull_request',
        displayTitle: 'Run 2',
        status: 'InProgress',
        createdAt: '2025-01-02T01:00:00Z',
        runStartedAt: '2025-01-02T01:00:05Z',
        updatedAt: '2025-01-02T01:00:10Z',
      })

      const job1 = createMockJob({
        id: 600n,
        name: 'Job 1',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Success',
        runner: createMockRunner({ name: 'Runner1' }),
        labels: ['ubuntu-latest'],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:15Z',
      })

      const job2 = createMockJob({
        id: 601n,
        name: 'Job 2',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Failure',
        runner: createMockRunner({ id: 2n, name: 'Runner2' }),
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:20Z',
      })

      const job3 = createMockJob({
        id: 602n,
        name: 'Job 3',
        runId: 61n,
        createdAt: '2025-01-02T01:00:00Z',
      })

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
        createdAt: '2026-04-16T09:00:00Z',
        updatedAt: '2026-04-16T09:00:00Z',
      })

      const run2 = createMockRun({
        id: 201n,
        displayTitle: 'Run 2',
        status: 'Queued',
        createdAt: '2026-04-16T09:00:00Z',
        updatedAt: '2026-04-16T09:00:00Z',
      })

      const run3 = createMockRun({
        id: 202n,
        displayTitle: 'Run 3',
        status: 'Queued',
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
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:05Z',
      })

      const run2 = createMockRun({
        id: 211n,
        displayTitle: 'Run 2',
        status: 'InProgress',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:05Z',
      })

      const run3 = createMockRun({
        id: 212n,
        displayTitle: 'Run 3',
        status: 'InProgress',
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
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:15Z',
      })

      const run2 = createMockRun({
        id: 221n,
        displayTitle: 'Run 2',
        status: 'Completed',
        conclusion: 'Success',
        createdAt: '2026-04-16T09:00:00Z',
        runStartedAt: '2026-04-16T09:00:05Z',
        updatedAt: '2026-04-16T09:00:15Z',
      })

      const run3 = createMockRun({
        id: 222n,
        displayTitle: 'Run 3',
        status: 'Completed',
        conclusion: 'Success',
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

  describe('rolling-deploy: missing runAttempt', () => {
    it('defaults missing runAttempt to 1 so jobs are not hidden', () => {
      // A pre-feature backend replica serves /v1/state without `runAttempt`.
      // The TS type claims it is always present, but at runtime it is absent —
      // simulate by deleting the field. Without normalization, the job
      // derivations compare `undefined >= undefined` (false) and drop every job.
      const run = { ...createMockRun({ id: 700n, status: 'InProgress' }) } as Record<
        string,
        unknown
      >
      delete run.runAttempt
      const jobNoAttempt = {
        ...createMockJob({ id: 7001n, name: 'job', runId: 700n, status: 'InProgress' }),
      } as Record<string, unknown>
      delete jobNoAttempt.runAttempt

      runStore.loadSnapshot([run as unknown as WorkflowRun], [jobNoAttempt as unknown as Job])

      // Normalized to 1 on both sides → job stays visible.
      expect(runStore.runs.get(700n)?.runAttempt).toBe(1)
      expect(runStore.jobsByRunId.get(700n)?.length).toBe(1)
      expect(runStore.jobStatsByRun.get(700n)?.total).toBe(1)
    })
  })
})
