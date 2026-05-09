import { describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import { summarizeRunners } from './runners'

/**
 * Minimal inline Job fixture for testing.
 * Fields required for summarizeRunners are just runner and runner.name.
 * Other fields are included to satisfy the Job type but are unused by the tests.
 */
function createMockJob(overrides?: Partial<Job>): Job {
  return {
    id: 1n,
    name: 'test-job',
    runId: 1n,
    status: 'Queued',
    conclusion: null,
    runner: null,
    labels: [],
    steps: [],
    createdAt: '2026-04-17T00:00:00Z',
    startedAt: null,
    completedAt: null,
    ...overrides,
  }
}

describe('format/runners', () => {
  describe('Aggregates multiple jobs with same runner name', () => {
    it('returns runner name when all jobs share the same runner', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
      ]

      const result = summarizeRunners(jobs)

      expect(result).toBe('runner-1')
    })
  })

  describe('Returns count when multiple distinct runners', () => {
    it('returns "2 runners" when jobs span exactly two distinct runner names', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 2n, name: 'runner-2', groupId: null, groupName: null },
        }),
      ]

      const result = summarizeRunners(jobs)

      expect(result).toBe('2 runners')
    })

    it('returns "3 runners" when jobs span three distinct runner names', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 2n, name: 'runner-2', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 3n, name: 'runner-3', groupId: null, groupName: null },
        }),
      ]

      const result = summarizeRunners(jobs)

      expect(result).toBe('3 runners')
    })
  })

  describe('Returns null when no runners assigned', () => {
    it('returns null for empty job list', () => {
      const jobs: Job[] = []

      const result = summarizeRunners(jobs)

      expect(result).toBe(null)
    })

    it('returns null when all jobs have runner: null', () => {
      const jobs = [
        createMockJob({ runner: null }),
        createMockJob({ runner: null }),
        createMockJob({ runner: null }),
      ]

      const result = summarizeRunners(jobs)

      expect(result).toBe(null)
    })
  })

  describe('Handles partial runner coverage', () => {
    it('returns the single runner name when one job has runner and others do not', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({ runner: null }),
        createMockJob({ runner: null }),
      ]

      const result = summarizeRunners(jobs)

      expect(result).toBe('runner-1')
    })
  })

  describe('Pure function with no side effects', () => {
    it('returns identical output when called twice with same input', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
        createMockJob({
          runner: { id: 2n, name: 'runner-2', groupId: null, groupName: null },
        }),
      ]

      const result1 = summarizeRunners(jobs)
      const result2 = summarizeRunners(jobs)

      expect(result1).toBe(result2)
    })

    it('does not mutate the input array when called', () => {
      const jobs = [
        createMockJob({
          runner: { id: 1n, name: 'runner-1', groupId: null, groupName: null },
        }),
      ]

      Object.freeze(jobs)

      const result = summarizeRunners(jobs)

      expect(result).toBe('runner-1')
      expect(Object.isFrozen(jobs)).toBe(true)
    })
  })
})
