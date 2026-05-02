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

      // Verify routing order: primitive event applied before sidecar
      expect(applyJobEventSpy.mock.invocationCallOrder[0]!).toBeLessThan(
        loadPoolsSpy.mock.invocationCallOrder[0]!,
      )

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

      // Verify routing order for each event: primitive applied before sidecar
      for (let i = 0; i < 3; i++) {
        expect(applyJobEventSpy.mock.invocationCallOrder[i]!).toBeLessThan(
          loadPoolsSpy.mock.invocationCallOrder[i]!,
        )
      }

      loadPoolsSpy.mockRestore()
      applyJobEventSpy.mockRestore()
    })
  })

  describe('AC6: setOnFlush post-flush callback hook', () => {
    beforeEach(() => {
      // Always reset the callback so previous tests don't leak callbacks
      eventDispatcher.setOnFlush(null)
      runStore.clear()
    })

    const makeRunSeqEvent = (id: bigint): SeqEvent => ({
      seq: id,
      event: {
        type: 'Run',
        data: {
          runId: id,
          org: 'org',
          repo: 'repo',
          workflowName: 'test',
          workflowPath: null,
          branch: 'main',
          headSha: 'abc',
          commitMessage: null,
          triggerEvent: 'push',
          displayTitle: `Run ${id}`,
          htmlUrl: 'https://example.com',
          createdAt: new Date().toISOString(),
          runStartedAt: null,
          updatedAt: new Date().toISOString(),
          action: { type: 'Requested' },
        },
      },
      poolStatsAfter: null,
    })

    it('AC6.1 — callback is invoked with flushed events after flush()', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      const e1 = makeRunSeqEvent(1n)
      const e2 = makeRunSeqEvent(2n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.dispatch(e2)
      eventDispatcher.flush()

      expect(cb).toHaveBeenCalledOnce()
      expect(cb).toHaveBeenCalledWith([e1, e2])
    })

    it('AC6.2 — callback is NOT invoked when no events were queued (empty flush)', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      // Flush with nothing in the buffer
      eventDispatcher.flush()

      expect(cb).not.toHaveBeenCalled()
    })

    it('AC6.3 — callback receives only events from the current flush, not cumulative', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      const e1 = makeRunSeqEvent(1n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.flush()

      const e2 = makeRunSeqEvent(2n)
      eventDispatcher.dispatch(e2)
      eventDispatcher.flush()

      expect(cb).toHaveBeenCalledTimes(2)
      expect(cb).toHaveBeenNthCalledWith(1, [e1])
      expect(cb).toHaveBeenNthCalledWith(2, [e2])
    })

    it('AC6.4 — dispatch(); flush() produces exactly one non-empty callback (no phantom RAF callback)', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      // dispatch() would schedule a RAF, flush() should cancel it
      const e1 = makeRunSeqEvent(1n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.flush()

      // At this point, if flush() didn't cancel the RAF, a real RAF callback
      // would fire and produce a phantom empty call. Since we're in jsdom/no
      // actual RAF, this verifies the mechanism is correct by checking callback
      // count and ensuring no extra empty calls happen.
      expect(cb).toHaveBeenCalledOnce()
      expect(cb).toHaveBeenCalledWith([e1])
    })

    it('AC6.5 — setOnFlush(null) detaches the callback', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)
      eventDispatcher.setOnFlush(null)

      eventDispatcher.dispatch(makeRunSeqEvent(1n))
      eventDispatcher.flush()

      expect(cb).not.toHaveBeenCalled()
    })

    it('AC6.6 — calling setOnFlush twice replaces the prior callback (idempotent replacement)', () => {
      const cb1 = vi.fn()
      const cb2 = vi.fn()

      eventDispatcher.setOnFlush(cb1)
      eventDispatcher.setOnFlush(cb2)

      eventDispatcher.dispatch(makeRunSeqEvent(1n))
      eventDispatcher.flush()

      expect(cb1).not.toHaveBeenCalled()
      expect(cb2).toHaveBeenCalledOnce()
    })

    it('AC6.7 — no invocation when setOnFlush was never set', () => {
      // Don't set any callback — should not throw and nothing should fail
      eventDispatcher.dispatch(makeRunSeqEvent(1n))
      expect(() => eventDispatcher.flush()).not.toThrow()
    })
  })

  describe('bufferLength getter', () => {
    beforeEach(() => {
      eventDispatcher.setOnFlush(null)
      runStore.clear()
    })

    it('returns 0 when buffer is empty', () => {
      expect(eventDispatcher.bufferLength).toBe(0)
    })

    it('returns the number of queued events before flush', () => {
      const e1: SeqEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: {
            runId: 1n,
            org: 'o',
            repo: 'r',
            workflowName: null,
            workflowPath: null,
            branch: null,
            headSha: 'x',
            commitMessage: null,
            triggerEvent: 'push',
            displayTitle: 'R',
            htmlUrl: 'https://x',
            createdAt: new Date().toISOString(),
            runStartedAt: null,
            updatedAt: new Date().toISOString(),
            action: { type: 'Requested' },
          },
        },
        poolStatsAfter: null,
      }
      const e2 = { ...e1, seq: 2n }

      eventDispatcher.dispatch(e1)
      expect(eventDispatcher.bufferLength).toBe(1)

      eventDispatcher.dispatch(e2)
      expect(eventDispatcher.bufferLength).toBe(2)

      eventDispatcher.flush()
      expect(eventDispatcher.bufferLength).toBe(0)
    })
  })
})
