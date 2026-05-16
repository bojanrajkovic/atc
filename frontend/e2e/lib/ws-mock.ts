import type { Page } from '@playwright/test'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunEvent } from '$lib/types/generated/RunEvent'

/**
 * JS-level WebSocket mock for E2E tests.
 *
 * Playwright's `routeWebSocket` intercepts at the CDP layer, but
 * `WebSocketRoute.send()` does not reliably deliver messages to the page
 * in Vite dev-server environments (known Playwright bug:
 * microsoft/playwright#34280, #33370). Instead, we mock at the JS level:
 *
 * - Intercept `new WebSocket('/v1/ws')` and fire `onopen` via microtask
 * - Passthrough all other URLs (including Vite HMR) to the real WebSocket
 * - State transitions use window.__stores bridge (set in main.ts) instead
 *
 * This tests: JSON parsing with bigint reviver → store mutation →
 * Svelte reactivity → DOM rendering. The WS transport and EventDispatcher
 * RAF path are NOT exercised (covered by browser-mode tests instead).
 */
export const WS_MOCK_INIT_SCRIPT = `
(() => {
  const _RealWS = window.WebSocket;
  let _mockInstance = null;

  class MockWebSocket extends EventTarget {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSING = 2;
    static CLOSED = 3;

    readyState = MockWebSocket.CONNECTING;
    url;
    protocol = '';
    extensions = '';
    bufferedAmount = 0;
    binaryType = 'blob';
    onopen = null;
    onmessage = null;
    onerror = null;
    onclose = null;

    constructor(url, protocols) {
      super();
      this.url = typeof url === 'string' ? url : url.toString();
      _mockInstance = this;
      // Fire onopen in a microtask so the caller can set up handlers
      Promise.resolve().then(() => {
        this.readyState = MockWebSocket.OPEN;
        const event = new Event('open');
        if (this.onopen) this.onopen(event);
        this.dispatchEvent(event);
      });
    }
    send(data) { /* page→server messages ignored in mock */ }
    close(code, reason) {
      this.readyState = MockWebSocket.CLOSED;
      const event = new CloseEvent('close', { code: code || 1000, reason: reason || '' });
      if (this.onclose) this.onclose(event);
      this.dispatchEvent(event);
    }
  }

  // Replace WebSocket — intercept /v1/ws, passthrough everything else
  window.WebSocket = function(url, protocols) {
    const urlStr = typeof url === 'string' ? url : url.toString();
    if (urlStr.includes('/v1/ws')) {
      return new MockWebSocket(url, protocols);
    }
    return new _RealWS(url, protocols);
  };
  window.WebSocket.CONNECTING = _RealWS.CONNECTING;
  window.WebSocket.OPEN = _RealWS.OPEN;
  window.WebSocket.CLOSING = _RealWS.CLOSING;
  window.WebSocket.CLOSED = _RealWS.CLOSED;
  window.WebSocket.prototype = _RealWS.prototype;

})();
`

