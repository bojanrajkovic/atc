import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'
import { type JobStats, runStore } from './runs.svelte'

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
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued1,
          displayTitle: 'Run 1',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued2,
          displayTitle: 'Run 2',
          action: { type: 'Requested' },
        }),
      )

      // Add in-progress run
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          displayTitle: 'Run 3',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          displayTitle: 'Run 3',
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.queuedRuns.length).toBe(2)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued1)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued2)
    })

    it('should filter inProgressRuns correctly', () => {
      const inProgress1 = 30n
      const inProgress2 = 31n
      const queued = 32n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress1,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress2,
          runStartedAt: '2025-01-01T00:00:10Z',
          updatedAt: '2025-01-01T00:00:10Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued,
          runStartedAt: null,
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.inProgressRuns.length).toBe(2)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress1)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress2)
      expect(runStore.inProgressRuns.map((r) => r.id)).not.toContain(queued)
    })

    it('should filter completedRuns correctly', () => {
      const completed1 = 40n
      const completed2 = 41n
      const inProgress = 42n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: completed1,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: completed2,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:20Z',
          action: { type: 'Completed', data: { conclusion: 'Failure' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:10Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.completedRuns.length).toBe(2)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed1)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed2)
      expect(runStore.completedRuns.map((r) => r.id)).not.toContain(inProgress)
    })
  })

  // AC3.1-AC3.5, AC3.7: Sort order tests
  describe('AC3.1-AC3.7: Sort strategies', () => {
    // AC3.1: queuedRuns sorted ascending by createdAt
    it('AC3.1: queuedRuns sorted ascending by createdAt', () => {
      const runId1 = 100n
      const runId2 = 101n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Later run',
          createdAt: '2026-04-16T10:00:00Z',
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Earlier run',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.queuedRuns[0]?.id).toBe(runId2) // Earlier first
      expect(runStore.queuedRuns[1]?.id).toBe(runId1) // Later second
    })

    // AC3.2: inProgressRuns sorted descending by runStartedAt
    it('AC3.2: inProgressRuns sorted descending by runStartedAt', () => {
      const runId1 = 110n
      const runId2 = 111n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Earlier start',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Later start',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T10:00:05Z',
          updatedAt: '2026-04-16T10:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns[0]?.id).toBe(runId2) // Later start first
      expect(runStore.inProgressRuns[1]?.id).toBe(runId1) // Earlier start second
    })

    // AC3.3: inProgressRuns with null runStartedAt falls back to createdAt
    it('AC3.3: inProgressRuns with null runStartedAt falls back to createdAt', () => {
      const runIdNull = 120n
      const runIdWithStart = 121n

      // Run with null runStartedAt (uses createdAt for sort)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdNull,
          displayTitle: 'No start time',
          createdAt: '2026-04-16T10:00:00Z',
          runStartedAt: null,
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      // Transition to InProgress without runStartedAt (stays null)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdNull,
          displayTitle: 'No start time',
          createdAt: '2026-04-16T10:00:00Z',
          runStartedAt: null,
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'InProgress' },
        }),
      )

      // Run with runStartedAt set
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdWithStart,
          displayTitle: 'With start time',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns.length).toBe(2)
      // The null-fallback run (createdAt '2026-04-16T10:00:00Z') is later in time
      // than the started run ('2026-04-16T09:00:05Z'), so under descending sort it comes first
      expect(runStore.inProgressRuns[0]?.id).toBe(runIdNull) // null-fallback run (createdAt 10:00)
      expect(runStore.inProgressRuns[1]?.id).toBe(runIdWithStart) // started run (runStartedAt 09:00:05)
    })

    // AC3.4: completedRuns sorted descending by updatedAt
    it('AC3.4: completedRuns sorted descending by updatedAt', () => {
      const runId1 = 130n
      const runId2 = 131n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Earlier update',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Later update',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:20Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      expect(runStore.completedRuns[0]?.id).toBe(runId2) // Later update first
      expect(runStore.completedRuns[1]?.id).toBe(runId1) // Earlier update second
    })

    // AC3.5: Tie-breaker tests using run.id
    it('AC3.5a: queuedRuns tie-breaker uses ascending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same createdAt - ordering should be determined by id (ascending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.queuedRuns[0]?.id).toBe(runId2) // 1n
      expect(runStore.queuedRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.queuedRuns[2]?.id).toBe(runId1) // 3n
    })

    // AC3.5b: inProgressRuns tie-breaker uses descending id
    it('AC3.5b: inProgressRuns tie-breaker uses descending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same runStartedAt - ordering should be determined by id (descending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns[0]?.id).toBe(runId1) // 3n (descending)
      expect(runStore.inProgressRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.inProgressRuns[2]?.id).toBe(runId2) // 1n
    })

    // AC3.5c: completedRuns tie-breaker uses descending id
    it('AC3.5c: completedRuns tie-breaker uses descending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same updatedAt - ordering should be determined by id (descending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      expect(runStore.completedRuns[0]?.id).toBe(runId1) // 3n (descending)
      expect(runStore.completedRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.completedRuns[2]?.id).toBe(runId2) // 1n
    })

    // AC3.7: Sort uses lexical comparison, not localeCompare
    // This test has two parts: behavioral verification and source-level assertion
    it('AC3.7: Sort implementation uses direct lexical comparison', () => {
      // Create runs with timestamps that would differ under locale-aware sorting
      const runId1 = 150n
      const runId2 = 151n

      // ISO-8601 timestamps: "2026-04-16T10:00:00Z" > "2026-04-16T09:00:00Z" lexically
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T10:00:00Z',
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      // Behavioral: If using direct < comparison: '2026-04-16T09:00:00Z' < '2026-04-16T10:00:00Z' = true
      // runId2 should come before runId1
      expect(runStore.queuedRuns[0]?.id).toBe(runId2)
      expect(runStore.queuedRuns[1]?.id).toBe(runId1)

      // Source-level: Assert the implementation does not use localeCompare
      const storeSource = readFileSync(
        resolve(dirname(fileURLToPath(import.meta.url)), './runs.svelte.ts'),
        'utf-8',
      )
      expect(storeSource).not.toContain('localeCompare')
    })
  })

  // Inline fixture helper for jobStatsByRun tests
  const runner = (name: string, id: bigint = 1n): RunnerInfo => ({
    id,
    name,
    groupId: null,
    groupName: null,
  })

  describe('runStore.jobStatsByRun', () => {
    // AC3.1: Type shape and export
    it('AC3.1: jobStatsByRun exports JobStats type with correct shape', () => {
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: 1n,
          action: { type: 'Requested' },
        }),
      )

      const entry = runStore.jobStatsByRun.get(1n)
      expect(entry).toBeDefined()

      // Type-level assertion: variable of type JobStats
      const stats: JobStats = entry!
      expect(typeof stats.completed).toBe('number')
      expect(typeof stats.total).toBe('number')
      expect(stats.runnerSummary === null || typeof stats.runnerSummary === 'string').toBe(true)

      // Shape check: for a run with no jobs, should be { completed: 0, total: 0, runnerSummary: null }
      expect(stats.completed).toBe(0)
      expect(stats.total).toBe(0)
      expect(stats.runnerSummary).toBeNull()
    })

    // AC3.2: Completed job count and summary
    it('AC3.2: completed count reflects Completed jobs; runnerSummary matches summarizeRunners', () => {
      const runId = 200n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: { type: 'Requested' },
        }),
      )

      // Add three jobs: one Completed, two Queued
      runStore.applyJobEvent({
        jobId: 1n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      runStore.applyJobEvent({
        jobId: 2n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-2',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Completed',
          data: {
            conclusion: 'Success',
            runner: runner('runner-a'),
            labels: [],
            steps: [],
          },
        },
      })

      runStore.applyJobEvent({
        jobId: 3n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-3',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      const stats = runStore.jobStatsByRun.get(runId)
      expect(stats).toBeDefined()
      expect(stats!.completed).toBe(1)
      expect(stats!.total).toBe(3)
      expect(stats!.runnerSummary).toBe('runner-a')
    })

    // AC3.3: Total-map invariant — runs with no jobs get fallback entry
    it('AC3.3: total-map invariant — every run has an entry, even with no jobs', () => {
      const run1 = 300n
      const run2 = 301n

      // Add two runs
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: run1,
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: run2,
          action: { type: 'Requested' },
        }),
      )

      // Add jobs only to run1
      runStore.applyJobEvent({
        jobId: 1n,
        runId: run1,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      // Assert total-map property
      expect(runStore.jobStatsByRun.size).toBe(2)
      expect(runStore.jobStatsByRun.get(run1)).toBeDefined()
      expect(runStore.jobStatsByRun.get(run2)).toBeDefined()

      // run2 without jobs should have fallback
      const run2Stats = runStore.jobStatsByRun.get(run2)
      expect(run2Stats).not.toBeUndefined()
      expect(run2Stats!.completed).toBe(0)
      expect(run2Stats!.total).toBe(0)
      expect(run2Stats!.runnerSummary).toBeNull()
    })

    // AC3.4: Derived dependency tracking (formula correctness)
    it('AC3.4: jobStatsByRun correctly computes counts from jobsByRun state', () => {
      const runId = 400n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: { type: 'Requested' },
        }),
      )

      // Add three jobs with mixed statuses
      const job1Details = {
        jobId: 1n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
      }

      const job2Details = {
        jobId: 2n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        createdAt: '2026-04-17T00:00:01Z',
        startedAt: '2026-04-17T00:00:05Z',
        completedAt: null,
      }

      const job3Details = {
        jobId: 3n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        createdAt: '2026-04-17T00:00:02Z',
        startedAt: '2026-04-17T00:00:05Z',
        completedAt: '2026-04-17T00:00:10Z',
      }

      // Job 1: Queued
      runStore.applyJobEvent({
        ...job1Details,
        name: 'job-1',
        action: { type: 'Queued', data: { labels: [], steps: [] } },
      })

      // Job 2: InProgress
      runStore.applyJobEvent({
        ...job2Details,
        name: 'job-2',
        action: {
          type: 'InProgress',
          data: { runner: null, labels: [], steps: [] },
        },
      })

      // Job 3: Completed
      runStore.applyJobEvent({
        ...job3Details,
        name: 'job-3',
        action: {
          type: 'Completed',
          data: {
            conclusion: 'Success',
            runner: null,
            labels: [],
            steps: [],
          },
        },
      })

      // Verify internal state is correct
      const jobs = runStore.jobsByRun.get(runId) || []
      expect(jobs).toHaveLength(3)
      expect(jobs.filter((j) => j.status === 'Queued')).toHaveLength(1)
      expect(jobs.filter((j) => j.status === 'InProgress')).toHaveLength(1)
      expect(jobs.filter((j) => j.status === 'Completed')).toHaveLength(1)

      // Verify jobStatsByRun correctly counts completed jobs
      const stats = runStore.jobStatsByRun.get(runId)
      expect(stats!.total).toBe(3)
      expect(stats!.completed).toBe(1)
    })

    // AC3.5: Integration with summarizeRunners
    it('AC3.5: runnerSummary integrates with summarizeRunners for single runner', () => {
      const runId = 500n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: { type: 'Requested' },
        }),
      )

      // Add jobs all on the same runner
      runStore.applyJobEvent({
        jobId: 1n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            runner: runner('runner-a'),
            labels: [],
            steps: [],
          },
        },
      })

      runStore.applyJobEvent({
        jobId: 2n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-2',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            runner: runner('runner-a'),
            labels: [],
            steps: [],
          },
        },
      })

      const stats = runStore.jobStatsByRun.get(runId)
      expect(stats!.runnerSummary).toBe('runner-a')
    })

    // AC3.5b: Integration with summarizeRunners for multiple runners
    it('AC3.5: runnerSummary integrates with summarizeRunners for multiple runners', () => {
      const runId = 501n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId,
          action: { type: 'Requested' },
        }),
      )

      // Add jobs on different runners
      runStore.applyJobEvent({
        jobId: 1n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            runner: runner('runner-a'),
            labels: [],
            steps: [],
          },
        },
      })

      runStore.applyJobEvent({
        jobId: 2n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-2',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            runner: runner('runner-b'),
            labels: [],
            steps: [],
          },
        },
      })

      const stats = runStore.jobStatsByRun.get(runId)
      expect(stats!.runnerSummary).toBe('2 runners')
    })
  })

  describe('runStore.jobsByRunId', () => {
    it('jobsByRunId returns a readonly map of jobs per run', () => {
      const run1 = 600n
      const run2 = 601n
      const unknownId = 999n

      // Create and apply run events
      runStore.applyRunEvent(createMockRunEvent({ runId: run1, action: { type: 'Requested' } }))
      runStore.applyRunEvent(createMockRunEvent({ runId: run2, action: { type: 'Requested' } }))

      // Add jobs via applyJobEvent to populate jobsByRun
      // Job 1: Queued for run1
      runStore.applyJobEvent({
        jobId: 1n,
        runId: run1,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      // Job 2: Completed for run1
      runStore.applyJobEvent({
        jobId: 2n,
        runId: run1,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-2',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: '2026-04-17T00:00:10Z',
        action: {
          type: 'Completed',
          data: {
            conclusion: 'Success',
            runner: null,
            labels: [],
            steps: [],
          },
        },
      })

      // Job 3: InProgress for run2
      runStore.applyJobEvent({
        jobId: 3n,
        runId: run2,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-3',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: '2026-04-17T00:00:05Z',
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            runner: null,
            labels: [],
            steps: [],
          },
        },
      })

      // Assert jobsByRunId snapshot structure
      expect(runStore.jobsByRunId.size).toBe(2)
      expect(runStore.jobsByRunId.get(run1)?.length).toBe(2)
      expect(runStore.jobsByRunId.get(run2)?.length).toBe(1)
      expect(runStore.jobsByRunId.get(unknownId)).toBeUndefined()
    })

    it('jobsByRunId reflects job mutations in real time', () => {
      const run1 = 700n

      runStore.applyRunEvent(createMockRunEvent({ runId: run1, action: { type: 'Requested' } }))

      // Initially no jobs
      expect(runStore.jobsByRunId.get(run1)).toBeUndefined()

      // Add a job
      runStore.applyJobEvent({
        jobId: 1n,
        runId: run1,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-1',
        createdAt: '2026-04-17T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: [], steps: [] } },
      })

      // Now the job should be in jobsByRunId
      expect(runStore.jobsByRunId.get(run1)?.length).toBe(1)

      // Add another job
      runStore.applyJobEvent({
        jobId: 2n,
        runId: run1,
        org: 'test-org',
        repo: 'test-repo',
        name: 'job-2',
        createdAt: '2026-04-17T00:00:01Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: [], steps: [] } },
      })

      // Now we should have two jobs
      expect(runStore.jobsByRunId.get(run1)?.length).toBe(2)
    })
  })
})
