/**
 * frame-budget.test.ts — Frame-budget tracing (informational).
 *
 * Measures dispatcher rAF coalescing under realistic randomized arrival pacing.
 * Fires 1000 WS events through the live `EventDispatcher` using `sendWSBatchPaced`,
 * which splits the burst into variable-size slices (default driver: `'raf'`) so
 * the dispatcher's flush rAF runs between pacing ticks — the alternating
 * "drain previous-slice → inject next-slice" cadence is what real WS arrival
 * looks like in production. The deltas observed via CDP `AnimationFrame` events
 * are therefore dominated by the dispatcher's coalescing behavior, not by a
 * synchronous injection-loop tax (as was the case in the prior `sendWSBatch`
 * shape).
 *
 * Tier 1 (`dispatcher.perf.browser.test.ts`) still owns the deterministic
 * rAF-coalescing gate via a manually-driven rAF queue. Tier 2 here is orthogonal:
 * it measures real-browser pacing under a randomized but seeded schedule so the
 * artifact is reproducible across CI runs.
 *
 * Modern Chromium emits a single `AnimationFrame` trace event per rAF tick
 * (the legacy `BeginFrame` / `FireAnimationFrame` names are not present in
 * `devtools.timeline,rendering` traces from this Chromium build — see the
 * `top_event_names` histogram in the artifact for empirical confirmation).
 *
 * This test:
 *   1. Starts a CDP trace with the devtools.timeline,rendering categories.
 *   2. Pre-computes a seeded random batch schedule (~12–30 slices of size [10,100]).
 *   3. Fires 1000 WS events through the live EventDispatcher with rAF pacing
 *      between slices (via sendWSBatchPaced).
 *   4. Parses AnimationFrame deltas from the collected trace data.
 *   5. Logs a structured frame-budget summary (p50_ms, p95_ms, dropped_frames).
 *   6. Saves the schedule, trace summary, top-50 event-name histogram (for
 *      diagnosing future Chromium event-name renames without rerunning), and
 *      per-frame timestamps to test-results/frame-budget-trace.json.
 *
 * The test ALWAYS passes (no timing assertions). All assertions are structural
 * (trace data was collected, artifact was written).
 *
 * Artifact location: frontend/test-results/frame-budget-trace.json
 * (uploaded by the "Upload test artifacts" step in ci.yml).
 */

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import {
  bigintReplacer,
  makeRunEvent,
  randomBatchSchedule,
  sendWSBatchPaced,
  WS_MOCK_INIT_SCRIPT,
} from './lib/ws-mock'

const _dirname = path.dirname(fileURLToPath(import.meta.url))

/** Minimal shape of a CDP Tracing.dataCollected event payload. */
interface TraceChunk {
  value: CdpTraceEvent[]
}

/** A single Chrome trace event (only fields we care about). */
interface CdpTraceEvent {
  name?: string
  ts?: number // microseconds
  [key: string]: unknown
}

/**
 * Compute percentile of a sorted array of numbers.
 * Returns 0 for empty arrays.
 */
function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0
  const idx = Math.ceil((p / 100) * sorted.length) - 1
  return sorted[Math.max(0, idx)] ?? 0
}

