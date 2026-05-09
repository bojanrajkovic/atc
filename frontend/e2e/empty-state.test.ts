import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E tests for EmptyState component.
 *
 * AC1.1: EmptyState renders with default caption when connected + 0 runs.
 * AC1.4: Connecting… placeholder shown before connected; EmptyState NOT shown.
 */
test.describe('EmptyState', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  /**
   * AC1.4: While still connecting, the hydration placeholder appears — NOT EmptyState.
   */
  test('AC1.4 — shows Connecting… placeholder before connection, not EmptyState', async ({
    page,
  }) => {
    let stateRoute: Parameters<Parameters<typeof page.route>[1]>[0] | null = null

    // Delay /v1/state so we can observe the connecting placeholder
    await page.route('**/v1/state', (route) => {
      stateRoute = route
    })

    await page.goto('/')

    // Hydration placeholder visible while connecting
    await expect(page.getByText(/Connecting/)).toBeVisible()

    // EmptyState caption should NOT be visible yet
    await expect(page.getByText('Watching for runs.')).not.toBeVisible()

    // Fulfill with empty snapshot — connection is now established
    await stateRoute!.fulfill({
      contentType: 'application/json',
      body: JSON.stringify(
        { lastSeq: 1n, runs: [], jobs: [] } satisfies StateSnapshot,
        bigintReplacer,
      ),
    })

    // AC1.1: EmptyState caption now shows
    await expect(page.getByText('Watching for runs.')).toBeVisible()
    // Connecting placeholder gone
    await expect(page.getByText(/Connecting/)).not.toBeVisible()
  })

  /**
   * AC1.1: EmptyState renders when connected with zero runs.
   */
  test('AC1.1 — EmptyState shows "Watching for runs." when connected with 0 runs', async ({
    page,
  }) => {
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

    await expect(page.getByText('Watching for runs.')).toBeVisible()

    // Schematic column group labels should be visible
    await expect(page.getByText('Queued')).toBeVisible()
    await expect(page.getByText('Running')).toBeVisible()
    await expect(page.getByText('Completed')).toBeVisible()
  })

  /**
   * EmptyState disappears when a run arrives.
   */
  test('EmptyState disappears when first run arrives via WS event', async ({ page }) => {
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

    // Confirm EmptyState is visible initially
    await expect(page.getByText('Watching for runs.')).toBeVisible()

    // Send a run event
    await sendWS(
      page,
      makeRunEvent(2, {
        runId: 999,
        displayTitle: 'Deploy — main',
        createdAt: '2026-05-02T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-05-02T10:00:00Z',
        action: { type: 'Requested' },
      }),
    )

    // EmptyState should be gone; card appears
    await expect(page.getByText('Watching for runs.')).not.toBeVisible()
    await expect(page.locator('article[data-run-id="999"]')).toBeVisible()
  })
})
