import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMockJob, createMockRun, createMockRunner } from '$lib/test-utils/factories'
import { computePoolStats } from './runners.svelte'
import { runStore } from './runs.svelte'

describe('computePoolStats (pure function)', () => {
  it('returns empty array for no jobs', () => {
    expect(computePoolStats([])).toEqual([])
  })

  it('skips Waiting and Completed jobs', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'Waiting', labels: ['ubuntu-latest'] }),
      createMockJob({ id: 2n, runId: 1n, status: 'Completed', labels: ['ubuntu-latest'] }),
    ]
    expect(computePoolStats(jobs)).toHaveLength(0)
  })

  it('counts Queued jobs', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] }),
      createMockJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools).toHaveLength(1)
    expect(pools[0]?.queued).toBe(2)
    expect(pools[0]?.running).toBe(0)
  })

  it('captures groupName from the most recent InProgress runner', () => {
    const job = createMockJob({
      id: 1n,
      runId: 1n,
      status: 'InProgress',
      labels: ['ubuntu-latest'],
      runner: createMockRunner({ name: 'r', groupName: 'GitHub' }),
    })
    const pools = computePoolStats([job])
    expect(pools[0]?.groupName).toBe('GitHub')
  })

  it('runner groupName present with no matching declaration stays Undeclared', () => {
    // Regression guard: only operator-declared capacities drive a pool's
    // bounded/unbounded/undeclared classification — runner group names
    // do not influence `total`.
    const job = createMockJob({
      id: 1n,
      runId: 1n,
      status: 'InProgress',
      labels: ['ubuntu-latest'],
      runner: createMockRunner({ name: 'r', groupName: 'GitHub' }),
    })
    const pools = computePoolStats([job])
    expect(pools[0]?.total).toEqual({ kind: 'Undeclared' })
  })

  it('LabelSet parity: deduplicates labels before keying', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['a', 'a', 'b'] }),
      createMockJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['a', 'b'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools).toHaveLength(1)
    expect(pools[0]?.queued).toBe(2)
    expect(pools[0]?.labels).toEqual(['a', 'b'])
  })

  it('sorts result by JSON-stringified labels', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['z'] }),
      createMockJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['a'] }),
    ]
    const pools = computePoolStats(jobs)
    expect(pools[0]?.labels).toEqual(['a'])
    expect(pools[1]?.labels).toEqual(['z'])
  })

  it('merges_bounded_capacity_to_total_bounded — integer declaration produces Bounded', () => {
    const jobs = [
      createMockJob({
        id: 1n,
        runId: 1n,
        status: 'InProgress',
        labels: ['self-hosted', 'linux', 'x64'],
      }),
    ]
    const pools = computePoolStats(jobs, [
      { labels: ['linux', 'self-hosted', 'x64'], capacity: 10 },
    ])
    expect(pools).toHaveLength(1)
    expect(pools[0]?.total).toEqual({ kind: 'Bounded', value: 10 })
    expect(pools[0]?.running).toBe(1)
  })

  it('merges_unbounded_capacity_to_total_unbounded — null declaration produces Unbounded', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'InProgress', labels: ['ubuntu-latest'] }),
    ]
    const pools = computePoolStats(jobs, [{ labels: ['ubuntu-latest'], capacity: null }])
    expect(pools).toHaveLength(1)
    expect(pools[0]?.total).toEqual({ kind: 'Unbounded' })
    expect(pools[0]?.running).toBe(1)
  })

  it('pool_without_declaration_is_undeclared — observed pools without a config entry stay Undeclared', () => {
    const jobs = [
      createMockJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] }),
      createMockJob({ id: 2n, runId: 1n, status: 'Queued', labels: ['self-hosted', 'linux'] }),
    ]
    const pools = computePoolStats(jobs, [{ labels: ['ubuntu-latest'], capacity: 20 }])
    const declared = pools.find((p) => p.labels[0] === 'ubuntu-latest')
    const undeclared = pools.find((p) => p.labels.includes('self-hosted'))
    expect(declared?.total).toEqual({ kind: 'Bounded', value: 20 })
    expect(undeclared?.total).toEqual({ kind: 'Undeclared' })
  })

  it('matches capacity declared with labels in any order via canonicalization', () => {
    // Capacities arrive from the wire pre-sorted by the backend's BTreeSet, but
    // poolKey() re-sorts on the frontend so unsorted input still matches.
    const jobs = [
      createMockJob({
        id: 1n,
        runId: 1n,
        status: 'InProgress',
        labels: ['self-hosted', 'linux', 'x64'],
      }),
    ]
    const pools = computePoolStats(jobs, [{ labels: ['x64', 'linux', 'self-hosted'], capacity: 5 }])
    expect(pools[0]?.total).toEqual({ kind: 'Bounded', value: 5 })
  })

  it('omitting capacities argument keeps existing zero-config behavior', () => {
    const jobs = [createMockJob({ id: 1n, runId: 1n, status: 'Queued', labels: ['ubuntu-latest'] })]
    const pools = computePoolStats(jobs)
    expect(pools[0]?.total).toEqual({ kind: 'Undeclared' })
  })
})

