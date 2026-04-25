import { beforeEach, describe, expect, it, vi } from 'vitest'
import { eventDispatcher } from '$lib/dispatcher'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

describe('EventDispatcher', () => {
  beforeEach(() => {
    // Reset stores before each test
    runStore.clear()
  })

  describe('fe-foundation.AC4.2: Basic event dispatching', () => {
    it('dispatches a Run event to the store', () => {
      // Create a minimal RunEventEnvelope
      const envelope: RunEventEnvelope = {
        runId: 1n,
        org: 'org',
        repo: 'repo',
        workflowName: 'test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: 'Test run',
        htmlUrl: 'https://github.com/org/repo/actions/runs/1',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Requested',
        },
      }

      const seqEvent: SeqEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: envelope,
        },
        poolStatsAfter: null,
      }

      eventDispatcher.dispatch(seqEvent)
      eventDispatcher.flush()

      // Verify the run appeared in the store
      expect(runStore.runs.has(1n)).toBe(true)
      const run = runStore.runs.get(1n)
      expect(run?.status).toBe('Queued')
    })

    it('dispatches a Job event to the store', () => {
      // First create a run for the job to belong to
      const runEnvelope: RunEventEnvelope = {
        runId: 1n,
        org: 'org',
        repo: 'repo',
        workflowName: 'test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: 'Test run',
        htmlUrl: 'https://github.com/org/repo/actions/runs/1',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Requested',
        },
      }

      const runSeqEvent: SeqEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: runEnvelope,
        },
        poolStatsAfter: null,
      }

      eventDispatcher.dispatch(runSeqEvent)
      eventDispatcher.flush()

      // Now dispatch a job event
      const jobEnvelope: JobEventEnvelope = {
        jobId: 100n,
        runId: 1n,
        org: 'org',
        repo: 'repo',
        name: 'test-job',
        createdAt: new Date().toISOString(),
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: {
            labels: [],
            steps: [],
          },
        },
      }

      const jobSeqEvent: SeqEvent = {
        seq: 2n,
        event: {
          type: 'Job',
          data: jobEnvelope,
        },
        poolStatsAfter: null,
      }

      eventDispatcher.dispatch(jobSeqEvent)
      eventDispatcher.flush()

      // Verify the job appeared in the store
      const jobs = runStore.jobsByRun.get(1n)
      expect(jobs).toBeDefined()
      if (!jobs) return
      expect(jobs.length).toBe(1)
      expect(jobs[0]?.id).toBe(100n)
      expect(jobs[0]?.status).toBe('Queued')
    })
  })

  describe('fe-foundation.AC4.3: Event batching via RAF', () => {
    it('batches multiple events dispatched rapidly into a single flush', () => {
      const applyRunEventSpy = vi.spyOn(runStore, 'applyRunEvent')

      // Create 3 run envelopes
      const createRunEnvelope = (id: bigint): RunEventEnvelope => ({
        runId: id,
        org: 'org',
        repo: 'repo',
        workflowName: 'test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: 'Test run',
        htmlUrl: 'https://github.com/org/repo/actions/runs/1',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Requested',
        },
      })

      // Dispatch 3 events rapidly without flushing between
      const event1: SeqEvent = {
        seq: 1n,
        event: { type: 'Run', data: createRunEnvelope(1n) },
        poolStatsAfter: null,
      }
      const event2: SeqEvent = {
        seq: 2n,
        event: { type: 'Run', data: createRunEnvelope(2n) },
        poolStatsAfter: null,
      }
      const event3: SeqEvent = {
        seq: 3n,
        event: { type: 'Run', data: createRunEnvelope(3n) },
        poolStatsAfter: null,
      }

      eventDispatcher.dispatch(event1)
      eventDispatcher.dispatch(event2)
      eventDispatcher.dispatch(event3)

      // Before flush, applyRunEvent should not have been called yet
      // (RAF hasn't fired in our test)
      expect(applyRunEventSpy).not.toHaveBeenCalled()

      // Now flush
      eventDispatcher.flush()

      // Verify all 3 events were processed
      expect(applyRunEventSpy).toHaveBeenCalledTimes(3)
      expect(runStore.runs.size).toBe(3)
      expect(runStore.runs.has(1n)).toBe(true)
      expect(runStore.runs.has(2n)).toBe(true)
      expect(runStore.runs.has(3n)).toBe(true)

      applyRunEventSpy.mockRestore()
    })
  })

  describe('AC2: pool stats sidecar', () => {
    it('AC2.1 — populated sidecar triggers loadPools with exact payload', () => {
      const loadPoolsSpy = vi.spyOn(runnerStore, 'loadPools')
      const applyJobEventSpy = vi.spyOn(runStore, 'applyJobEvent')

      // Create a job envelope and pool stats payload
      const jobEnvelope: JobEventEnvelope = {
        jobId: 100n,
        runId: 1n,
        org: 'org',
        repo: 'repo',
        name: 'test-job',
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
      }

      const poolStats = [
        {
          labels: ['ubuntu-latest'],
          queued: 1,
          running: 0,
          groupName: 'GitHub Actions',
          isElastic: true,
          total: null,
        },
      ]

      const seqEvent: SeqEvent = {
        seq: 1n,
        event: {
          type: 'Job',
          data: jobEnvelope,
        },
        poolStatsAfter: poolStats,
      }

      eventDispatcher.dispatch(seqEvent)
      eventDispatcher.flush()

      expect(applyJobEventSpy).toHaveBeenCalledOnce()
      expect(loadPoolsSpy).toHaveBeenCalledOnce()
      expect(loadPoolsSpy).toHaveBeenCalledWith(poolStats)

      loadPoolsSpy.mockRestore()
      applyJobEventSpy.mockRestore()
    })

    it('AC2.2 — null sidecar does not trigger loadPools', () => {
      const loadPoolsSpy = vi.spyOn(runnerStore, 'loadPools')
      const applyRunEventSpy = vi.spyOn(runStore, 'applyRunEvent')

      const runEnvelope: RunEventEnvelope = {
        runId: 1n,
        org: 'org',
        repo: 'repo',
        workflowName: 'test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: 'Test run',
        htmlUrl: 'https://github.com/org/repo/actions/runs/1',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Requested',
        },
      }

      const seqEvent: SeqEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: runEnvelope,
        },
        poolStatsAfter: null,
      }

      eventDispatcher.dispatch(seqEvent)
      eventDispatcher.flush()

      expect(applyRunEventSpy).toHaveBeenCalledOnce()
      expect(loadPoolsSpy).not.toHaveBeenCalled()

      loadPoolsSpy.mockRestore()
      applyRunEventSpy.mockRestore()
    })

    it('AC2.5 — RAF-batched flush applies all populated sidecars in dispatch order', () => {
      const loadPoolsSpy = vi.spyOn(runnerStore, 'loadPools')
      const applyJobEventSpy = vi.spyOn(runStore, 'applyJobEvent')

      // Create three distinct pool stats payloads
      const poolsP1 = [
        {
          labels: ['ubuntu-latest'],
          queued: 1,
          running: 0,
          groupName: 'GitHub Actions',
          isElastic: true,
          total: null,
        },
      ]

      const poolsP2 = [
        {
          labels: ['ubuntu-latest'],
          queued: 0,
          running: 1,
          groupName: 'GitHub Actions',
          isElastic: true,
          total: null,
        },
      ]

      const poolsP3: typeof poolsP1 = []

      const makeJobEnvelope = (id: bigint): JobEventEnvelope => ({
        jobId: id,
        runId: 1n,
        org: 'org',
        repo: 'repo',
        name: `test-job-${id}`,
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
      })

      // Dispatch three events with distinct sidecars
      const event1: SeqEvent = {
        seq: 1n,
        event: { type: 'Job', data: makeJobEnvelope(1n) },
        poolStatsAfter: poolsP1,
      }

      const event2: SeqEvent = {
        seq: 2n,
        event: { type: 'Job', data: makeJobEnvelope(2n) },
        poolStatsAfter: poolsP2,
      }

      const event3: SeqEvent = {
        seq: 3n,
        event: { type: 'Job', data: makeJobEnvelope(3n) },
        poolStatsAfter: poolsP3,
      }

      eventDispatcher.dispatch(event1)
      eventDispatcher.dispatch(event2)
      eventDispatcher.dispatch(event3)

      eventDispatcher.flush()

      expect(applyJobEventSpy).toHaveBeenCalledTimes(3)
      expect(loadPoolsSpy).toHaveBeenCalledTimes(3)
      expect(loadPoolsSpy).toHaveBeenNthCalledWith(1, poolsP1)
      expect(loadPoolsSpy).toHaveBeenNthCalledWith(2, poolsP2)
      expect(loadPoolsSpy).toHaveBeenNthCalledWith(3, poolsP3)

      loadPoolsSpy.mockRestore()
      applyJobEventSpy.mockRestore()
    })
  })
})
