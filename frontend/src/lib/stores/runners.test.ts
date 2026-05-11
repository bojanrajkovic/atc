import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
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

// Minimal WorkflowRun factory
function makeRun(
  overrides: Partial<WorkflowRun> & { id: bigint; status: WorkflowRun['status'] },
): WorkflowRun {
  return {
    id: overrides.id,
    org: overrides.org ?? 'test-org',
    repo: overrides.repo ?? 'test-repo',
    workflowName: overrides.workflowName ?? null,
    workflowPath: overrides.workflowPath ?? null,
    branch: overrides.branch ?? null,
    headSha: overrides.headSha ?? 'abc123',
    commitMessage: overrides.commitMessage ?? null,
    event: overrides.event ?? 'push',
    displayTitle: overrides.displayTitle ?? 'Test Run',
    status: overrides.status,
    conclusion: overrides.conclusion ?? null,
    htmlUrl: overrides.htmlUrl ?? 'https://github.com/test',
    createdAt: overrides.createdAt ?? new Date().toISOString(),
    runStartedAt: overrides.runStartedAt ?? null,
    updatedAt: overrides.updatedAt ?? new Date().toISOString(),
  }
}

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

  it('excludes Queued jobs whose parent run is Completed (orphan from cancelled run)', async () => {
    // GitHub does not emit workflow_job terminal events when a run is cancelled
    // before the job starts. These jobs stay Queued but their run is Completed.
    const { runnerStore } = await import('./runners.svelte')
    const job = makeJob({ id: 1n, runId: 10n, status: 'Queued', labels: ['ubuntu-latest'] })
    const run = makeRun({ id: 10n, status: 'Completed', conclusion: 'Cancelled' })
    runStore.jobsByRun.set(10n, [job])
    runStore.runs.set(10n, run)

    expect(runnerStore.pools).toHaveLength(0)
  })

  it('includes Queued jobs whose parent run is not Completed', async () => {
    const { runnerStore } = await import('./runners.svelte')
    const job = makeJob({ id: 1n, runId: 20n, status: 'Queued', labels: ['ubuntu-latest'] })
    const run = makeRun({ id: 20n, status: 'InProgress' })
    runStore.jobsByRun.set(20n, [job])
    runStore.runs.set(20n, run)

    expect(runnerStore.pools).toHaveLength(1)
    expect(runnerStore.pools[0]?.queued).toBe(1)
  })
})
