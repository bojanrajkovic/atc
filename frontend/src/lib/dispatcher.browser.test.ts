import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

describe('live-pool-stats.AC2.6: dispatcher + runnerStore integration (browser mode)', () => {
  let eventDispatcher: typeof import('$lib/dispatcher')['eventDispatcher']
  let runnerStore: typeof import('$lib/stores/runners.svelte')['runnerStore']

  beforeEach(async () => {
    vi.resetModules()
    const dispatcherModule = await import('$lib/dispatcher')
    const runnerModule = await import('$lib/stores/runners.svelte')
    eventDispatcher = dispatcherModule.eventDispatcher
    runnerStore = runnerModule.runnerStore

    // Clear any pending RAF buffer state and prior pools
    eventDispatcher.clear()
    runnerStore.clear()
  })

  it('runnerStore.pools equals last populated poolStatsAfter after batched flush', () => {
    // fixtures: three distinct RunnerPoolStats arrays
    const poolsA = [
      {
        labels: ['ubuntu-latest'],
        queued: 1,
        running: 0,
        groupName: 'GitHub Actions',
        isElastic: true,
        total: null,
      },
    ]

    const poolsB = [
      {
        labels: ['ubuntu-latest'],
        queued: 0,
        running: 1,
        groupName: 'GitHub Actions',
        isElastic: true,
        total: null,
      },
    ]

    const poolsC: typeof poolsA = []

    const makeJobEnvelope = (id: bigint): JobEventEnvelope => ({
      jobId: id,
      runId: 1n,
      org: 'org',
      repo: 'repo',
      name: `test-job-${id}`,
      createdAt: new Date().toISOString(),
      startedAt: null,
      completedAt: null,
      action: {
        type: 'Queued',
        data: {
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    })

    // dispatch three Job events with distinct sidecars
    ;[poolsA, poolsB, poolsC].forEach((pools, i) => {
      const seqEvent: SeqEvent = {
        seq: BigInt(i + 1),
        event: { type: 'Job', data: makeJobEnvelope(BigInt(i + 1)) },
        poolStatsAfter: pools,
      }
      eventDispatcher.dispatch(seqEvent)
    })

    // single flush processes all three
    eventDispatcher.flush()

    // last wins: pools equals the third payload
    expect(runnerStore.pools).toEqual(poolsC)
  })
})
