import type { Route } from '@playwright/test'
import { expect, test } from '@playwright/test'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

test.describe('Kanban board', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  /**
   * AC8.1: Full lifecycle verification
   * Hydration placeholder → empty state → populated board (via WS event)
   */
  test('AC8.1: hydration → empty → populated board lifecycle', async ({ page }) => {
    let stateRoute: Route | null = null

    // Delay /v1/state fulfillment to observe hydration placeholder
    await page.route('**/v1/state', (route) => {
      stateRoute = route
    })

    await page.goto('/')

    // Step 1: Hydration placeholder visible while connecting
    await expect(page.getByText(/Connecting/)).toBeVisible()

    // Step 2: Fulfill empty snapshot → "No workflows yet."
    await stateRoute!.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        seq: 1,
        runs: [],
        jobs: [],
        poolStats: [],
      } satisfies StateSnapshot),
    })
    await expect(page.getByText('No workflows yet.')).toBeVisible()

    // Step 3: Send WS event → board populates
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1001,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-04-16T10:00:00Z',
        action: { type: 'Requested' },
      }),
    )

    // Column headers appear
    await expect(page.getByRole('heading', { name: 'QUEUED' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'IN PROGRESS' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'COMPLETED' })).toBeVisible()

    // Card appears in QUEUED column
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    await expect(queuedSection.locator('article[data-run-id="1001"]')).toBeVisible()
  })

  /**
   * AC8.2: Card movement through lifecycle via WS events
   * Queued → InProgress → Completed within a single page session
   */
  test('AC8.2: card moves between columns as run status changes', async ({ page }) => {
    // Start with one run in Queued via initial snapshot
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [
            {
              id: 1002,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'def456',
              commitMessage: 'another commit',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1002',
              createdAt: '2026-04-16T11:00:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-16T11:00:00Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    // Section locators
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })

    // Verify initial state: card in QUEUED
    await expect(queuedSection.locator('article[data-run-id="1002"]')).toBeVisible()

    // Send WS event: Queued → InProgress
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1002,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T11:00:00Z',
        runStartedAt: '2026-04-16T11:00:30Z',
        updatedAt: '2026-04-16T11:00:30Z',
        action: { type: 'InProgress' },
      }),
    )

    // Card moves to IN PROGRESS column
    await expect(inProgressSection.locator('article[data-run-id="1002"]')).toBeVisible()
    await expect(queuedSection.locator('article[data-run-id="1002"]')).not.toBeVisible()

    // Send WS event: InProgress → Completed
    await sendWS(
      page,
      makeRunEvent(11, {
        runId: 1002,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T11:00:00Z',
        runStartedAt: '2026-04-16T11:00:30Z',
        updatedAt: '2026-04-16T11:00:45Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    // Card moves to COMPLETED column
    await expect(completedSection.locator('article[data-run-id="1002"]')).toBeVisible()
    await expect(inProgressSection.locator('article[data-run-id="1002"]')).not.toBeVisible()

    // Capture screenshot for visual regression check
    await page.screenshot({ path: 'e2e/screenshots/kanban-populated.png', fullPage: true })
  })

  /**
   * AC8.3: Reduced motion variant
   * Same lifecycle as AC8.2 with prefers-reduced-motion, zero console errors
   */
  test('AC8.3: reduced motion variant completes lifecycle without errors', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' })

    const consoleErrors: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleErrors.push(msg.text())
    })

    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [
            {
              id: 1003,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'Build',
              workflowPath: '.github/workflows/build.yml',
              branch: 'main',
              headSha: 'ghi789',
              commitMessage: 'reduced motion test',
              event: 'push',
              displayTitle: 'Build — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1003',
              createdAt: '2026-04-16T12:00:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-16T12:00:00Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })

    await expect(queuedSection.locator('article[data-run-id="1003"]')).toBeVisible()

    // Queued → InProgress via WS
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1003,
        displayTitle: 'Build — main',
        createdAt: '2026-04-16T12:00:00Z',
        runStartedAt: '2026-04-16T12:00:30Z',
        updatedAt: '2026-04-16T12:00:30Z',
        action: { type: 'InProgress' },
      }),
    )

    await expect(inProgressSection.locator('article[data-run-id="1003"]')).toBeVisible()

    // InProgress → Completed via WS
    await sendWS(
      page,
      makeRunEvent(11, {
        runId: 1003,
        displayTitle: 'Build — main',
        createdAt: '2026-04-16T12:00:00Z',
        runStartedAt: '2026-04-16T12:00:30Z',
        updatedAt: '2026-04-16T12:00:45Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    await expect(completedSection.locator('article[data-run-id="1003"]')).toBeVisible()

    // Zero console errors after full lifecycle
    expect(consoleErrors).toEqual([])
  })
})
