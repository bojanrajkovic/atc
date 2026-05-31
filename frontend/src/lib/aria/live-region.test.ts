import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { runStore } from '$lib/stores/runs.svelte'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { LiveRegion } from './live-region.svelte'

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

let seqCounter = 0

function makeRunCommittedEvent(opts: {
  runId: bigint
  action: 'Requested' | 'InProgress' | { Completed: { conclusion: string } }
  displayTitle?: string
  branch?: string | null
  org?: string
  repo?: string
}): CommittedEvent {
  seqCounter++
  let actionPayload: { type: string; data?: unknown }
  if (opts.action === 'Requested') {
    actionPayload = { type: 'Requested' }
  } else if (opts.action === 'InProgress') {
    actionPayload = { type: 'InProgress' }
  } else {
    actionPayload = { type: 'Completed', data: { conclusion: opts.action.Completed.conclusion } }
  }

  return {
    seq: BigInt(seqCounter),
    event: {
      type: 'Run',
      data: {
        runId: opts.runId,
        org: opts.org ?? 'test-org',
        repo: opts.repo ?? 'test-repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: opts.branch === undefined ? 'main' : opts.branch,
        headSha: 'abc123',
        commitMessage: 'test',
        triggerEvent: 'push',
        displayTitle: opts.displayTitle ?? `Run ${opts.runId}`,
        htmlUrl: 'https://example.com',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        runAttempt: 1,
        // biome-ignore lint/suspicious/noExplicitAny: test fixture
        action: actionPayload as any,
      },
    },
  }
}

function makeJobCommittedEvent(runId: bigint): CommittedEvent {
  seqCounter++
  return {
    seq: BigInt(seqCounter),
    event: {
      type: 'Job',
      data: {
        jobId: BigInt(seqCounter) * 100n,
        runId,
        org: 'test-org',
        repo: 'test-repo',
        name: 'test-job',
        createdAt: new Date().toISOString(),
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      },
    },
  }
}

