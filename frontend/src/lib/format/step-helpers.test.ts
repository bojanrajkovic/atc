import { describe, expect, it } from 'vitest'
import { createMockStep } from '$lib/test-utils/factories'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import { computeStepDurationText, computeStepStatusKey } from './step-helpers'

// ---------------------------------------------------------------------------
// computeStepStatusKey
// ---------------------------------------------------------------------------

describe('computeStepStatusKey', () => {
  describe('Queued status', () => {
    it('returns Queued for a queued step', () => {
      const step = createMockStep()
      expect(computeStepStatusKey(step)).toBe('Queued')
    })
  })

  describe('InProgress status', () => {
    it('returns InProgress for an in-progress step', () => {
      const step = createMockStep({ status: 'InProgress' })
      expect(computeStepStatusKey(step)).toBe('InProgress')
    })
  })

  describe('Completed status — maps conclusion to StatusKey', () => {
    it.each([
      ['Success', 'Success'],
      ['Failure', 'Failure'],
      ['Cancelled', 'Cancelled'],
      ['TimedOut', 'TimedOut'],
      ['ActionRequired', 'ActionRequired'],
      ['Stale', 'Stale'],
      ['Neutral', 'Neutral'],
      ['Skipped', 'Skipped'],
    ] as const satisfies ReadonlyArray<
      [JobConclusion, string]
    >)('returns %s for conclusion=%s', (conclusion, expected) => {
      const step = createMockStep({ status: 'Completed', conclusion })
      expect(computeStepStatusKey(step)).toBe(expected)
    })

    it('returns Cancelled for bare-Completed (null conclusion) — same fallback as job', () => {
      const step = createMockStep({ status: 'Completed', conclusion: null })
      expect(computeStepStatusKey(step)).toBe('Cancelled')
    })
  })

  describe('exhaustiveness defense at runtime', () => {
    it('throws when an unknown conclusion value is encountered at the boundary', () => {
      expect(() =>
        computeStepStatusKey(
          createMockStep({ status: 'Completed', conclusion: 'failure' as JobConclusion }),
        ),
      ).toThrow(/unhandled.*conclusion/i)
    })
  })
})

// ---------------------------------------------------------------------------
// computeStepDurationText
// ---------------------------------------------------------------------------

describe('computeStepDurationText', () => {
  it('returns em dash when both startedAt and completedAt are null', () => {
    const step = createMockStep({ startedAt: null, completedAt: null })
    expect(computeStepDurationText(step)).toBe('—')
  })

  it('returns em dash when only startedAt is null', () => {
    const step = createMockStep({ startedAt: null, completedAt: '2026-04-17T10:00:00Z' })
    expect(computeStepDurationText(step)).toBe('—')
  })

  it('returns em dash when only completedAt is null (incomplete step)', () => {
    const step = createMockStep({ startedAt: '2026-04-17T09:58:00Z', completedAt: null })
    expect(computeStepDurationText(step)).toBe('—')
  })

  it('returns static MM:SS when both startedAt and completedAt are present', () => {
    const step = createMockStep({
      startedAt: '2026-04-17T09:58:00Z',
      completedAt: '2026-04-17T10:00:00Z',
    })
    expect(computeStepDurationText(step)).toBe('2:00')
  })

  it('returns H:MM:SS for durations over 1 hour', () => {
    const step = createMockStep({
      startedAt: '2026-04-17T08:00:00Z',
      completedAt: '2026-04-17T09:30:45Z',
    })
    expect(computeStepDurationText(step)).toBe('1:30:45')
  })

  it('is stable — same output on repeated calls with same input', () => {
    const step = createMockStep({
      startedAt: '2026-04-17T09:58:00Z',
      completedAt: '2026-04-17T10:00:14Z',
    })
    expect(computeStepDurationText(step)).toBe('2:14')
    expect(computeStepDurationText(step)).toBe('2:14')
  })
})
