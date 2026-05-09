import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import {
  bigintReplacer,
  makeRunEvent,
  sendWS,
  sendWSBatch,
  WS_MOCK_INIT_SCRIPT,
} from './lib/ws-mock'

test.describe('ARIA live region (AC6)', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)

    // Fulfill empty state snapshot immediately so the app reaches "connected" fast
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
    // Wait for the app to connect
    await expect(page.locator('[aria-label="Workflow run updates"]')).toBeAttached()
  })

  /**
   * AC6.1 — The live region element mounts with the correct static attributes.
   */
  test('AC6.1 — live region mounts with correct ARIA attributes', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    await expect(region).toHaveAttribute('aria-live', 'polite')
    await expect(region).toHaveAttribute('aria-atomic', 'true')
    await expect(region).toHaveAttribute('aria-busy', 'false')
    await expect(region).toHaveAttribute('aria-label', 'Workflow run updates')
    await expect(region).toHaveClass(/sr-only/)
  })

  /**
   * AC6.2 — A single Queued transition (≤3) produces a per-run message
   * with the "queued" verb, and aria-busy stays "false".
   */
  test('AC6.2 — single Queued event produces per-run message', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1001,
        displayTitle: 'Deploy to prod',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    )

    await expect(region).toHaveText(
      'Run Deploy to prod for test-org/test-repo on main (push) queued',
    )
    await expect(region).toHaveAttribute('aria-busy', 'false')
  })

  /**
   * AC6.2 — A Completed event (≤3) produces the conclusion-specific verb.
   */
  test('AC6.2 — single Completed event produces conclusion verb "succeeded"', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // First queue a run so it exists in the store
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 2001,
        displayTitle: 'CI suite',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: '2026-05-02T10:00:01Z',
        updatedAt: '2026-05-02T10:00:01Z',
        action: { type: 'InProgress' },
      }),
    )

    await sendWS(
      page,
      makeRunEvent(11, {
        runId: 2001,
        displayTitle: 'CI suite',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: '2026-05-02T10:00:01Z',
        updatedAt: '2026-05-02T10:00:30Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    await expect(region).toHaveText('Run CI suite for test-org/test-repo on main (push) succeeded')
    await expect(region).toHaveAttribute('aria-busy', 'false')
  })

  /**
   * AC6.2 — Multiple transitions in the same flush (≤3) are joined by ". "
   */
  test('AC6.2 — two transitions below threshold joined by period-space', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // Both events dispatched and flushed together via sendWSBatch (≤3 events)
    await sendWSBatch(page, [
      makeRunEvent(10, {
        runId: 3001,
        displayTitle: 'Run A',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(11, {
        runId: 3002,
        displayTitle: 'Run B',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    ])

    await expect(region).toHaveText(
      'Run Run A for test-org/test-repo on main (push) queued. Run Run B for test-org/test-repo on main (push) queued',
    )
    await expect(region).toHaveAttribute('aria-busy', 'false')
  })

  /**
   * AC6.4 — When branch is null, the "on {branch}" segment is elided.
   * makeRunEvent always uses branch='main' — we need to override it.
   */
  test('AC6.4 — null branch elides "on {branch}" segment', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // Inject a run event with branch: null by using page.evaluate
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
      const dispatcher = (window as any).eventDispatcher
      const seqEvent = {
        seq: BigInt(20),
        event: {
          type: 'Run',
          data: {
            runId: BigInt(4001),
            org: 'test-org',
            repo: 'test-repo',
            workflowName: 'CI',
            workflowPath: '.github/workflows/ci.yml',
            branch: null,
            headSha: 'abc123',
            commitMessage: 'test commit',
            triggerEvent: 'push',
            displayTitle: 'Null branch run',
            htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/4001',
            createdAt: '2026-05-02T10:00:00Z',
            runStartedAt: null,
            updatedAt: '2026-05-02T10:00:00Z',
            action: { type: 'Requested' },
          },
        },
      }
      dispatcher.dispatch(seqEvent)
      dispatcher.flush()
    })

    await expect(region).toHaveText('Run Null branch run for test-org/test-repo (push) queued')
  })

  /**
   * AC6.3 — More than 3 transitions in a flush: aria-busy flips to "true"
   * and eventually resolves to a summary message with aria-busy="false".
   */
  test('AC6.3 — burst (>3 transitions) sets aria-busy="true" then resolves to summary', async ({
    page,
  }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // Send 4 events (above burst threshold of 3)
    await sendWSBatch(page, [
      makeRunEvent(30, {
        runId: 5001,
        displayTitle: 'Run 1',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(31, {
        runId: 5002,
        displayTitle: 'Run 2',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(32, {
        runId: 5003,
        displayTitle: 'Run 3',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(33, {
        runId: 5004,
        displayTitle: 'Run 4',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    ])

    // aria-busy should be "true" immediately after the burst
    await expect(region).toHaveAttribute('aria-busy', 'true')

    // After 200ms debounce, aria-busy resolves to "false" with summary
    await expect(region).toHaveAttribute('aria-busy', 'false', { timeout: 1000 })
    await expect(region).toHaveText(/4 runs queued/)
  })

  /**
   * AC6.3 (multi-flush) — A 2-transition flush during an open burst window
   * contributes to the accumulated counts (even though 2 ≤ 3).
   */
  test('AC6.3 — multi-flush burst: subsequent flush within debounce window adds to counts', async ({
    page,
  }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // Open a burst with 4 events
    await sendWSBatch(page, [
      makeRunEvent(40, {
        runId: 6001,
        displayTitle: 'Run 1',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(41, {
        runId: 6002,
        displayTitle: 'Run 2',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(42, {
        runId: 6003,
        displayTitle: 'Run 3',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(43, {
        runId: 6004,
        displayTitle: 'Run 4',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    ])

    // Verify burst is open
    await expect(region).toHaveAttribute('aria-busy', 'true')

    // Send 2 more events within the debounce window (this is ≤3 but window is open)
    await sendWS(
      page,
      makeRunEvent(44, {
        runId: 6005,
        displayTitle: 'Run 5',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    )

    // After debounce closes, summary should show 5 queued (not just 4)
    await expect(region).toHaveAttribute('aria-busy', 'false', { timeout: 1000 })
    await expect(region).toHaveText(/5 runs queued/)
  })

  /**
   * AC6.3 — Summary elides absent conclusions (e.g., if no failures, no "0 failed").
   */
  test('AC6.3 — summary elides absent conclusion counts', async ({ page }) => {
    const region = page.locator('[aria-label="Workflow run updates"]')

    // 4 Queued events → burst → summary should NOT mention "failed", "cancelled", etc.
    await sendWSBatch(page, [
      makeRunEvent(50, {
        runId: 7001,
        displayTitle: 'R1',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(51, {
        runId: 7002,
        displayTitle: 'R2',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(52, {
        runId: 7003,
        displayTitle: 'R3',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
      makeRunEvent(53, {
        runId: 7004,
        displayTitle: 'R4',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    ])

    await expect(region).toHaveAttribute('aria-busy', 'false', { timeout: 1000 })
    const text = await region.textContent()
    expect(text).toMatch(/4 runs queued/)
    // Should not mention failed/cancelled in a queued-only burst
    expect(text).not.toContain('failed')
    expect(text).not.toContain('cancelled')
  })
})
