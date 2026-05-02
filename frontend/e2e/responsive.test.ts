import { expect, type Page, test } from '@playwright/test'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import type { WorkflowRun } from '../src/lib/types/generated/WorkflowRun'
import { bigintReplacer, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Responsive layout tests (Phase 2).
 *
 * AC2.1: ≥1280px → 3 kanban columns
 * AC2.2: 640–1279px → 2 kanban columns (Completed wraps onto row 2)
 * AC2.3: <640px → 1 column stack
 * AC2.4: <768px → TopBar wraps to two rows
 * AC2.5: No horizontal scroll at any width ≥320px
 */

/** Build a minimal WorkflowRun fixture for snapshot injection */
function makeRun(id: number, status: WorkflowRun['status']): WorkflowRun {
  return {
    id: BigInt(id),
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'test',
    event: 'push',
    displayTitle: `Run ${id}`,
    status,
    conclusion: null,
    htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${id}`,
    createdAt: '2026-05-02T10:00:00Z',
    runStartedAt: status === 'Queued' ? null : '2026-05-02T10:00:10Z',
    updatedAt: '2026-05-02T10:00:00Z',
  }
}

/** Fulfill /v1/state with runs in all three columns */
async function setupWithRuns(page: Page) {
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify(
        {
          seq: 1n,
          runs: [makeRun(1, 'Queued'), makeRun(2, 'InProgress'), makeRun(3, 'Completed')],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot,
        bigintReplacer,
      ),
    })
  })

  await page.goto('/')
  // Wait until kanban is populated
  await expect(page.getByRole('heading', { name: 'QUEUED' })).toBeVisible()
}

test.describe('Responsive layout', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  /**
   * AC2.1: At ≥1280px, kanban renders 3 columns.
   */
  test('AC2.1 — 1280px width: kanban renders three columns', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    // Three tracks: three non-empty segments when split on spaces
    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(3)
  })

  /**
   * AC2.2: At 900px (640–1279px), kanban renders 2 columns.
   */
  test('AC2.2 — 900px width: kanban renders two columns', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(2)
  })

  /**
   * AC2.3: At 480px (<640px), kanban renders 1 column.
   */
  test('AC2.3 — 480px width: kanban renders single column', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(1)
  })

  /**
   * AC2.4: At <768px, TopBar wraps to two rows (logo+connection on row 1,
   * RunnerBar+settings on row 2). Verified by checking the offset top of the
   * RunnerBar wrapper differs from the Logo offset top.
   */
  test('AC2.4 — 640px width: TopBar RunnerBar is on a different row than Logo', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 640, height: 800 })
    await setupWithRuns(page)

    const logoTop = await page
      .getByLabel(/ATC — Actions Traffic Control/i)
      .evaluate((el: Element) => el.getBoundingClientRect().top)

    const runnerBarTop = await page
      .locator('[data-runner-bar]')
      .evaluate((el) => el.getBoundingClientRect().top)

    // RunnerBar should be on a lower row than the logo
    expect(runnerBarTop).toBeGreaterThan(logoTop)
  })

  /**
   * AC2.5: No horizontal page scroll at widths 320px, 480px, 640px, 900px, 1280px.
   */
  for (const width of [320, 480, 640, 900, 1280]) {
    test(`AC2.5 — no horizontal scroll at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 })
      await setupWithRuns(page)

      const hasHScroll = await page.evaluate(() => {
        return document.documentElement.scrollWidth > document.documentElement.clientWidth
      })

      expect(hasHScroll).toBe(false)
    })
  }
})
