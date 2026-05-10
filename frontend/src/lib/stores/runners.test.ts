import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import { computePoolStats } from './runners.svelte'
import { runStore } from './runs.svelte'

// Minimal Job factory
function makeJob(
  overrides: Partial<Job> & { id: bigint; runId: bigint; status: Job['status']; labels: string[] },
): Job {
  const {
    id,
    runId,
    status,
    labels,
    runner,
    conclusion,
    steps,
    createdAt,
    startedAt,
    completedAt,
  } = overrides
  return {
    id,
    runId,
    name: overrides.name ?? 'test-job',
    status,
    conclusion: conclusion ?? null,
    labels,
    runner: runner ?? null,
    steps: steps ?? [],
    createdAt: createdAt ?? new Date().toISOString(),
    startedAt: startedAt ?? null,
    completedAt: completedAt ?? null,
  }
}

describe('computePoolStats (pure function)', () => {
  it('returns empty array for no jobs', () => {
    expect(computePoolStats([])).toEqual([])
  })

  it('skips Waiting and Completed jobs', () => {
    const jobs = [
      makeJob({ id: 1n, runId: 1n, status: 'Waiting', labels: ['ubuntu-latest'] }),
      makeJob({ id: 2n, runId: 1n, status: 'Completed', labels: ['ubuntu-latest'] }),
    ]
    expect(computePoolStats(jobs)).toHaveLength(0)
  })

  it('counts Queued jobs', () => {
    const jobs = [
      makeJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] }),
      makeJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools).toHaveLength(1)
    expect(pools[0]?.queued).toBe(2)
    expect(pools[0]?.running).toBe(0)
  })

  it('sets isElastic=true when groupId===0n', () => {
    const job = makeJob({
      id: 1n,
      runId: 1n,
      status: 'InProgress',
      labels: ['ubuntu-latest'],
      runner: { id: 1n, name: 'r', groupId: 0n, groupName: 'GitHub' },
    })
    const pools = computePoolStats([job])
    expect(pools[0]?.isElastic).toBe(true)
    expect(pools[0]?.groupName).toBe('GitHub')
  })

  it('does NOT set isElastic when groupId is non-zero bigint', () => {
    const job = makeJob({
      id: 1n,
      runId: 1n,
      status: 'InProgress',
      labels: ['ubuntu-latest'],
      runner: { id: 1n, name: 'r', groupId: 42n, groupName: 'Group' },
    })
    const pools = computePoolStats([job])
    expect(pools[0]?.isElastic).toBe(false)
  })

  it('LabelSet parity: deduplicates labels before keying', () => {
    const jobs = [
      makeJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['a', 'a', 'b'] }),
      makeJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['a', 'b'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools).toHaveLength(1)
    expect(pools[0]?.queued).toBe(2)
    expect(pools[0]?.labels).toEqual(['a', 'b'])
  })

  it('sorts result by JSON-stringified labels', () => {
    const jobs = [
      makeJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['z'] }),
      makeJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['a'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools[0]?.labels).toEqual(['a'])
    expect(pools[1]?.labels).toEqual(['z'])
  })
})

describe('runnerStore.pools (derived)', () => {
  beforeEach(() => {
    runStore.jobsByRun.clear()
  })

  afterEach(() => {
    runStore.jobsByRun.clear()
  })

  it('starts empty', () => {
    expect(runStore.jobs).toHaveLength(0)
  })

  it('reflects pools after adding jobs to runStore', async () => {
    const { runnerStore } = await import('./runners.svelte')
    const job = makeJob({ id: 1n, runId: 10n, status: 'Queued', labels: ['ubuntu-latest'] })
    runStore.jobsByRun.set(10n, [job])

    expect(runnerStore.pools).toHaveLength(1)
    expect(runnerStore.pools[0]?.queued).toBe(1)
  })

  it('runnerStore.pools has no loadPools method (compile-time check: only verify the store shape)', async () => {
    const { runnerStore } = await import('./runners.svelte')
    // Type-level check: loadPools should not exist on the store
    expect('loadPools' in runnerStore).toBe(false)
    expect('clear' in runnerStore).toBe(false)
  })
})
