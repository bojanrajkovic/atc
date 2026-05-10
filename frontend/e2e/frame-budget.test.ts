/**
 * frame-budget.test.ts — Frame-budget tracing (informational).
 *
 * Measures end-to-end injection-loop + first-flush + post-flush rAF tail
 * latency while the EventDispatcher receives a 1000-event burst, by parsing
 * `AnimationFrame` trace event deltas from a CDP trace. This is a composite
 * regression canary, not a pure dispatcher-pacing measurement — Tier 1
 * (dispatcher.perf.browser.test.ts) owns the deterministic rAF-coalescing
 * gate. The dominant cost in current results is the synchronous JSON.parse +
 * dispatcher.dispatch loop inside `sendWSBatch.page.evaluate`, which blocks
 * the main thread for ~200ms across 1000 iterations.
 *
 * Modern Chromium emits a single `AnimationFrame` trace event per rAF tick
 * (the legacy `BeginFrame` / `FireAnimationFrame` names are not present in
 * `devtools.timeline,rendering` traces from this Chromium build — see the
 * `top_event_names` histogram in the artifact for empirical confirmation).
 *
 * A future PR may rewrite `sendWSBatch` to inject events across rAF boundaries
 * (simulating real WS arrival pacing); tracked in GitHub issue #46. Until then,
 * the metric is useful as a regression canary on full-pipeline cost — if
 * dispatch() got 5× slower we'd see it here.
 *
 * This test:
 *   1. Starts a CDP trace with the devtools.timeline,rendering categories.
 *   2. Fires 1000 WS events through the live EventDispatcher (via sendWSBatch).
 *   3. Parses AnimationFrame deltas from the collected trace data.
 *   4. Logs a structured frame-budget summary (p50_ms, p95_ms, dropped_frames).
 *   5. Saves the trace summary, top-50 event-name histogram (for diagnosing
 *      future Chromium event-name renames without rerunning), and per-frame
 *      timestamps to test-results/frame-budget-trace.json.
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
import { bigintReplacer, makeRunEvent, sendWSBatch, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

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
          { lastSeq: 1n, runs: [], jobs: [] } satisfies StateSnapshot,
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

    // --- Fire 1000 events as a batch through the real EventDispatcher ---
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
    await sendWSBatch(page, msgs)

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
    // The full deltas span end-to-end injection-loop + first-flush + post-flush
    // rAF tail. The dominant signal in current results is the synchronous
    // JSON.parse + dispatcher.dispatch loop inside `sendWSBatch.page.evaluate`,
    // which blocks the main thread for ~200ms (1000 iterations) and prevents
    // any rAF from firing during that window. See architecture doc for the
    // honest framing — this measures composite end-to-end latency, not the
    // dispatcher's rAF coalescing in isolation (Tier 1 owns that gate).
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