// Populate the run store with a run so formatRunTransition can look it up
function setupRun(runId: bigint, opts: Partial<WorkflowRun> = {}): void {
  // Dispatch a Requested event to create the run in the store
  runStore.applyRunEvent({
    runId,
    org: opts.org ?? 'test-org',
    repo: opts.repo ?? 'test-repo',
    workflowName: 'CI',
    workflowPath: null,
    branch: opts.branch === undefined ? 'main' : opts.branch,
    headSha: 'abc123',
    commitMessage: 'test',
    triggerEvent: 'push',
    displayTitle: opts.displayTitle ?? `Run ${runId}`,
    htmlUrl: 'https://example.com',
    createdAt: new Date().toISOString(),
    runStartedAt: null,
    updatedAt: new Date().toISOString(),
    runAttempt: 1,
    action: { type: 'Requested' },
  })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('LiveRegion', () => {
  let liveRegion: LiveRegion

  beforeEach(() => {
    vi.useFakeTimers()
    seqCounter = 0
    runStore.clear()
    liveRegion = new LiveRegion()
  })

  afterEach(() => {
    vi.useRealTimers()
    runStore.clear()
  })

  // ---------------------------------------------------------------------------
  // Event walking: Queued/Completed counted; InProgress skipped
  // ---------------------------------------------------------------------------

  describe('observeFlush event walking', () => {
    it('counts a Requested (queued) transition as 1 announcement', () => {
      setupRun(1n)
      liveRegion.observeFlush([makeRunCommittedEvent({ runId: 1n, action: 'Requested' })])
      expect(liveRegion.message).toContain('queued')
      expect(liveRegion.busy).toBe(false)
    })

    it('counts a Completed transition as 1 announcement', () => {
      setupRun(1n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: { Completed: { conclusion: 'Success' } } }),
      ])
      expect(liveRegion.message).toContain('succeeded')
      expect(liveRegion.busy).toBe(false)
    })

    it('skips InProgress events (no announcement)', () => {
      setupRun(1n)
      liveRegion.observeFlush([makeRunCommittedEvent({ runId: 1n, action: 'InProgress' })])
      expect(liveRegion.message).toBe('')
      expect(liveRegion.busy).toBe(false)
    })

    it('skips Job events (no announcement)', () => {
      liveRegion.observeFlush([makeJobCommittedEvent(1n)])
      expect(liveRegion.message).toBe('')
      expect(liveRegion.busy).toBe(false)
    })

    it('same-run Queued+Completed in one flush produces 2 per-run announcements joined by ". "', () => {
      setupRun(1n)
      // After Requested the run is in store; then Completed updates it
      runStore.applyRunEvent({
        runId: 1n,
        org: 'test-org',
        repo: 'test-repo',
        workflowName: 'CI',
        workflowPath: null,
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test',
        triggerEvent: 'push',
        displayTitle: 'Run 1',
        htmlUrl: 'https://example.com',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        runAttempt: 1,
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      })

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 1n, action: { Completed: { conclusion: 'Success' } } }),
      ])

      expect(liveRegion.message).toContain('queued')
      expect(liveRegion.message).toContain('succeeded')
      expect(liveRegion.message).toContain('. ')
      expect(liveRegion.busy).toBe(false)
    })

    it('2 runs (Requested + Completed) in one flush below threshold → 2 per-run messages', () => {
      setupRun(1n)
      setupRun(2n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: { Completed: { conclusion: 'Failure' } } }),
      ])
      expect(liveRegion.message).toContain('queued')
      expect(liveRegion.message).toContain('failed')
    })

    it('3 transitions in one flush stay below threshold → per-run form', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
      ])
      expect(liveRegion.message).toContain('queued')
      expect(liveRegion.busy).toBe(false)
      // Per-run form: no summary "N runs queued"
      expect(liveRegion.message).not.toMatch(/\d+ runs queued/)
    })
  })

  // ---------------------------------------------------------------------------
  // Burst threshold: >3 transitions → summary form
  // ---------------------------------------------------------------------------

  describe('burst threshold and BurstAccumulator', () => {
    it('4+ transitions in a flush opens burst (aria-busy=true)', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])
      expect(liveRegion.busy).toBe(true)
    })

    it('after 200ms debounce, aria-busy flips back to false and summary is set', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])

      expect(liveRegion.busy).toBe(true)

      vi.advanceTimersByTime(200)

      expect(liveRegion.busy).toBe(false)
      expect(liveRegion.message).toContain('4 runs queued')
    })

    it('subsequent flush within debounce window adds to accumulated counts', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)
      setupRun(5n)
      setupRun(6n)

      // 4-transition flush opens burst
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])

      // 2-transition flush within window — also added to burst even though <3
      vi.advanceTimersByTime(100)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 5n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 6n, action: 'Requested' }),
      ])

      vi.advanceTimersByTime(200)

      // Should say 6 runs queued
      expect(liveRegion.message).toContain('6 runs queued')
      expect(liveRegion.busy).toBe(false)
    })

    it('summary includes completed breakdown (succeeded/failed)', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)
      setupRun(5n)

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 3n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 4n, action: { Completed: { conclusion: 'Failure' } } }),
        makeRunCommittedEvent({ runId: 5n, action: { Completed: { conclusion: 'Failure' } } }),
      ])

      vi.advanceTimersByTime(200)

      expect(liveRegion.message).toContain('1 run queued')
      expect(liveRegion.message).toContain('4 completed')
      expect(liveRegion.message).toContain('succeeded')
      expect(liveRegion.message).toContain('failed')
      expect(liveRegion.busy).toBe(false)
    })

    it('summary elides absent conclusion counts', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 2n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 3n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 4n, action: { Completed: { conclusion: 'Success' } } }),
      ])

      vi.advanceTimersByTime(200)

      // No cancellations — should not appear
      expect(liveRegion.message).not.toContain('cancelled')
      expect(liveRegion.message).not.toContain('failed')
      expect(liveRegion.busy).toBe(false)
    })

    it('non-overlapping bursts: second burst after first closes is independent', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)

      // First burst
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])
      vi.advanceTimersByTime(200)

      expect(liveRegion.message).toContain('4 runs queued')
      expect(liveRegion.busy).toBe(false)

      // Second burst — independent counts
      setupRun(5n)
      setupRun(6n)
      setupRun(7n)
      setupRun(8n)

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 5n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 6n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 7n, action: { Completed: { conclusion: 'Success' } } }),
        makeRunCommittedEvent({ runId: 8n, action: { Completed: { conclusion: 'Success' } } }),
      ])
      vi.advanceTimersByTime(200)

      expect(liveRegion.message).not.toContain('queued') // first burst counts don't carry over
      expect(liveRegion.message).toContain('4 completed')
      expect(liveRegion.busy).toBe(false)
    })
  })

  // ---------------------------------------------------------------------------
  // Per-event error containment
  // ---------------------------------------------------------------------------

  describe('per-event error containment', () => {
    it('a bad event (Completed with null conclusion) does not kill the rest of the batch', () => {
      setupRun(1n)
      setupRun(2n)
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

      const badEvent = makeRunCommittedEvent({
        runId: 1n,
        action: { Completed: { conclusion: 'null_conclusion' as 'Success' } },
      })
      // Make it have null conclusion by overriding the data
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
      ;(badEvent.event as any).data.action = { type: 'Completed', data: { conclusion: null } }

      liveRegion.observeFlush([badEvent, makeRunCommittedEvent({ runId: 2n, action: 'Requested' })])

      // The good event should still be announced
      expect(liveRegion.message).toContain('queued')
      // Error was logged
      expect(consoleErrorSpy).toHaveBeenCalled()

      consoleErrorSpy.mockRestore()
    })

    it('logs the offending event payload on error', () => {
      const consoleErrorSpy = vi.spyOn(console, 'error').mockImplementation(() => {})

      const badEvent = makeRunCommittedEvent({
        runId: 99n,
        action: 'Requested',
      })
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
      ;(badEvent.event as any).data.action = { type: 'Completed', data: { conclusion: null } }

      liveRegion.observeFlush([badEvent])

      expect(consoleErrorSpy).toHaveBeenCalledWith(
        expect.stringContaining('classifyEvent'),
        badEvent,
      )

      consoleErrorSpy.mockRestore()
    })
  })

  // ---------------------------------------------------------------------------
  // aria-busy true→false transition
  // ---------------------------------------------------------------------------

  describe('aria-busy transitions', () => {
    it('starts with busy=false', () => {
      expect(liveRegion.busy).toBe(false)
    })

    it('flips busy to true when burst opens, and back to false on debounce', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])

      expect(liveRegion.busy).toBe(true)
      vi.advanceTimersByTime(200)
      expect(liveRegion.busy).toBe(false)
    })
  })

  // ---------------------------------------------------------------------------
  // cancelBurst — disconnect/reconnect cleanup hook (Codex P2)
  // ---------------------------------------------------------------------------

  describe('cancelBurst', () => {
    it('cancels pending debounce timer so closeBurst does not fire', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)

      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])
      // Burst is open; debounce has not fired yet (timers are faked)
      expect(liveRegion.busy).toBe(true)
      const messageBeforeCancel = liveRegion.message

      liveRegion.cancelBurst()

      // busy was dropped immediately; message untouched
      expect(liveRegion.busy).toBe(false)
      expect(liveRegion.message).toBe(messageBeforeCancel)

      // Advance past the debounce window — the canceled timer must not fire
      vi.advanceTimersByTime(500)
      expect(liveRegion.message).toBe(messageBeforeCancel)
      expect(liveRegion.message).not.toMatch(/\d+ runs queued/)
    })

    it('after cancelBurst, a fresh burst can open normally (state fully reset)', () => {
      setupRun(1n)
      setupRun(2n)
      setupRun(3n)
      setupRun(4n)

      // Open burst, cancel it
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 1n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 2n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 3n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 4n, action: 'Requested' }),
      ])
      liveRegion.cancelBurst()

      // Open a fresh burst — accumulator must be empty so summary reflects only new events
      setupRun(5n)
      setupRun(6n)
      setupRun(7n)
      setupRun(8n)
      liveRegion.observeFlush([
        makeRunCommittedEvent({ runId: 5n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 6n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 7n, action: 'Requested' }),
        makeRunCommittedEvent({ runId: 8n, action: 'Requested' }),
      ])
      vi.advanceTimersByTime(200)

      // Summary reflects only the second burst (4 runs), not the canceled first
      expect(liveRegion.message).toMatch(/^4 runs queued\.$/)
    })

    it('is a no-op when no burst is active', () => {
      expect(liveRegion.busy).toBe(false)
      const messageBefore = liveRegion.message

      liveRegion.cancelBurst()

      expect(liveRegion.busy).toBe(false)
      expect(liveRegion.message).toBe(messageBefore)
    })
  })
})