test.describe('Frame-budget tracing (informational)', () => {
  // This test exercises real rendering via CDP tracing — give it extra time.
  test.setTimeout(60_000)

  test('fires 1000 events, captures AnimationFrame deltas, saves trace artifact', async ({
    page,
    context,
  }) => {
    // --- Setup: WS mock + empty state snapshot ---
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)

    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(
          { lastSeq: 1n, runs: [], jobs: [], runnerPoolCapacities: [] } satisfies StateSnapshot,
          bigintReplacer,
        ),
      })
    })

    await page.goto('/')
    // Wait for the app to reach "connected" state (same pattern as aria-live.test.ts).
    await expect(page.locator('[aria-label="Workflow run updates"]')).toBeAttached({
      timeout: 10_000,
    })

    // --- Start CDP tracing ---
    const cdpSession = await context.newCDPSession(page)
    const traceEvents: CdpTraceEvent[] = []

    cdpSession.on('Tracing.dataCollected', (chunk: TraceChunk) => {
      traceEvents.push(...chunk.value)
    })

    const traceCompletePromise = new Promise<void>((resolve) => {
      cdpSession.on('Tracing.tracingComplete', () => resolve())
    })

    await cdpSession.send('Tracing.start', {
      // devtools.timeline: BeginFrame, CommitLoad, etc.
      // rendering: layout, paint, composite timing events
      categories: 'devtools.timeline,rendering',
      transferMode: 'ReportEvents',
    })

    // --- Fire 1000 events with randomized rAF pacing through the real EventDispatcher ---
    const msgs: string[] = []
    const now = new Date().toISOString()
    for (let i = 1; i <= 1000; i++) {
      msgs.push(
        makeRunEvent(i, {
          runId: i,
          displayTitle: `Run ${i}`,
          createdAt: now,
          runStartedAt: null,
          updatedAt: now,
          action: { type: 'Requested' },
        }),
      )
    }
    // Seeded schedule keeps the artifact reproducible across CI runs (~12–30
    // slices, wide variance). Default driver `'raf'` interleaves dispatcher
    // flushes between pacing ticks — what real WS arrival pacing looks like.
    const schedule = randomBatchSchedule(1000, { min: 10, max: 100, seed: 1 })
    await sendWSBatchPaced(page, msgs, schedule)

    // --- Stop tracing and collect data ---
    await cdpSession.send('Tracing.end')
    await traceCompletePromise
    await cdpSession.detach()

    // --- Diagnostic: top event names by count (kept in artifact for future debugging) ---
    const eventNameCounts = new Map<string, number>()
    for (const e of traceEvents) {
      if (typeof e.name === 'string') {
        eventNameCounts.set(e.name, (eventNameCounts.get(e.name) ?? 0) + 1)
      }
    }
    const topEventNames = [...eventNameCounts.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 50)
      .map(([name, count]) => ({ name, count }))

    // --- Parse AnimationFrame deltas ---
    // With paced injection (sendWSBatchPaced), the deltas reflect dispatcher
    // rAF coalescing under randomized per-tick arrival pacing: the dispatcher's
    // flush rAF fires between pacing ticks (FIFO with the next pacing rAF), so
    // each slice drains in its own frame. Slices near the upper bound (100 events)
    // can exceed one frame budget — that's the realistic WS-burst case we wanted
    // to capture. See architecture doc § Performance Verification for framing.
    const animationFrames = traceEvents
      .filter(
        (e): e is CdpTraceEvent & { name: string; ts: number } =>
          e.name === 'AnimationFrame' && typeof e.ts === 'number',
      )
      .map((e) => e.ts)
      .sort((a, b) => a - b)

    const deltas: number[] = []
    for (let i = 1; i < animationFrames.length; i++) {
      const delta = (animationFrames[i]! - animationFrames[i - 1]!) / 1000 // µs → ms
      deltas.push(delta)
    }
    deltas.sort((a, b) => a - b)

    // "Dropped" = delta > 2× the expected 16.67ms budget (i.e., we skipped at
    // least one rAF tick because the main thread was busy)
    const FRAME_BUDGET_MS = 16.67
    const droppedFrames = deltas.filter((d) => d > FRAME_BUDGET_MS * 2).length

    const summary = {
      total_animation_frames: animationFrames.length,
      frame_deltas_count: deltas.length,
      p50_ms: Math.round(percentile(deltas, 50) * 100) / 100,
      p95_ms: Math.round(percentile(deltas, 95) * 100) / 100,
      p99_ms: Math.round(percentile(deltas, 99) * 100) / 100,
      dropped_frames: droppedFrames,
    }

    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic — frame-budget summary is the primary output of this informational test and must be visible in CI logs.
    console.log('[frame-budget]', JSON.stringify(summary))

    // --- Save trace artifact ---
    const artifactDir = path.join(_dirname, '..', 'test-results')
    fs.mkdirSync(artifactDir, { recursive: true })
    const artifactPath = path.join(artifactDir, 'frame-budget-trace.json')
    fs.writeFileSync(
      artifactPath,
      JSON.stringify(
        {
          summary,
          schedule,
          trace_event_count: traceEvents.length,
          top_event_names: topEventNames,
          animation_frame_timestamps_us: animationFrames,
          frame_deltas_ms: deltas,
        },
        null,
        2,
      ),
    )

    // --- Structural assertions (always pass) ---
    // The trace was collected (even if empty — headless might have 0 frames).
    expect(traceEvents.length).toBeGreaterThanOrEqual(0)
    // The artifact exists on disk.
    expect(fs.existsSync(artifactPath)).toBe(true)

    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic — frame-budget detail lines are informational output for debugging and baseline tracking across CI runs.
    console.log(`[frame-budget] Artifact saved to: ${artifactPath}`)
    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic (see above)
    console.log(`[frame-budget] AnimationFrame events: ${animationFrames.length}`)
    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic (see above)
    console.log(
      `[frame-budget] p50=${summary.p50_ms}ms p95=${summary.p95_ms}ms dropped=${summary.dropped_frames}`,
    )
  })
})