describe('runnerStore.pools (derived)', () => {
  beforeEach(() => {
    runStore.jobsByRun.clear()
    runStore.runs.clear()
  })

  afterEach(() => {
    runStore.jobsByRun.clear()
    runStore.runs.clear()
  })

  it('starts empty', () => {
    expect(runStore.jobs).toHaveLength(0)
  })

  it('reflects pools after adding jobs to runStore', async () => {
    const { runnerStore } = await import('./runners.svelte')
    const job = createMockJob({ id: 1n, runId: 10n, status: 'Queued', labels: ['ubuntu-latest'] })
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

  it('excludes Queued jobs whose parent run is Completed (orphan from cancelled run)', async () => {
    // GitHub does not emit workflow_job terminal events when a run is cancelled
    // before the job starts. These jobs stay Queued but their run is Completed.
    const { runnerStore } = await import('./runners.svelte')
    const job = createMockJob({ id: 1n, runId: 10n, status: 'Queued', labels: ['ubuntu-latest'] })
    const run = createMockRun({ id: 10n, status: 'Completed', conclusion: 'Cancelled' })
    runStore.jobsByRun.set(10n, [job])
    runStore.runs.set(10n, run)

    expect(runnerStore.pools).toHaveLength(0)
  })

  it('counts a higher-attempt Queued job even when the parent run is still the old Completed attempt', async () => {
    // Re-run: workflow_job.queued (attempt 2) arrives before the run event, so
    // the parent row is still the old Completed attempt-1 run. The fresh queued
    // demand must still count toward runner pools (not treated as an orphan).
    const { runnerStore } = await import('./runners.svelte')
    const job = createMockJob({
      id: 1n,
      runId: 30n,
      status: 'Queued',
      labels: ['ubuntu-latest'],
      runAttempt: 2,
    })
    const run = createMockRun({
      id: 30n,
      status: 'Completed',
      conclusion: 'Success',
      runAttempt: 1,
    })
    runStore.jobsByRun.set(30n, [job])
    runStore.runs.set(30n, run)

    expect(runnerStore.pools).toHaveLength(1)
    expect(runnerStore.pools[0]?.queued).toBe(1)
  })

  it('includes Queued jobs whose parent run is not Completed', async () => {
    const { runnerStore } = await import('./runners.svelte')
    const job = createMockJob({ id: 1n, runId: 20n, status: 'Queued', labels: ['ubuntu-latest'] })
    const run = createMockRun({ id: 20n, status: 'InProgress' })
    runStore.jobsByRun.set(20n, [job])
    runStore.runs.set(20n, run)

    expect(runnerStore.pools).toHaveLength(1)
    expect(runnerStore.pools[0]?.queued).toBe(1)
  })
})
