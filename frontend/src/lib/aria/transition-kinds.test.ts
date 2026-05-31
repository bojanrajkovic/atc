import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import { classifyEvent, VERB_BY_CONCLUSION } from './transition-kinds'

// ---------------------------------------------------------------------------
// Compile-time exhaustiveness check
// The tsd-style helpers below verify at the TypeScript level that
// VERB_BY_CONCLUSION's keyset exactly equals RunConclusion's variant set.
// ---------------------------------------------------------------------------
type Expect<T extends true> = T
type Equal<X, Y> =
  (<T>() => T extends X ? 1 : 2) extends <T>() => T extends Y ? 1 : 2 ? true : false
// If RunConclusion grows a new variant and VERB_BY_CONCLUSION doesn't cover it,
// this line will cause a tsc error. Exported so svelte-check's "declared but
// never used" rule treats it as part of the module's public surface.
export type _CheckExhaustive = Expect<Equal<keyof typeof VERB_BY_CONCLUSION, RunConclusion>>

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeRunCommittedEvent(action: { type: string; data?: unknown }): CommittedEvent {
  return {
    seq: 1n,
    event: {
      type: 'Run',
      data: {
        runId: 1n,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test',
        triggerEvent: 'push',
        displayTitle: 'Test run',
        htmlUrl: 'https://example.com',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        runAttempt: 1,
        // biome-ignore lint/suspicious/noExplicitAny: test fixture constructs off-shape values
        action: action as any,
      },
    },
  }
}

