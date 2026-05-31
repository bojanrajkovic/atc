import type { Page } from '@playwright/test'
import { createMockRun } from '../src/lib/test-utils/factories'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import type { WorkflowRun } from '../src/lib/types/generated/WorkflowRun'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Responsive layout tests. Covers three column breakpoints, TopBar wrapping,
 * horizontal scroll prevention, and column-level scroll behavior.
 */

/** Build a minimal WorkflowRun fixture for snapshot injection */
function makeRun(id: number, status: WorkflowRun['status']): WorkflowRun {
  return createMockRun({
    id: BigInt(id),
    status,
    displayTitle: `Run ${id}`,
    htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${id}`,
    createdAt: '2026-05-02T10:00:00Z',
    runStartedAt: status === 'Queued' ? null : '2026-05-02T10:00:10Z',
    updatedAt: '2026-05-02T10:00:00Z',
  })
}

/** Fulfill /v1/state with runs in all three columns */
async function setupWithRuns(page: Page) {
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify(
        {
          lastSeq: 1n,
          runs: [makeRun(1, 'Queued'), makeRun(2, 'InProgress'), makeRun(3, 'Completed')],
          jobs: [],
          runnerPoolCapacities: [],
          displayTtlSeconds: 0,
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

  test('1280px width: kanban renders three columns', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    // Three tracks: three non-empty segments when split on spaces
    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(3)
  })

  test('900px width: kanban renders two columns', async ({ page }) => {
    await page.setViewportSize({ width: 900, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(2)
  })

  test('480px width: kanban renders single column', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const grid = page.locator('[data-kanban-grid]')
    const gridStyle = await grid.evaluate((el) => getComputedStyle(el).gridTemplateColumns)

    const tracks = gridStyle.trim().split(/\s+(?=\d)/)
    expect(tracks.length).toBe(1)
  })

  test('640px width: TopBar wraps to two rows (logo+connection / runnerbar+settings)', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 640, height: 800 })
    await setupWithRuns(page)

    // Helper: compute vertical midpoint from bounding rect
    type Rect = { top: number; bottom: number; midY: number }
    const getRect = async (locator: ReturnType<typeof page.locator>): Promise<Rect> =>
      locator.evaluate((el: Element) => {
        const r = el.getBoundingClientRect()
        return { top: r.top, bottom: r.bottom, midY: (r.top + r.bottom) / 2 }
      })

    const logo = await getRect(page.getByLabel(/ATC — Actions Traffic Control/i))
    const connection = await getRect(page.locator('header [role="status"]').first())
    const runnerBar = await getRect(page.locator('[data-runner-bar]'))
    const settings = await getRect(page.getByRole('button', { name: /settings/i }))

    const TOLERANCE = 4 // px — accounts for sub-pixel rounding

    // Row 1: Logo and ConnectionIndicator share the same row (midpoints within tolerance)
    expect(Math.abs(logo.midY - connection.midY)).toBeLessThanOrEqual(TOLERANCE)

    // Row 2: RunnerBar and SettingsPopover share the same row (midpoints within tolerance)
    expect(Math.abs(runnerBar.midY - settings.midY)).toBeLessThanOrEqual(TOLERANCE)

    // Row 2 is below row 1
    expect(runnerBar.top).toBeGreaterThan(logo.bottom - TOLERANCE)
  })

  test('1280px width: TopBar all elements on one row', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await setupWithRuns(page)

    const getMidY = async (locator: ReturnType<typeof page.locator>): Promise<number> =>
      locator.evaluate((el: Element) => {
        const r = el.getBoundingClientRect()
        return (r.top + r.bottom) / 2
      })

    const logoMidY = await getMidY(page.getByLabel(/ATC — Actions Traffic Control/i))
    const connectionMidY = await getMidY(page.locator('header [role="status"]').first())
    const runnerBarMidY = await getMidY(page.locator('[data-runner-bar]'))
    const settingsMidY = await getMidY(page.getByRole('button', { name: /settings/i }))

    const TOLERANCE = 4 // px

    // At md+ all four elements should share the same row (midpoints within tolerance)
    expect(Math.abs(logoMidY - connectionMidY)).toBeLessThanOrEqual(TOLERANCE)
    expect(Math.abs(logoMidY - runnerBarMidY)).toBeLessThanOrEqual(TOLERANCE)
    expect(Math.abs(logoMidY - settingsMidY)).toBeLessThanOrEqual(TOLERANCE)
  })

  for (const width of [320, 480, 640, 900, 1280]) {
    test(`no horizontal scroll at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 800 })
      await setupWithRuns(page)

      const hasHScroll = await page.evaluate(() => {
        return document.documentElement.scrollWidth > document.documentElement.clientWidth
      })

      expect(hasHScroll).toBe(false)
    })
  }

  test('480px width: column bodies do not scroll independently', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const queuedList = page.locator('[data-kanban-grid] [role="list"]').first()
    const overflowY = await queuedList.evaluate((el) => getComputedStyle(el).overflowY)
    expect(overflowY).toBe('visible')
  })

  test('480px width: column headers are sticky', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const header = page.locator('[data-column-header]').first()
    const position = await header.evaluate((el) => getComputedStyle(el).position)
    expect(position).toBe('sticky')
  })

  test('1280px width: column bodies scroll independently', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await setupWithRuns(page)

    const queuedList = page.locator('[data-kanban-grid] [role="list"]').first()
    const overflowY = await queuedList.evaluate((el) => getComputedStyle(el).overflowY)
    expect(overflowY).toBe('auto')
  })
})
