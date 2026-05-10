/**
 * Deterministic 1000-event burst perf test.
 *
 * Browser-mode rationale: requestAnimationFrame is a real browser API.
 * Running in Chromium ensures vi.stubGlobal reliably replaces window.requestAnimationFrame
 * before the module singleton imports and before any RAF call happens. The jsdom
 * environment does not reliably intercept RAF via stubGlobal because vi.useFakeTimers()
 * overrides it again in that environment.
 *
 * Strategy:
 *   - Replace requestAnimationFrame with a manually-driven queue via vi.stubGlobal
 *     BEFORE importing the dispatcher (module-scope singleton captures RAF at call time,
 *     but vi.resetModules() + fresh import ensures a clean slate).
 *   - Dispatch events in BATCH_COUNT batches of BATCH_SIZE each, ticking the RAF
 *     queue after each batch.
 *   - This produces exactly N=BATCH_COUNT flush callbacks (one per tick) with
 *     no wall-clock dependency.
 *
 * Assertions (all deterministic, no bounded/gte checks):
 *   1. flushCount === BATCH_COUNT  (exactly N flushes, not "at most N")
 *   2. runStore.runs.size === TOTAL_EVENTS  (every event landed in store state)
 *   3. totalEventsReceived === TOTAL_EVENTS (no events dropped across all flushes)
 */

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

const BATCH_SIZE = 100
const BATCH_COUNT = 10
const TOTAL_EVENTS = BATCH_SIZE * BATCH_COUNT // 1000

describe('deterministic 1000-event burst coalescing', () => {
  let eventDispatcher: typeof import('$lib/dispatcher')['eventDispatcher']
  let runStore: typeof import('$lib/stores/runs.svelte')['runStore']

  // Manually-driven RAF queue. Populated by the stubbed requestAnimationFrame;
  // drained by tickRAF(). Declared outside beforeEach so tickRAF() can
  // reference the same array as the stub closure.
  const rafQueue: FrameRequestCallback[] = []

  function tickRAF(): void {
    // Snapshot-and-drain: prevent re-entrant RAF scheduling from confusing the
    // queue. The dispatcher's processBuffer() sets rafId=null, so a fresh
    // dispatch() from within the callback would push a new entry — we want
    // that to land in the NEXT tick, not the current one.
    const pending = rafQueue.splice(0)
    for (const cb of pending) {
      cb(performance.now())
    }
  }

  beforeEach(async () => {
    // Isolate modules so the dispatcher singleton is fresh per test run.
    vi.resetModules()

    // Stub RAF BEFORE importing dispatcher. The dispatcher's dispatch() calls
    // requestAnimationFrame() at call-time; since we resetModules, the fresh
    // import will close over the stubbed global.
    rafQueue.length = 0

    vi.stubGlobal('requestAnimationFrame', (cb: FrameRequestCallback): number => {
      rafQueue.push(cb)
      return rafQueue.length // stable id = position in queue
    })

    // cancelAnimationFrame: used by flush() to cancel a pending RAF before
    // draining synchronously. We don't call flush() in this test, but the
    // stub must not throw.
    vi.stubGlobal('cancelAnimationFrame', (_id: number): void => {
      // In this test we never call flush(), so cancelAnimationFrame is never
      // triggered. Provide a no-op stub for completeness.
    })

    // Import AFTER stubs are registered.
    const dispMod = await import('$lib/dispatcher')
    const runsMod = await import('$lib/stores/runs.svelte')
    eventDispatcher = dispMod.eventDispatcher
    runStore = runsMod.runStore

    // Clean state.
    eventDispatcher.clear()
    eventDispatcher.setOnFlush(null)
    runStore.clear()
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    vi.resetModules()
  })

  /** Build a Requested SeqEvent for a unique run id. */
  function makeRequestedEvent(id: number): SeqEvent {
    const envelope: RunEventEnvelope = {
      runId: BigInt(id),
      org: 'test-org',
      repo: 'test-repo',
      workflowName: 'CI',
      workflowPath: null,
      branch: 'main',
      headSha: 'abc',
      commitMessage: null,
      triggerEvent: 'push',
      displayTitle: `Run ${id}`,
      htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${id}`,
      createdAt: '2026-05-02T10:00:00Z',
      runStartedAt: null,
      updatedAt: '2026-05-02T10:00:00Z',
      action: { type: 'Requested' },
    }
    return {
      seq: BigInt(id),
      event: { type: 'Run', data: envelope },
    }
  }

  it('coalesces 1000 events into exactly 10 RAF ticks with zero dropped events', () => {
    // Wire the flush observer via the public setOnFlush hook.
    let flushCount = 0
    let totalEventsReceived = 0
    eventDispatcher.setOnFlush((events) => {
      flushCount++
      totalEventsReceived += events.length
    })

    // Dispatch BATCH_COUNT batches of BATCH_SIZE events each.
    // After each batch, tick the RAF queue exactly once.
    // The first dispatch() in each batch schedules a single RAF (rafId === null at
    // the start of each batch because processBuffer() sets rafId=null when it runs).
    // Subsequent dispatches in the same batch skip scheduling (rafId !== null).
    // tickRAF() fires the one scheduled callback, draining all BATCH_SIZE events.
    for (let batch = 0; batch < BATCH_COUNT; batch++) {
      const base = batch * BATCH_SIZE + 1
      for (let i = base; i < base + BATCH_SIZE; i++) {
        eventDispatcher.dispatch(makeRequestedEvent(i))
      }
      // Advance exactly one RAF tick → one flush of BATCH_SIZE events.
      tickRAF()
    }

    // Assertion 1: exactly N=10 flush callbacks (deterministic, not bounded).
    expect(flushCount).toBe(BATCH_COUNT)

    // Assertion 2: every event landed in store state.
    expect(runStore.runs.size).toBe(TOTAL_EVENTS)

    // Assertion 3: no events were dropped across all flushes.
    expect(totalEventsReceived).toBe(TOTAL_EVENTS)
  })
})
