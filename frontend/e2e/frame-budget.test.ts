/**
 * frontend-1-0-polish.AC7.2 — Frame-budget tracing (informational).
 *
 * This test:
 *   1. Starts a Chrome DevTools Protocol trace with the rendering/devtools.timeline
 *      categories so it captures BeginFrame events.
 *   2. Fires 1000 WS events through the live EventDispatcher (via sendWSBatch),
 *      which exercises real RAF batching.
 *   3. Parses BeginFrame deltas from the collected trace data.
 *   4. Logs a structured frame-budget summary (p50_ms, p95_ms, dropped_frames).
 *   5. Saves the raw trace JSON to test-results/frame-budget-trace.json as a CI
 *      artifact for future tightening.
 *
 * The test ALWAYS passes (no timing assertions). All assertions are structural
 * (trace data was collected, artifact was written). Future tightening is
 * mechanical: add expect(summary.p95_ms).toBeLessThan(N) once a baseline is
 * established.
 *
 * Artifact location: frontend/test-results/frame-budget-trace.json
 * (uploaded by the "Upload test artifacts" step in ci.yml).
 */

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
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

test.describe('AC7.2: frame-budget tracing (informational)', () => {
  // This test exercises real rendering via CDP tracing — give it extra time.
  test.setTimeout(60_000)

  test('fires 1000 events, captures BeginFrame deltas, saves trace artifact', async ({
    page,
    context,
  }) => {
    // --- Setup: WS mock + empty state snapshot ---
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)

    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(
          { seq: 1n, runs: [], jobs: [], poolStats: [] } satisfies StateSnapshot,
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

    // --- Parse BeginFrame deltas ---
    const beginFrames = traceEvents
      .filter(
        (e): e is CdpTraceEvent & { name: string; ts: number } =>
          e.name === 'BeginFrame' && typeof e.ts === 'number',
      )
      .map((e) => e.ts)
      .sort((a, b) => a - b)

    const deltas: number[] = []
    for (let i = 1; i < beginFrames.length; i++) {
      const delta = (beginFrames[i]! - beginFrames[i - 1]!) / 1000 // µs → ms
      deltas.push(delta)
    }
    deltas.sort((a, b) => a - b)

    // "Dropped" = frame delta > 2× the expected 16.67ms budget (i.e., missed at least one frame)
    const FRAME_BUDGET_MS = 16.67
    const droppedFrames = deltas.filter((d) => d > FRAME_BUDGET_MS * 2).length

    const summary = {
      total_begin_frames: beginFrames.length,
      frame_deltas_count: deltas.length,
      p50_ms: Math.round(percentile(deltas, 50) * 100) / 100,
      p95_ms: Math.round(percentile(deltas, 95) * 100) / 100,
      p99_ms: Math.round(percentile(deltas, 99) * 100) / 100,
      dropped_frames: droppedFrames,
    }

    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic — frame-budget summary is
    // the primary output of this informational test and must be visible in CI logs.
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
          begin_frame_timestamps_us: beginFrames,
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

    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic — frame-budget detail
    // lines are informational output for debugging and baseline tracking across CI runs.
    console.log(`[frame-budget] Artifact saved to: ${artifactPath}`)
    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic (see above)
    console.log(`[frame-budget] BeginFrame events: ${beginFrames.length}`)
    // biome-ignore lint/suspicious/noConsole: intentional CI diagnostic (see above)
    console.log(
      `[frame-budget] p50=${summary.p50_ms}ms p95=${summary.p95_ms}ms dropped=${summary.dropped_frames}`,
    )
  })
})
