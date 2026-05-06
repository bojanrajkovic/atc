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
          lastSeq: 1n,
          runs: [makeRun(1, 'Queued'), makeRun(2, 'InProgress'), makeRun(3, 'Completed')],
          jobs: [],
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
   * AC2.4: At <768px, TopBar wraps to exactly two rows:
   *   Row 1 — Logo + ConnectionIndicator (same vertical center)
   *   Row 2 — RunnerBar + SettingsPopover (same vertical center, below row 1)
   *
   * Bounding-rect approach: elements sharing a flex line with items-center
   * alignment have the same vertical midpoint. Elements on a lower line have
   * a strictly greater midpoint.
   */
  test('AC2.4 — 640px width: TopBar wraps to two rows (logo+connection / runnerbar+settings)', async ({
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

  /**
   * AC2.4 (md+): At ≥768px, all TopBar elements collapse onto one row.
   * Verified by checking logo, connection indicator, runner bar, and settings
   * all share the same vertical midpoint.
   */
  test('AC2.4 — 1280px width: TopBar all elements on one row', async ({ page }) => {
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

  /**
   * AC2.6: At <sm, the kanban scroll is unified across stacked columns —
   * column bodies do not scroll independently. The unified scroll lives on
   * <main>, and column headers are `position: sticky` so each pins to the
   * top of <main>'s viewport while its column section is in view.
   *
   * At sm+, columns regain independent vertical scroll (overflow-y: auto on
   * the column body), and `sticky` on the header is functionally a no-op
   * because <main> no longer scrolls.
   */
  test('AC2.6 — 480px width: column bodies do not scroll independently', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const queuedList = page.locator('[data-kanban-grid] [role="list"]').first()
    const overflowY = await queuedList.evaluate((el) => getComputedStyle(el).overflowY)
    expect(overflowY).toBe('visible')
  })

  test('AC2.6 — 480px width: column headers are sticky', async ({ page }) => {
    await page.setViewportSize({ width: 480, height: 800 })
    await setupWithRuns(page)

    const header = page.locator('[data-column-header]').first()
    const position = await header.evaluate((el) => getComputedStyle(el).position)
    expect(position).toBe('sticky')
  })

  test('AC2.6 — 1280px width: column bodies scroll independently', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 800 })
    await setupWithRuns(page)

    const queuedList = page.locator('[data-kanban-grid] [role="list"]').first()
    const overflowY = await queuedList.evaluate((el) => getComputedStyle(el).overflowY)
    expect(overflowY).toBe('auto')
  })
})
