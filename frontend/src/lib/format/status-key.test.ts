import { describe, expect, it, vi } from 'vitest'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { resolveStatusKey, STATUS_KEYS } from './status-key'

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
      const consoleSpy = vi.spyOn(console, 'log')

      resolveStatusKey({ status: 'Queued', conclusion: null })

      expect(consoleSpy).not.toHaveBeenCalled()
      consoleSpy.mockRestore()
    })

    it('returns every StatusKey when iterating all combinations', () => {
      const resultSet = new Set<string>()

      // Test Queued (no conclusion matters)
      resultSet.add(resolveStatusKey({ status: 'Queued', conclusion: null }))

      // Test InProgress (no conclusion matters)
      resultSet.add(resolveStatusKey({ status: 'InProgress', conclusion: null }))

      // Test all RunConclusion values with Completed status
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
})
