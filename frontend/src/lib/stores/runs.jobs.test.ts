import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMockJobEvent, createMockRunner } from '$lib/test-utils/factories'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  describe('Group jobs by run ID', () => {
    it('should create a job for an unknown run ID', () => {
      const runId = 10n
      const jobId = 100n

      runStore.applyJobEvent(createMockJobEvent({ jobId, runId, name: 'Test Job' }))

      expect(runStore.jobsByRun.has(runId)).toBe(true)
      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(1)
      expect(jobs[0]!.id).toBe(jobId)
      expect(jobs[0]!.name).toBe('Test Job')
      expect(jobs[0]!.status).toBe('Queued')
    })

    it('should group multiple jobs with the same run ID', () => {
      const runId = 11n
      const jobId1 = 101n
      const jobId2 = 102n

      runStore.applyJobEvent(createMockJobEvent({ jobId: jobId1, runId, name: 'Job 1' }))
      runStore.applyJobEvent(createMockJobEvent({ jobId: jobId2, runId, name: 'Job 2' }))

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(2)
      expect(jobs.map((j) => j.id)).toContain(jobId1)
      expect(jobs.map((j) => j.id)).toContain(jobId2)
    })

    it('should update a job when applyJobEvent is called for the same job ID', () => {
      const runId = 12n
      const jobId = 103n

      // Create job
      runStore.applyJobEvent(
        createMockJobEvent({
          jobId,
          runId,
          name: 'Job',
          action: { type: 'Queued', data: { labels: ['label1'], steps: [] } },
        }),
      )

      // Update same job
      runStore.applyJobEvent(
        createMockJobEvent({
          jobId,
          runId,
          name: 'Job',
          startedAt: '2025-01-01T00:00:05Z',
          action: { type: 'InProgress', data: { labels: ['label1'], steps: [], runner: null } },
        }),
      )

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(1) // Should still be 1, not duplicated
      expect(jobs[0]!.status).toBe('InProgress')
      expect(jobs[0]!.startedAt).toBe('2025-01-01T00:00:05Z')
    })

    it('should preserve job fields when updating with null values', () => {
      const runId = 13n
      const jobId = 104n

      // Create with initial runner
      runStore.applyJobEvent(
        createMockJobEvent({
          jobId,
          runId,
          name: 'Job',
          startedAt: '2025-01-01T00:00:05Z',
          action: {
            type: 'InProgress',
            data: {
              labels: ['ubuntu-latest'],
              steps: [],
              runner: createMockRunner({ name: 'Runner1' }),
            },
          },
        }),
      )

      // Update with null runner (should preserve)
      runStore.applyJobEvent(
        createMockJobEvent({
          jobId,
          runId,
          name: 'Job',
          startedAt: '2025-01-01T00:00:05Z',
          completedAt: '2025-01-01T00:00:15Z',
          action: {
            type: 'Completed',
            data: { labels: ['ubuntu-latest'], steps: [], runner: null, conclusion: 'Success' },
          },
        }),
      )

      const jobs = runStore.jobsByRun.get(runId)!
      const job = jobs[0]!
      expect(job.status).toBe('Completed')
      // Runner preserved from prior event (backend uses .or() semantics)
      expect(job.runner).toEqual({ id: 1n, name: 'Runner1', groupName: null })
    })

    it('should handle duplicate job events without creating duplicates', () => {
      const runId = 71n
      const jobId = 710n
      const envelope = createMockJobEvent({ jobId, runId, name: 'Job' })

      // Apply same event twice
      runStore.applyJobEvent(envelope)
      expect(runStore.jobsByRun.get(runId)!.length).toBe(1)

      runStore.applyJobEvent(envelope)
      expect(runStore.jobsByRun.get(runId)!.length).toBe(1) // Still 1, not 2

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs[0]!.status).toBe('Queued')
    })
  })
})
