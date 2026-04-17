import type { Route } from '@playwright/test'
import { expect, test } from '@playwright/test'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'

test.describe('Kanban board', () => {
  /**
   * AC8.1: Full lifecycle verification
   * Verify hydration placeholder → empty state → populated board transitions
   */
  test('AC8.1: hydration → empty → populated board lifecycle', async ({ page }) => {
    let stateRoute: Route | null = null

    // Set up WebSocket mock BEFORE navigation
    await page.routeWebSocket('**/v1/ws', (ws) => {
      ws.onMessage(() => {
        // No-op
      })
    })

    // Set up HTTP state mock BEFORE navigation
    await page.route('**/v1/state', (route) => {
      stateRoute = route
      // Deliberately don't fulfill immediately — we want to see "Connecting..." first
    })

    // Navigate to the app
    await page.goto('/')

    // Step 1: Verify hydration placeholder is visible while connecting
    // (WS is open but /v1/state hasn't resolved yet)
    await expect(page.getByText(/Connecting/)).toBeVisible()

    // Step 2: Now fulfill the /v1/state route with empty snapshot
    if (stateRoute) {
      await stateRoute.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    }

    // Wait for connection to complete and empty state to show
    await expect(page.getByText('No workflows yet.')).toBeVisible()

    // Step 3: Verify the empty state shows correct styling and layout
    // (This demonstrates hydration completed successfully)
    const emptyText = page.getByText('No workflows yet.')
    await expect(emptyText).toBeVisible()

    // Step 3b: Navigate to a fresh page with data already in the snapshot
    // to verify the board populates correctly from the initial state
    await page.goto('/')

    // Set up a new state route that returns a run
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [
            {
              id: 1001,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test commit',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1001',
              createdAt: '2026-04-16T10:00:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-16T10:00:00Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    // The board should populate with columns and card
    await expect(page.getByRole('heading', { name: 'QUEUED' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'IN PROGRESS' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'COMPLETED' })).toBeVisible()

    // The card should appear in the QUEUED column
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    const card = queuedSection.locator('article[role="listitem"][data-run-id="1001"]')
    await expect(card).toBeVisible()
  })

  /**
   * AC8.2: Card movement through lifecycle
   * Start with a run in Queued → move to InProgress → move to Completed
   * Tests that the board correctly renders and updates runs in different columns
   */
  test('AC8.2: card moves between columns via state updates', async ({ page }) => {
    // Set up WebSocket mock
    await page.routeWebSocket('**/v1/ws', (ws) => {
      ws.onMessage(() => {
        // No-op
      })
    })

    // Set up HTTP state mock with initial run in Queued state
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

    // Verify initial state: card in QUEUED column
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    await expect(queuedSection.locator('h2')).toBeVisible()
    const card = queuedSection.locator('article[role="listitem"][data-run-id="1002"]').first()
    await expect(card).toBeVisible()

    // Verify NOT in other columns
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()
    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()

    // Simulate a state update where the run transitions to InProgress
    // (In real usage, this comes from a WS event, but we verify the board state here)
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 2,
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
              status: 'InProgress',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1002',
              createdAt: '2026-04-16T11:00:00Z',
              runStartedAt: '2026-04-16T11:00:30Z',
              updatedAt: '2026-04-16T11:00:30Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    // Reload to get updated state (simulating the WS event effect)
    await page.goto('/')

    // Verify card moved to IN PROGRESS column
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1002"]').first(),
    ).toBeVisible()
    // Verify it's no longer in QUEUED
    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()
    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()

    // Simulate final transition to Completed
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 3,
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
              status: 'Completed',
              conclusion: 'Success',
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1002',
              createdAt: '2026-04-16T11:00:00Z',
              runStartedAt: '2026-04-16T11:00:30Z',
              updatedAt: '2026-04-16T11:00:45Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    // Reload to get updated state
    await page.goto('/')

    // Verify card moved to COMPLETED column
    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1002"]').first(),
    ).toBeVisible()
    // Verify it's no longer in other columns
    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()

    // Capture screenshot of completed board state
    await page.screenshot({ path: 'e2e/screenshots/kanban-populated.png', fullPage: true })
  })

  /**
   * AC8.3: Reduced motion variant
   * Verify the board functions correctly with prefers-reduced-motion without console errors
   */
  test('AC8.3: reduced motion variant completes lifecycle without errors', async ({ page }) => {
    // Enable reduced motion BEFORE navigation
    await page.emulateMedia({ reducedMotion: 'reduce' })

    const consoleErrors: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        consoleErrors.push(msg.text())
      }
    })

    // Set up WebSocket mock
    await page.routeWebSocket('**/v1/ws', (ws) => {
      ws.onMessage(() => {
        // No-op
      })
    })

    // Set up HTTP state mock with initial run in Queued state
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

    // Wait for board to load
    const queuedSectionRM = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    await expect(queuedSectionRM.locator('h2')).toBeVisible()

    // Verify the card is in Queued state initially
    await expect(
      queuedSectionRM.locator('article[role="listitem"][data-run-id="1003"]').first(),
    ).toBeVisible()

    // Transition to InProgress
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 2,
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
              status: 'InProgress',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1003',
              createdAt: '2026-04-16T12:00:00Z',
              runStartedAt: '2026-04-16T12:00:30Z',
              updatedAt: '2026-04-16T12:00:30Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    // Verify transition
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1003"]').first(),
    ).toBeVisible()

    // Transition to Completed
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 3,
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
              status: 'Completed',
              conclusion: 'Success',
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1003',
              createdAt: '2026-04-16T12:00:00Z',
              runStartedAt: '2026-04-16T12:00:30Z',
              updatedAt: '2026-04-16T12:00:45Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    // Verify final transition
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })
    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1003"]').first(),
    ).toBeVisible()

    // Verify zero console errors after full lifecycle
    expect(consoleErrors).toEqual([])
  })
})
