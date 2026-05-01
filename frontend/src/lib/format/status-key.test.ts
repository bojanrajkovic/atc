import { describe, expect, it, vi } from 'vitest'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import {
  resolveJobStatusKey,
  resolveStatusKey,
  STATUS_KEYS,
  statusKeyToHumanLabel,
  statusKeyToVar,
} from './status-key'

describe('format/status-key', () => {
  describe('run-cards.AC6A.1: Queued status returns Queued key', () => {
    it('returns Queued when status is Queued and conclusion is null', () => {
      const result = resolveStatusKey({ status: 'Queued', conclusion: null })
      expect(result).toBe('Queued')
    })

    it('returns Queued when status is Queued, ignoring a theoretical conclusion value', () => {
      const result = resolveStatusKey({ status: 'Queued', conclusion: 'Success' })
      expect(result).toBe('Queued')
    })
  })

  describe('run-cards.AC6A.2: InProgress status returns InProgress key', () => {
    it('returns InProgress when status is InProgress and conclusion is null', () => {
      const result = resolveStatusKey({ status: 'InProgress', conclusion: null })
      expect(result).toBe('InProgress')
    })

    it('returns InProgress when status is InProgress, ignoring a theoretical conclusion value', () => {
      const result = resolveStatusKey({ status: 'InProgress', conclusion: 'Failure' })
      expect(result).toBe('InProgress')
    })
  })

  describe('run-cards.AC6A.3: Completed status with conclusion resolves to conclusion key', () => {
    it('returns Success when status is Completed and conclusion is Success', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Success' })
      expect(result).toBe('Success')
    })

    it('returns Failure when status is Completed and conclusion is Failure', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Failure' })
      expect(result).toBe('Failure')
    })

    it('returns Cancelled when status is Completed and conclusion is Cancelled', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Cancelled' })
      expect(result).toBe('Cancelled')
    })

    it('returns TimedOut when status is Completed and conclusion is TimedOut', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'TimedOut' })
      expect(result).toBe('TimedOut')
    })

    it('returns ActionRequired when status is Completed and conclusion is ActionRequired', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'ActionRequired' })
      expect(result).toBe('ActionRequired')
    })

    it('returns StartupFailure when status is Completed and conclusion is StartupFailure', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'StartupFailure' })
      expect(result).toBe('StartupFailure')
    })

    it('returns Stale when status is Completed and conclusion is Stale', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Stale' })
      expect(result).toBe('Stale')
    })

    it('returns Neutral when status is Completed and conclusion is Neutral', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Neutral' })
      expect(result).toBe('Neutral')
    })

    it('returns Skipped when status is Completed and conclusion is Skipped', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: 'Skipped' })
      expect(result).toBe('Skipped')
    })
  })

  describe('run-cards.AC6A.4: Bare-Completed fallback returns Cancelled', () => {
    it('returns Cancelled when status is Completed and conclusion is null', () => {
      const result = resolveStatusKey({ status: 'Completed', conclusion: null })
      // Bare-Completed fallback (see docs/design-plans/2026-04-17-run-cards.md, "StatusKey normalization at the boundary")
      expect(result).toBe('Cancelled')
    })
  })

  describe('run-cards.AC6A.5: Pure function with no side effects', () => {
    it('returns identical output when called twice with same input', () => {
      const input: Pick<WorkflowRun, 'status' | 'conclusion'> = {
        status: 'Completed',
        conclusion: 'Success',
      }
      const result1 = resolveStatusKey(input)
      const result2 = resolveStatusKey(input)
      expect(result1).toBe(result2)
    })

    it('does not mutate the input object', () => {
      const input: Pick<WorkflowRun, 'status' | 'conclusion'> = {
        status: 'Completed',
        conclusion: 'Failure',
      }
      Object.freeze(input)

      const result = resolveStatusKey(input)

      expect(result).toBe('Failure')
      expect(Object.isFrozen(input)).toBe(true)
    })

    it('does not call console methods', () => {
      const consoleSpies = [
        vi.spyOn(console, 'log'),
        vi.spyOn(console, 'warn'),
        vi.spyOn(console, 'error'),
        vi.spyOn(console, 'info'),
        vi.spyOn(console, 'debug'),
      ]

      resolveStatusKey({ status: 'Queued', conclusion: null })

      for (const spy of consoleSpies) {
        expect(spy).not.toHaveBeenCalled()
        spy.mockRestore()
      }
    })

    it('returns every StatusKey when iterating all combinations', () => {
      const resultSet = new Set<string>()

      // Test Queued (no conclusion matters)
      resultSet.add(resolveStatusKey({ status: 'Queued', conclusion: null }))

      // Test InProgress (no conclusion matters)
      resultSet.add(resolveStatusKey({ status: 'InProgress', conclusion: null }))

      // Test all RunConclusion values with Completed status.
      // NOTE: This array must be manually kept in sync with the switch statement in
      // conclusionToKey() (status-key.ts). The switch is the canonical exhaustiveness
      // gate; this array is a secondary coverage check. If ts-rs regeneration adds a
      // new RunConclusion variant, update this array AND the switch simultaneously.
      const conclusions: RunConclusion[] = [
        'Success',
        'Failure',
        'Cancelled',
        'TimedOut',
        'ActionRequired',
        'StartupFailure',
        'Stale',
        'Neutral',
        'Skipped',
      ]

      for (const conclusion of conclusions) {
        resultSet.add(resolveStatusKey({ status: 'Completed', conclusion }))
      }

      // Also test the bare-Completed fallback
      resultSet.add(resolveStatusKey({ status: 'Completed', conclusion: null }))

      // All 11 StatusKey values should be covered
      expect(resultSet.size).toBe(STATUS_KEYS.length)
      for (const key of STATUS_KEYS) {
        expect(resultSet.has(key)).toBe(true)
      }
    })
  })

  describe('resolveJobStatusKey', () => {
    it('returns Queued when status is Queued', () => {
      expect(resolveJobStatusKey({ status: 'Queued', conclusion: null })).toBe('Queued')
    })

    it('returns InProgress when status is Waiting', () => {
      expect(resolveJobStatusKey({ status: 'Waiting', conclusion: null })).toBe('InProgress')
    })

    it('returns InProgress when status is InProgress', () => {
      expect(resolveJobStatusKey({ status: 'InProgress', conclusion: null })).toBe('InProgress')
    })

    it('returns Cancelled when Completed with null conclusion (bare-Completed fallback)', () => {
      expect(resolveJobStatusKey({ status: 'Completed', conclusion: null })).toBe('Cancelled')
    })

    it.each([
      ['Success', 'Success'],
      ['Failure', 'Failure'],
      ['Cancelled', 'Cancelled'],
      ['TimedOut', 'TimedOut'],
      ['ActionRequired', 'ActionRequired'],
      ['Stale', 'Stale'],
      ['Neutral', 'Neutral'],
      ['Skipped', 'Skipped'],
    ] as const)('Completed + %s conclusion → %s key', (conclusion, expected) => {
      expect(resolveJobStatusKey({ status: 'Completed', conclusion })).toBe(expected)
    })
  })

  describe('exhaustiveness defense at runtime', () => {
    // These tests guard against off-shape input slipping past the TypeScript
    // boundary (test fixtures with loose types, JSON over the wire). Without a
    // runtime default branch, the switch silently returns undefined and the
    // failure cascades into broken renders that don't surface a useful error.
    // See feedback_exhaustive_switches_at_boundaries.md.

    it('throws when conclusionToKey receives an unknown RunConclusion value', () => {
      expect(() =>
        resolveStatusKey({
          status: 'Completed',
          conclusion: 'success' as RunConclusion, // wrong casing — what the e2e fixtures used to send
        }),
      ).toThrow(/unhandled run conclusion/i)
    })

    it('throws when jobConclusionToKey receives an unknown JobConclusion value', () => {
      expect(() =>
        resolveJobStatusKey({
          status: 'Completed',
          conclusion: 'failure' as JobConclusion, // wrong casing
        }),
      ).toThrow(/unhandled job conclusion/i)
    })
  })

  describe('statusKeyToHumanLabel', () => {
    it.each([
      ['Queued', 'Queued'],
      ['InProgress', 'In progress'],
      ['Success', 'Success'],
      ['Failure', 'Failure'],
      ['Cancelled', 'Cancelled'],
      ['TimedOut', 'Timed out'],
      ['ActionRequired', 'Action required'],
      ['StartupFailure', 'Startup failure'],
      ['Stale', 'Stale'],
      ['Neutral', 'Neutral'],
      ['Skipped', 'Skipped'],
    ] as const)('returns "%s" for %s', (key, expected) => {
      expect(statusKeyToHumanLabel(key)).toBe(expected)
    })

    it('covers all STATUS_KEYS', () => {
      for (const key of STATUS_KEYS) {
        expect(() => statusKeyToHumanLabel(key)).not.toThrow()
        expect(typeof statusKeyToHumanLabel(key)).toBe('string')
      }
    })
  })

  describe('statusKeyToVar', () => {
    it.each([
      ['Queued', 'queued'],
      ['InProgress', 'running'],
      ['Success', 'success'],
      ['Failure', 'failed'],
      ['Cancelled', 'cancelled'],
      ['TimedOut', 'timed-out'],
      ['ActionRequired', 'action-required'],
      ['StartupFailure', 'failed'],
      ['Stale', 'neutral'],
      ['Neutral', 'neutral'],
      ['Skipped', 'neutral'],
    ] as const)('returns CSS token name "%s" for %s', (key, expected) => {
      expect(statusKeyToVar(key)).toBe(expected)
    })

    it('covers all STATUS_KEYS', () => {
      for (const key of STATUS_KEYS) {
        expect(() => statusKeyToVar(key)).not.toThrow()
        expect(typeof statusKeyToVar(key)).toBe('string')
      }
    })
  })
})