function makeJobCommittedEvent(): CommittedEvent {
  return {
    seq: 2n,
    event: {
      type: 'Job',
      data: {
        jobId: 10n,
        runId: 1n,
        org: 'org',
        repo: 'repo',
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

// ---------------------------------------------------------------------------
// VERB_BY_CONCLUSION: all 9 RunConclusion variants present
// ---------------------------------------------------------------------------

describe('VERB_BY_CONCLUSION', () => {
  const allConclusions: RunConclusion[] = [
    'Success',
    'Failure',
    'Cancelled',
    'TimedOut',
    'ActionRequired',
    'Stale',
    'Neutral',
    'Skipped',
    'StartupFailure',
  ]

  it('has an entry for every RunConclusion variant', () => {
    for (const conclusion of allConclusions) {
      expect(VERB_BY_CONCLUSION).toHaveProperty(conclusion)
      expect(typeof VERB_BY_CONCLUSION[conclusion]).toBe('string')
      expect(VERB_BY_CONCLUSION[conclusion].length).toBeGreaterThan(0)
    }
  })

  it('has exactly 9 entries (no extras)', () => {
    expect(Object.keys(VERB_BY_CONCLUSION)).toHaveLength(9)
  })
})

// ---------------------------------------------------------------------------
// classifyEvent: non-announcement events return null
// ---------------------------------------------------------------------------

describe('classifyEvent', () => {
  describe('non-announcement events return null', () => {
    it('returns null for InProgress RunEvent', () => {
      const event = makeRunCommittedEvent({ type: 'InProgress' })
      expect(classifyEvent(event)).toBeNull()
    })

    it('returns null for Job events', () => {
      const event = makeJobCommittedEvent()
      expect(classifyEvent(event)).toBeNull()
    })
  })

  // ---------------------------------------------------------------------------
  // classifyEvent: Requested → queued
  // ---------------------------------------------------------------------------

  describe('Requested events', () => {
    it('returns {kind:"queued"} for a Requested RunEvent', () => {
      const event = makeRunCommittedEvent({ type: 'Requested' })
      const result = classifyEvent(event)
      expect(result).not.toBeNull()
      expect(result?.kind).toBe('queued')
    })

    it('result satisfies TransitionKind type narrowing', () => {
      const event = makeRunCommittedEvent({ type: 'Requested' })
      const result = classifyEvent(event)
      if (result === null) throw new Error('Expected non-null')
      // Narrow to queued branch
      if (result.kind === 'queued') {
        // No extra fields
        expect(Object.keys(result)).toEqual(['kind'])
      } else {
        throw new Error('Expected queued kind')
      }
    })
  })

  // ---------------------------------------------------------------------------
  // classifyEvent: Completed → completed with conclusion
  // ---------------------------------------------------------------------------

  describe('Completed events', () => {
    it('returns {kind:"completed", conclusion:"Success"} for a Completed/Success RunEvent', () => {
      const event = makeRunCommittedEvent({ type: 'Completed', data: { conclusion: 'Success' } })
      const result = classifyEvent(event)
      expect(result).not.toBeNull()
      expect(result?.kind).toBe('completed')
      if (result?.kind === 'completed') {
        expect(result.conclusion).toBe('Success')
      }
    })

    it('returns the correct conclusion for every RunConclusion variant', () => {
      const allConclusions: RunConclusion[] = [
        'Success',
        'Failure',
        'Cancelled',
        'TimedOut',
        'ActionRequired',
        'Stale',
        'Neutral',
        'Skipped',
        'StartupFailure',
      ]
      for (const conclusion of allConclusions) {
        const event = makeRunCommittedEvent({ type: 'Completed', data: { conclusion } })
        const result = classifyEvent(event)
        expect(result?.kind).toBe('completed')
        if (result?.kind === 'completed') {
          expect(result.conclusion).toBe(conclusion)
        }
      }
    })
  })

  // ---------------------------------------------------------------------------
  // classifyEvent: throws on invariant violation
  // ---------------------------------------------------------------------------

  describe('throws on invariant violation', () => {
    it('throws when Completed RunEvent has conclusion === null', () => {
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
      const event = makeRunCommittedEvent({ type: 'Completed', data: { conclusion: null } as any })
      expect(() => classifyEvent(event)).toThrow()
    })

    it('throws when Completed RunEvent has conclusion === undefined', () => {
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
      const event = makeRunCommittedEvent({ type: 'Completed', data: {} as any })
      expect(() => classifyEvent(event)).toThrow()
    })
  })

  // ---------------------------------------------------------------------------
  // classifyEvent: off-shape string conclusions — warn once and return null
  // ---------------------------------------------------------------------------

  describe('off-shape string conclusions', () => {
    let warnSpy: ReturnType<typeof vi.spyOn>

    beforeEach(() => {
      warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {})
    })

    afterEach(() => {
      warnSpy.mockRestore()
    })

    it('returns null for lowercase "success" (canonical off-shape case)', () => {
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
      const event = makeRunCommittedEvent({
        type: 'Completed',
        data: { conclusion: 'success' } as any,
      })
      const result = classifyEvent(event)
      expect(result).toBeNull()
    })

    it('emits console.warn with the unknown value and does not warn again (dedupe)', () => {
      // Use a unique value so module-scope dedup state from other tests doesn't interfere.
      const event = makeRunCommittedEvent({
        type: 'Completed',
        // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input
        data: { conclusion: 'off_shape_unique' } as any,
      })
      // First call: should warn
      classifyEvent(event)
      expect(warnSpy).toHaveBeenCalledOnce()
      expect(warnSpy).toHaveBeenCalledWith(expect.stringContaining('"off_shape_unique"'))
      // Second call with the same unknown value: dedupe must suppress the warning
      warnSpy.mockClear()
      classifyEvent(event)
      expect(warnSpy).not.toHaveBeenCalled()
    })

    it('valid PascalCase conclusions still classify correctly (no regression)', () => {
      // biome-ignore lint/suspicious/noExplicitAny: testing off-shape input — cast needed for the test fixture
      const event = makeRunCommittedEvent({
        type: 'Completed',
        data: { conclusion: 'Success' } as any,
      })
      const result = classifyEvent(event)
      expect(result).not.toBeNull()
      expect(result?.kind).toBe('completed')
      if (result?.kind === 'completed') {
        expect(result.conclusion).toBe('Success')
      }
    })
  })
})
