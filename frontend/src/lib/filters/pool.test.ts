import { describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import { filterRunsByPool, jobMatchesPool, type PoolKey, parsePoolKey, poolKey } from './pool'

describe('pool filter', () => {
  describe('poolKey', () => {
    it('is order-independent', () => {
      expect(poolKey(['a', 'b'])).toBe(poolKey(['b', 'a']))
    })

    it('returns a PoolKey-typed value', () => {
      const pk: PoolKey = poolKey(['x86', 'linux'])
      expect(typeof pk).toBe('string')
    })

    it('handles empty label array', () => {
      const empty: PoolKey = poolKey([])
      expect(empty).toBe('')
    })
  })

  describe('parsePoolKey', () => {
    it('returns the branded value for a canonical input', () => {
      const canonical = poolKey(['linux', 'x86'])
      expect(parsePoolKey(canonical)).toBe(canonical)
    })

    it('round-trips poolKey output', () => {
      const labels = ['amd64', 'large', 'self-hosted', 'ubuntu-latest']
      const canonical = poolKey(labels)
      expect(parsePoolKey(canonical)).toBe(canonical)
    })

    it('rejects empty string', () => {
      expect(parsePoolKey('')).toBeNull()
    })

    it('rejects empty segments (leading separator)', () => {
      expect(parsePoolKey('|linux')).toBeNull()
    })

    it('rejects empty segments (trailing separator)', () => {
      expect(parsePoolKey('linux|')).toBeNull()
    })

    it('rejects empty segments (consecutive separators)', () => {
      expect(parsePoolKey('linux||x86')).toBeNull()
    })

    it('rejects unsorted input even though characters are valid', () => {
      // 'x86|linux' is the unsorted form; canonical is 'linux|x86'
      expect(parsePoolKey('x86|linux')).toBeNull()
    })

    it('accepts a single-label canonical form', () => {
      expect(parsePoolKey('linux')).toBe('linux')
    })
  })

  describe('jobMatchesPool', () => {
    it('true case: all pool labels present', () => {
      const result = jobMatchesPool(['x86', 'linux', 'self-hosted'], ['linux', 'self-hosted'])
      expect(result).toBe(true)
    })

    it('false case: job missing one pool label', () => {
      const result = jobMatchesPool(['x86', 'linux'], ['linux', 'self-hosted'])
      expect(result).toBe(false)
    })

    it('true case: empty pool labels (vacuously true)', () => {
      const result = jobMatchesPool(['x86', 'linux'], [])
      expect(result).toBe(true)
    })
  })

  describe('filterRunsByPool', () => {
    const mockRun1: WorkflowRun = {
      id: 1n,
      org: 'test-org',
      repo: 'test-repo',
      workflowName: 'Test Workflow',
      workflowPath: '.github/workflows/test.yml',
      branch: 'main',
      headSha: 'abc123',
      commitMessage: 'Test commit',
      event: 'push',
      displayTitle: 'Run 1',
      status: 'Queued',
      conclusion: null,
      htmlUrl: 'https://example.com/run/1',
      createdAt: '2025-01-01T00:00:00Z',
      runStartedAt: null,
      updatedAt: '2025-01-01T00:00:00Z',
      runAttempt: 1,
    }

    const mockRun2: WorkflowRun = {
      id: 2n,
      org: 'test-org',
      repo: 'test-repo',
      workflowName: 'Test Workflow',
      workflowPath: '.github/workflows/test.yml',
      branch: 'main',
      headSha: 'def456',
      commitMessage: 'Another test',
      event: 'push',
      displayTitle: 'Run 2',
      status: 'InProgress',
      conclusion: null,
      htmlUrl: 'https://example.com/run/2',
      createdAt: '2025-01-02T00:00:00Z',
      runStartedAt: '2025-01-02T00:01:00Z',
      updatedAt: '2025-01-02T00:01:00Z',
      runAttempt: 1,
    }

    const mockJob1: Job = {
      id: 100n,
      name: 'Test Job 1',
      runId: 1n,
      status: 'Queued',
      conclusion: null,
      runner: null,
      labels: ['x86', 'linux', 'self-hosted'],
      steps: [],
      createdAt: '2025-01-01T00:00:00Z',
      startedAt: null,
      completedAt: null,
    }

    const mockJob2: Job = {
      id: 101n,
      name: 'Test Job 2',
      runId: 2n,
      status: 'Completed',
      conclusion: null,
      runner: null,
      labels: ['arm64', 'macos'],
      steps: [],
      createdAt: '2025-01-02T00:00:00Z',
      startedAt: '2025-01-02T00:00:30Z',
      completedAt: '2025-01-02T00:02:00Z',
    }

    it('null filter is identity passthrough', () => {
      const runs = [mockRun1, mockRun2]
      const jobsMap = new Map<bigint, readonly Job[]>([
        [1n, [mockJob1]],
        [2n, [mockJob2]],
      ])
      const result = filterRunsByPool(runs, jobsMap, null)
      expect(result).toBe(runs)
    })

    it('includes runs whose jobs match', () => {
      const runs = [mockRun1, mockRun2]
      const jobsMap = new Map<bigint, readonly Job[]>([
        [1n, [mockJob1]],
        [2n, [mockJob2]],
      ])
      const poolFilter = poolKey(['linux', 'self-hosted'])
      const result = filterRunsByPool(runs, jobsMap, poolFilter)
      expect(result).toHaveLength(1)
      expect(result[0]?.id).toBe(1n)
    })

    it('excludes runs with no jobs in jobsByRunId', () => {
      const runs = [mockRun1, mockRun2]
      const jobsMap = new Map<bigint, readonly Job[]>([
        [1n, [mockJob1]],
        // run 2 not in map
      ])
      const poolFilter = poolKey(['x86', 'linux'])
      const result = filterRunsByPool(runs, jobsMap, poolFilter)
      expect(result).toHaveLength(1)
      expect(result[0]?.id).toBe(1n)
    })

    it('roundtrip: poolKey and jobMatchesPool are consistent', () => {
      const jobLabels = ['linux', 'x86']
      const poolLabels = ['x86', 'linux']
      // Both should produce the same canonical key
      expect(poolKey(jobLabels)).toBe(poolKey(poolLabels))
      // jobMatchesPool should confirm match
      expect(jobMatchesPool(jobLabels, poolLabels)).toBe(true)
    })

    it('subset match: job with extra labels still matches', () => {
      const runs = [mockRun1]
      const jobWithExtra: Job = {
        ...mockJob1,
        labels: ['linux', 'x86', 'self-hosted', 'gpu'],
      }
      const jobsMap = new Map<bigint, readonly Job[]>([[1n, [jobWithExtra]]])
      const poolFilter = poolKey(['linux', 'self-hosted', 'x86'])
      const result = filterRunsByPool(runs, jobsMap, poolFilter)
      expect(result).toHaveLength(1)
      expect(result[0]?.id).toBe(1n)
    })

    it('subset match: job missing one pool label is excluded', () => {
      const runs = [mockRun1]
      const jobMissing: Job = {
        ...mockJob1,
        labels: ['linux', 'x86'],
      }
      const jobsMap = new Map<bigint, readonly Job[]>([[1n, [jobMissing]]])
      const poolFilter = poolKey(['linux', 'self-hosted', 'x86'])
      const result = filterRunsByPool(runs, jobsMap, poolFilter)
      expect(result).toHaveLength(0)
    })
  })

  describe('PoolKey brand enforcement', () => {
    it('rejects raw string assignment without a constructor call', () => {
      const fromConstructor: PoolKey = poolKey(['linux', 'x86'])
      expect(typeof fromConstructor).toBe('string')

      // This block proves the brand is enforced by the TS compiler.
      // If the @ts-expect-error directive is REMOVED, TypeScript will
      // refuse to compile this file (the assignment is a type error).
      // If the brand is REMOVED from PoolKey (i.e., PoolKey becomes a
      // plain string alias), TypeScript will emit "Unused @ts-expect-error
      // directive" and svelte-check / pnpm check will fail.
      // Either failure mode means the brand contract is broken.
      // @ts-expect-error -- raw string is not assignable to PoolKey
      const fromRawString: PoolKey = 'linux|x86'

      // Use both bindings so tsc/biome do not flag them as unused;
      // value equality is incidental.
      expect(typeof fromRawString).toBe('string')
    })
  })
})