/** Helper: build a CommittedEvent JSON payload for a Run event (wire format). */
export function makeRunEvent(
  seq: number,
  fields: {
    runId: number
    displayTitle: string
    createdAt: string
    runStartedAt: string | null
    updatedAt: string
    // Strictly typed via the generated discriminated union so that wrong
    // casings (e.g. `conclusion: 'success'` instead of `'Success'`) fail at
    // compile time rather than silently breaking renders at runtime. See
    // feedback_exhaustive_switches_at_boundaries.md for the original incident.
    action: RunEvent
  },
): string {
  return JSON.stringify({
    seq,
    event: {
      type: 'Run',
      data: {
        runId: fields.runId,
        org: 'test-org',
        repo: 'test-repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: fields.displayTitle,
        htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${fields.runId}`,
        createdAt: fields.createdAt,
        runStartedAt: fields.runStartedAt,
        updatedAt: fields.updatedAt,
        action: fields.action,
      },
    },
  })
}

/** Helper: build a CommittedEvent JSON payload for a Job event. */
export function makeJobCommittedEvent(
  seq: number,
  opts: {
    jobData: JobEventEnvelope
  },
): string {
  return JSON.stringify(
    {
      seq,
      event: {
        type: 'Job',
        data: opts.jobData,
      },
    },
    bigintReplacer,
  )
}

/**
 * JSON.stringify replacer that emits bigint values as strings. Use when
 * stringifying any payload that includes ts-rs `RunId` / `JobId` / `seq`
 * fields (which are bigint in-memory). The matching reviver in
 * `connection.ts` accepts both number and string and converts back to
 * bigint, so wire round-trip is preserved.
 */
export const bigintReplacer = (_key: string, value: unknown): unknown =>
  typeof value === 'bigint' ? value.toString() : value

/** Inject a websocket event into the app via the EventDispatcher global bridge.
 *  Tests the JSON parsing (bigint reviver) → EventDispatcher → store mutation → Svelte reactivity → DOM pipeline.
 *  Note: Playwright's routeWebSocket.send() has a known delivery issue in this
 *  Vite dev-server environment, so we route through window.eventDispatcher instead. */
export async function sendWS(page: Page, msg: string): Promise<void> {
  const result = await page.evaluate((data) => {
    // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
    const dispatcher = (window as any).eventDispatcher
    if (!dispatcher) return 'no dispatcher bridge'

    // Parse with the same bigint reviver as ConnectionManager
    const reviver = (key: string, value: unknown) => {
      if (
        ['seq', 'id', 'runId', 'jobId', 'groupId', 'number'].includes(key) &&
        (typeof value === 'number' || typeof value === 'string')
      ) {
        try {
          return BigInt(value)
        } catch {
          return value
        }
      }
      return value
    }
    const committedEvent = JSON.parse(data, reviver)

    dispatcher.dispatch(committedEvent)
    dispatcher.flush()

    return JSON.stringify({ result: 'dispatched' })
  }, msg)
  const parsed = JSON.parse(result)
  if (parsed.result !== 'dispatched') throw new Error(`WS send failed: ${result}`)
}

/**
 * Inject multiple websocket events as a batch via the EventDispatcher global bridge.
 * Dispatches all events without flushing between them, then waits for the RAF drain
 * to complete. Used by burst-testing scenarios (aria-live, frame-budget).
 *
 * Synchronization fence:
 *   1. `bufferLength === 0` — the RAF fired and the buffer was drained
 *   2. One extra RAF tick — ensures the post-flush callback has had at least one
 *      tick to run (relevant for aria-busy flipping and debounce timers)
 */
export async function sendWSBatch(page: Page, msgs: string[]): Promise<void> {
  await page.evaluate((dataList) => {
    // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
    const dispatcher = (window as any).eventDispatcher
    if (!dispatcher) throw new Error('no dispatcher bridge')

    const reviver = (key: string, value: unknown) => {
      if (
        ['seq', 'id', 'runId', 'jobId', 'groupId', 'number'].includes(key) &&
        (typeof value === 'number' || typeof value === 'string')
      ) {
        try {
          return BigInt(value)
        } catch {
          return value
        }
      }
      return value
    }

    for (const data of dataList) {
      const committedEvent = JSON.parse(data, reviver)
      dispatcher.dispatch(committedEvent)
    }
    // Do NOT flush here — let RAF batch naturally
  }, msgs)

  // Wait for the RAF to drain the buffer
  await page.waitForFunction(() => {
    // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
    return (window as any).eventDispatcher?.bufferLength === 0
  })

  // One extra RAF tick to allow the post-flush callback to complete
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())))
}

/**
 * Pacing driver for `sendWSBatchPaced`:
 *   - `'raf'` (default): each pacing tick fires on a `requestAnimationFrame`.
 *     The dispatcher's own flush rAF (scheduled by the first `dispatch()` of a
 *     slice) is queued before the next pacing rAF, so the next frame runs
 *     "dispatcher flush → next pacing tick" — alternating drain/inject.
 *   - `'macrotask'`: each pacing tick fires on a `setTimeout(0)` macrotask. The
 *     dispatcher's flush rAF can coalesce multiple paces, simulating sub-frame
 *     WS bursts that arrive off the rAF cadence.
 *   - `{ intervalMs }`: each pacing tick fires after a fixed `setTimeout` delay
 *     (useful when a test wants explicit time spacing between slices).
 */
export type PaceDriver = 'raf' | 'macrotask' | { intervalMs: number }

/**
 * Pre-compute a randomized batch schedule for `sendWSBatchPaced`. Uses a seeded
 * LCG (Numerical Recipes constants) so traces are reproducible across CI runs.
 * Returns an array of slice sizes whose sum equals `total`, with each slice in
 * `[min, max]`.
 *
 * Constraint: `max >= 2 * min - 1` (otherwise the algorithm can land in an
 * infeasible remainder window where `remaining > max` but `remaining < 2 * min`).
 */
export function randomBatchSchedule(
  total: number,
  opts: { min: number; max: number; seed: number },
): number[] {
  const { min, max, seed } = opts
  // Inputs must be integers so the produced schedule is a `Array<positive integer>` —
  // sendWSBatchPaced's invariant. `Math.floor` inside the slice-size formula already
  // rounds the random component, but non-integer `min` would propagate through.
  if (!Number.isInteger(min))
    throw new Error(`randomBatchSchedule: min must be an integer (got ${min})`)
  if (!Number.isInteger(max))
    throw new Error(`randomBatchSchedule: max must be an integer (got ${max})`)
  if (!Number.isInteger(total))
    throw new Error(`randomBatchSchedule: total must be an integer (got ${total})`)
  if (min < 1) throw new Error(`randomBatchSchedule: min must be >= 1 (got ${min})`)
  if (max < min) throw new Error(`randomBatchSchedule: max (${max}) must be >= min (${min})`)
  if (max < 2 * min - 1)
    throw new Error(
      `randomBatchSchedule: max (${max}) must be >= 2 * min - 1 (${2 * min - 1}) to avoid an infeasible remainder window`,
    )
  if (total < min) throw new Error(`randomBatchSchedule: total (${total}) must be >= min (${min})`)

  let state = seed >>> 0 || 1
  const next = (): number => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state / 0x1_0000_0000
  }

  const schedule: number[] = []
  let remaining = total
  while (remaining > 0) {
    let size: number
    if (remaining <= max) {
      size = remaining
    } else {
      const upper = Math.min(max, remaining - min)
      size = min + Math.floor(next() * (upper - min + 1))
    }
    schedule.push(size)
    remaining -= size
  }
  return schedule
}

/**
 * Inject events across asynchronous boundaries, draining `schedule[i]` events per tick.
 * Each tick parses + dispatches a slice synchronously, then schedules the next tick on
 * the chosen driver. Default driver is `'raf'` — the dispatcher's own flush rAF runs
 * between pacing ticks, exercising coalescing under per-tick arrival pacing.
 *
 * The existing `sendWSBatch` drain fence (`waitForFunction(() => bufferLength === 0)`
 * + one extra rAF) is reused after the pacing promise resolves.
 *
 * Throws if any slice is not a positive integer, or if `sum(schedule) !== msgs.length`.
 */
export async function sendWSBatchPaced(
  page: Page,
  msgs: string[],
  schedule: ReadonlyArray<number>,
  opts?: { driver?: PaceDriver },
): Promise<void> {
  // Validate every slice is a positive integer before crossing the page.evaluate
  // boundary — a non-integer or non-positive entry would silently corrupt msgIdx
  // arithmetic inside the in-browser tick loop. `randomBatchSchedule` guarantees
  // this by construction, but hand-authored schedules (a supported use case)
  // can violate it.
  for (let i = 0; i < schedule.length; i++) {
    const size = schedule[i]
    if (size === undefined || !Number.isInteger(size) || size < 1) {
      throw new Error(`sendWSBatchPaced: schedule[${i}] must be a positive integer (got ${size})`)
    }
  }
  const scheduleSum = schedule.reduce((a, b) => a + b, 0)
  if (scheduleSum !== msgs.length) {
    throw new Error(
      `sendWSBatchPaced: schedule sum (${scheduleSum}) !== msgs.length (${msgs.length})`,
    )
  }
  const driver: PaceDriver = opts?.driver ?? 'raf'

  await page.evaluate(
    ({ dataList, schedule, driver }) => {
      return new Promise<void>((resolve, reject) => {
        // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
        const dispatcher = (window as any).eventDispatcher
        if (!dispatcher) {
          reject(new Error('no dispatcher bridge'))
          return
        }

        const reviver = (key: string, value: unknown) => {
          if (
            ['seq', 'id', 'runId', 'jobId', 'groupId', 'number'].includes(key) &&
            (typeof value === 'number' || typeof value === 'string')
          ) {
            try {
              return BigInt(value)
            } catch {
              return value
            }
          }
          return value
        }

        const scheduleNext: (cb: () => void) => void =
          driver === 'raf'
            ? (cb) => {
                requestAnimationFrame(() => cb())
              }
            : driver === 'macrotask'
              ? (cb) => {
                  setTimeout(cb, 0)
                }
              : (cb) => {
                  setTimeout(cb, driver.intervalMs)
                }

        let scheduleIdx = 0
        let msgIdx = 0
        const tick = (): void => {
          // Invariants (enforced at API boundary):
          //   sum(schedule) === dataList.length, and tick() is only re-scheduled
          //   when msgIdx < dataList.length — so scheduleIdx and msgIdx are
          //   both in-bounds here. Non-null assertions surface invariant
          //   violations loudly rather than silently skipping.
          // biome-ignore lint/style/noNonNullAssertion: scheduleIdx < schedule.length by invariant
          const sliceSize = schedule[scheduleIdx++]!
          const end = msgIdx + sliceSize
          for (; msgIdx < end; msgIdx++) {
            // biome-ignore lint/style/noNonNullAssertion: msgIdx < dataList.length by invariant
            dispatcher.dispatch(JSON.parse(dataList[msgIdx]!, reviver))
          }
          if (msgIdx < dataList.length) scheduleNext(tick)
          else resolve()
        }
        scheduleNext(tick)
      })
    },
    { dataList: msgs, schedule: [...schedule], driver },
  )

  await page.waitForFunction(() => {
    // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
    return (window as any).eventDispatcher?.bufferLength === 0
  })

  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())))
}
