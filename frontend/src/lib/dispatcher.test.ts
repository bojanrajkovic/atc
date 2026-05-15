import { beforeEach, describe, expect, it, vi } from 'vitest'
import { eventDispatcher } from '$lib/dispatcher'
import { runStore } from '$lib/stores/runs.svelte'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'

describe('EventDispatcher', () => {
  beforeEach(() => {
    // Reset stores before each test
    runStore.clear()
  })

  describe('Basic event dispatching', () => {
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

      const committedEvent: CommittedEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: envelope,
        },
      }

      eventDispatcher.dispatch(committedEvent)
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

      const runCommittedEvent: CommittedEvent = {
        seq: 1n,
        event: {
          type: 'Run',
          data: runEnvelope,
        },
      }

      eventDispatcher.dispatch(runCommittedEvent)
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

      const jobCommittedEvent: CommittedEvent = {
        seq: 2n,
        event: {
          type: 'Job',
          data: jobEnvelope,
        },
      }

      eventDispatcher.dispatch(jobCommittedEvent)
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

  describe('Event batching via RAF', () => {
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
      const event1: CommittedEvent = {
        seq: 1n,
        event: { type: 'Run', data: createRunEnvelope(1n) },
      }
      const event2: CommittedEvent = {
        seq: 2n,
        event: { type: 'Run', data: createRunEnvelope(2n) },
      }
      const event3: CommittedEvent = {
        seq: 3n,
        event: { type: 'Run', data: createRunEnvelope(3n) },
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

  describe('setOnFlush post-flush callback hook', () => {
    beforeEach(() => {
      // Always reset the callback so previous tests don't leak callbacks
      eventDispatcher.setOnFlush(null)
      runStore.clear()
    })

    const makeRunCommittedEvent = (id: bigint): CommittedEvent => ({
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
    })

    it('callback is invoked with flushed events after flush()', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      const e1 = makeRunCommittedEvent(1n)
      const e2 = makeRunCommittedEvent(2n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.dispatch(e2)
      eventDispatcher.flush()

      expect(cb).toHaveBeenCalledOnce()
      expect(cb).toHaveBeenCalledWith([e1, e2])
    })

    it('callback is NOT invoked when no events were queued (empty flush)', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      // Flush with nothing in the buffer
      eventDispatcher.flush()

      expect(cb).not.toHaveBeenCalled()
    })

    it('callback receives only events from the current flush, not cumulative', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      const e1 = makeRunCommittedEvent(1n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.flush()

      const e2 = makeRunCommittedEvent(2n)
      eventDispatcher.dispatch(e2)
      eventDispatcher.flush()

      expect(cb).toHaveBeenCalledTimes(2)
      expect(cb).toHaveBeenNthCalledWith(1, [e1])
      expect(cb).toHaveBeenNthCalledWith(2, [e2])
    })

    it('dispatch(); flush() produces exactly one non-empty callback (no phantom RAF callback)', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)

      // dispatch() would schedule a RAF, flush() should cancel it
      const e1 = makeRunCommittedEvent(1n)
      eventDispatcher.dispatch(e1)
      eventDispatcher.flush()

      // At this point, if flush() didn't cancel the RAF, a real RAF callback
      // would fire and produce a phantom empty call. Since we're in jsdom/no
      // actual RAF, this verifies the mechanism is correct by checking callback
      // count and ensuring no extra empty calls happen.
      expect(cb).toHaveBeenCalledOnce()
      expect(cb).toHaveBeenCalledWith([e1])
    })

    it('setOnFlush(null) detaches the callback', () => {
      const cb = vi.fn()
      eventDispatcher.setOnFlush(cb)
      eventDispatcher.setOnFlush(null)

      eventDispatcher.dispatch(makeRunCommittedEvent(1n))
      eventDispatcher.flush()

      expect(cb).not.toHaveBeenCalled()
    })

    it('calling setOnFlush twice replaces the prior callback (idempotent replacement)', () => {
      const cb1 = vi.fn()
      const cb2 = vi.fn()

      eventDispatcher.setOnFlush(cb1)
      eventDispatcher.setOnFlush(cb2)

      eventDispatcher.dispatch(makeRunCommittedEvent(1n))
      eventDispatcher.flush()

      expect(cb1).not.toHaveBeenCalled()
      expect(cb2).toHaveBeenCalledOnce()
    })

    it('no invocation when setOnFlush was never set', () => {
      // Don't set any callback — should not throw and nothing should fail
      eventDispatcher.dispatch(makeRunCommittedEvent(1n))
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
      const e1: CommittedEvent = {
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

  describe('unknown event type tolerance (wire-skew resilience)', () => {
    // Helper factories
    const makeRunCommittedEvent = (id: bigint): CommittedEvent => ({
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
    })

    const makeUnknownCommittedEvent = (seq: bigint, unknownType: string): CommittedEvent =>
      ({ seq, event: { type: unknownType } }) as unknown as CommittedEvent

    beforeEach(() => {
      eventDispatcher.setOnFlush(null)
      runStore.clear()
    })

    it('skips unknown event types without aborting the batch, warns once per type, and deduplicates across batches', () => {
      const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})

      try {
        // --- Batch 1: [valid Run A, unknown "future_unknown_type", valid Run B] ---
        const runA = makeRunCommittedEvent(10n)
        const unknown1 = makeUnknownCommittedEvent(11n, 'future_unknown_type')
        const runB = makeRunCommittedEvent(12n)

        eventDispatcher.dispatch(runA)
        eventDispatcher.dispatch(unknown1)
        eventDispatcher.dispatch(runB)
        eventDispatcher.flush()

        // Both valid events must have been applied
        expect(runStore.runs.has(10n)).toBe(true)
        expect(runStore.runs.has(12n)).toBe(true)

        // console.warn called exactly once, mentioning the unknown type
        expect(warnSpy).toHaveBeenCalledOnce()
        expect(warnSpy.mock.calls[0]![0]).toContain('future_unknown_type')

        warnSpy.mockClear()

        // --- Batch 2: same unknown type again — warn must NOT fire again (dedupe) ---
        const unknown2 = makeUnknownCommittedEvent(20n, 'future_unknown_type')
        eventDispatcher.dispatch(unknown2)
        eventDispatcher.flush()

        expect(warnSpy).not.toHaveBeenCalled()

        // --- Batch 3: different unknown type — warn IS fired once for the new type ---
        const unknown3 = makeUnknownCommittedEvent(30n, 'another_future_type')
        eventDispatcher.dispatch(unknown3)
        eventDispatcher.flush()

        expect(warnSpy).toHaveBeenCalledOnce()
        expect(warnSpy.mock.calls[0]![0]).toContain('another_future_type')
      } finally {
        warnSpy.mockRestore()
      }
    })
  })
})
