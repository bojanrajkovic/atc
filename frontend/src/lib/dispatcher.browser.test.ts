import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createMockJobCommittedEvent, createMockRunner } from '$lib/test-utils/factories'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'

describe('pool-stats derivation: dispatcher + runnerStore integration (browser mode)', () => {
  let eventDispatcher: typeof import('$lib/dispatcher')['eventDispatcher']
  let runnerStore: typeof import('$lib/stores/runners.svelte')['runnerStore']
  let runStore: typeof import('$lib/stores/runs.svelte')['runStore']

  beforeEach(async () => {
    vi.resetModules()
    const dispatcherModule = await import('$lib/dispatcher')
    const runnerModule = await import('$lib/stores/runners.svelte')
    const runModule = await import('$lib/stores/runs.svelte')
    eventDispatcher = dispatcherModule.eventDispatcher
    runnerStore = runnerModule.runnerStore
    runStore = runModule.runStore

    eventDispatcher.clear()
    runStore.jobsByRun.clear()
  })

  const makeJobEvent = (
    jobId: bigint,
    runId: bigint,
    action: JobEventEnvelope['action'],
  ): CommittedEvent =>
    createMockJobCommittedEvent(jobId, { jobId, runId, name: `job-${jobId}`, action })

  it('Queued job creates pool with queued=1, running=0', () => {
    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, { type: 'Queued', data: { labels: ['ubuntu-latest'], steps: [] } }),
    )
    eventDispatcher.flush()

    expect(runnerStore.pools).toHaveLength(1)
    const pool = runnerStore.pools[0]!
    expect(pool.labels).toEqual(['ubuntu-latest'])
    expect(pool.queued).toBe(1)
    expect(pool.running).toBe(0)
    // Without an operator declaration for this label set, the pool is
    // Undeclared — runner group ids never participate in classification.
    expect(pool.total).toEqual({ kind: 'Undeclared' })
  })

  it('InProgress job with groupName populated produces running=1', () => {
    const runner = createMockRunner({ groupName: 'Default' })

    // First: Queued
    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, { type: 'Queued', data: { labels: ['ubuntu-latest'], steps: [] } }),
    )
    eventDispatcher.flush()

    // Then: InProgress with runner
    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, {
        type: 'InProgress',
        data: { runner, labels: ['ubuntu-latest'], steps: [] },
      }),
    )
    eventDispatcher.flush()

    expect(runnerStore.pools).toHaveLength(1)
    const pool = runnerStore.pools[0]!
    expect(pool.queued).toBe(0)
    expect(pool.running).toBe(1)
    expect(pool.groupName).toBe('Default')
    // Group-id observation does not drive total — only operator declarations do.
    expect(pool.total).toEqual({ kind: 'Undeclared' })
  })

  it('Completed job removes pool', () => {
    const runner = createMockRunner({ groupName: 'Default' })

    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, { type: 'Queued', data: { labels: ['ubuntu-latest'], steps: [] } }),
    )
    eventDispatcher.flush()

    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, {
        type: 'Completed',
        data: { conclusion: 'Success', runner, labels: ['ubuntu-latest'], steps: [] },
      }),
    )
    eventDispatcher.flush()

    expect(runnerStore.pools).toHaveLength(0)
  })

  it('Duplicate Job event dispatch produces idempotent pools (deep-equal across two dispatches)', () => {
    const event = makeJobEvent(1n, 10n, {
      type: 'Queued',
      data: { labels: ['ubuntu-latest'], steps: [] },
    })

    eventDispatcher.dispatch(event)
    eventDispatcher.flush()
    const poolsAfterFirst = JSON.stringify(runnerStore.pools)

    eventDispatcher.dispatch(event)
    eventDispatcher.flush()
    const poolsAfterSecond = JSON.stringify(runnerStore.pools)

    expect(poolsAfterFirst).toBe(poolsAfterSecond)
    expect(runnerStore.pools[0]?.queued).toBe(1)
  })

  it('LabelSet parity: labels ["a","a","b"] and ["a","b"] key to the same pool', () => {
    // Job 1: labels with duplicate
    eventDispatcher.dispatch(
      makeJobEvent(1n, 10n, { type: 'Queued', data: { labels: ['a', 'a', 'b'], steps: [] } }),
    )
    eventDispatcher.flush()

    // Job 2: same labels without duplicate — should merge into same pool
    eventDispatcher.dispatch(
      makeJobEvent(2n, 10n, { type: 'Queued', data: { labels: ['a', 'b'], steps: [] } }),
    )
    eventDispatcher.flush()

    // Both jobs should be in the same pool (dedup makes ['a','a','b'] === ['a','b'])
    expect(runnerStore.pools).toHaveLength(1)
    expect(runnerStore.pools[0]?.queued).toBe(2)
    expect(runnerStore.pools[0]?.labels).toEqual(['a', 'b'])
  })
})
