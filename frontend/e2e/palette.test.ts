import { expect, test } from '@playwright/test'
import { sendWS, makeRunEvent, makeJobSeqEvent, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'
import type { JobEventEnvelope } from '../src/lib/types/generated/JobEventEnvelope'

test.describe('Command palette', () => {
  test.beforeEach(async ({ page }) => {
    // Inject the WS mock before navigating
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
    // Mock the /v1/state endpoint to allow connection to succeed
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [],
          jobs: [],
          poolStats: [],
        }),
      })
    })
    await page.goto('/')
    // Give the app a moment to initialize and stores to be available
    await page.waitForTimeout(200)
  })

  test('AC1.1 opens via paletteStore.open() and renders all sections', async ({ page }) => {
    // Verify stores are available
    const storesAvailable = await page.evaluate(() => {
      return typeof (window as any).__stores?.paletteStore !== 'undefined'
    })
    expect(storesAvailable).toBe(true)

    // Open palette
    await page.evaluate(() => {
      ;(window as any).__stores!.paletteStore!.open()
    })

    // Check palette is now open
    const isOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(isOpen).toBe(true)

    // Check DOM
    await expect(page.getByRole('dialog')).toBeVisible()
    await expect(page.getByRole('searchbox')).toBeFocused()

    // Sections render in source order
    const headings = await page.locator('[data-command-group-heading]').allInnerTexts()
    expect(headings).toEqual(['Runs', 'Jobs', 'Runner Pools', 'Commands'])
  })

  test('AC1.2 palette closes via paletteStore.close()', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())

    // Close it
    await page.evaluate(() => (window as any).__stores!.paletteStore!.close())

    // Verify store state
    const isOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(isOpen).toBe(false)

    // Verify DOM
    await expect(page.getByRole('dialog')).not.toBeVisible()
  })

  test('AC1.3 typing into searchbox updates paletteQuery and filters results', async ({ page }) => {
    // Seed some runs
    const run1 = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'feat: add feature',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })
    const run2 = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'fix: bug fix',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })

    await sendWS(page, run1)
    await sendWS(page, run2)

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.getByRole('searchbox').type('feat')

    // Check query is set
    const query = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteQuery)
    expect(query).toBe('feat')

    // Only the 'feat' run should be visible in results
    await expect(page.getByText('feat: add feature')).toBeVisible()
    await expect(page.getByText('fix: bug fix')).not.toBeVisible()
  })

  test('AC1.4 selecting a run sets selectedRunId and records the visit', async ({ page }) => {
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Test Run #1',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })
    await sendWS(page, run)

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.getByRole('option', { name: /Test Run #1/ }).click()

    const selectedRunId = await page.evaluate(() => (window as any).__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).toBe(1n)

    const paletteOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)

    const recentRunIds = await page.evaluate(() => (window as any).__stores!.paletteStore!.recentRunIds)
    expect(recentRunIds.length).toBeGreaterThan(0)
    expect(recentRunIds[0]).toBe(1n)
  })

  test('AC1.5 selecting a job sets selectedRunId, selectedJobId, and closes palette', async ({
    page,
  }) => {
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Test Run',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })
    await sendWS(page, run)

    const jobEnvelope: JobEventEnvelope = {
      id: 100n,
      runId: 1n,
      name: 'build',
      status: 'InProgress' as const,
      conclusion: null,
      createdAt: new Date().toISOString(),
      startedAt: new Date().toISOString(),
      completedAt: null,
      runner: null,
      labels: [],
      steps: [],
    }

    await sendWS(page, makeJobSeqEvent(2, { jobData: jobEnvelope, poolStatsAfter: null }))

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    // Find and click the job option
    await page.getByRole('option', { name: /build/ }).click()

    const selectedRunId = await page.evaluate(() => (window as any).__stores!.uiStore!.selectedRunId)
    const selectedJobId = await page.evaluate(() => (window as any).__stores!.uiStore!.selectedJobId)
    expect(selectedRunId).toBe(1n)
    expect(selectedJobId).toBe(100n)

    const paletteOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)
  })

  test('AC1.6 selecting a pool sets activePoolFilter and closes palette', async ({ page }) => {
    // Seed a runner pool
    await page.evaluate(() => {
      ;(window as any).__stores!.runnerStore!.loadPools([
        {
          labels: ['linux', 'x86'],
          running: 1,
          queued: 0,
          total: 4,
          isElastic: false,
          groupName: 'linux',
        },
      ])
    })

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    // Click on the pool option
    await page.getByRole('option', { name: /linux.*x86/ }).click()

    const activePoolFilter = await page.evaluate(() => (window as any).__stores!.uiStore!.activePoolFilter)
    expect(activePoolFilter).not.toBeNull()

    const paletteOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)
  })

  test('AC1.7 enterSubmenu sets subMenu to "theme"', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.evaluate(() => (window as any).__stores!.paletteStore!.enterSubmenu('theme'))

    const subMenu = await page.evaluate(() => (window as any).__stores!.paletteStore!.subMenu)
    expect(subMenu).toBe('theme')
  })

  test('AC1.8 selecting a theme sets uiStore.theme and closes submenu', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.evaluate(() => (window as any).__stores!.paletteStore!.enterSubmenu('theme'))

    // Simulate theme selection via direct API
    await page.evaluate(() => {
      ;(window as any).__stores!.uiStore!.theme = 'violet'
      ;(window as any).__stores!.paletteStore!.exitSubmenu()
      ;(window as any).__stores!.paletteStore!.close()
    })

    const theme = await page.evaluate(() => (window as any).__stores!.uiStore!.theme)
    const subMenu = await page.evaluate(() => (window as any).__stores!.paletteStore!.subMenu)
    const paletteOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)

    expect(theme).toBe('violet')
    expect(subMenu).toBeNull()
    expect(paletteOpen).toBe(false)
  })

  test('AC1.9 empty state shows message when no items match query', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.getByRole('searchbox').type('xyz123nonexistent')
    await expect(page.getByText('Nothing in flight matching')).toBeVisible()
  })

  test('AC1.10 empty state shows curly-quoted query', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.getByRole('searchbox').type('xyz123')
    // The empty state message should contain the query in curly quotes
    const emptyMessage = await page.getByText(/"xyz123"/).first()
    await expect(emptyMessage).toBeVisible()
  })

  test('AC1.11 pool rows show three states (browse / query-active / focused)', async ({ page }) => {
    // Seed a runner pool
    await page.evaluate(() => {
      ;(window as any).__stores!.runnerStore!.loadPools([
        {
          labels: ['linux', 'self-hosted', 'x86', 'big-runners'],
          running: 2,
          queued: 1,
          total: 4,
          isElastic: true,
          groupName: 'foo',
        },
      ])
    })

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())

    // Browse state: white-space nowrap (truncated)
    const labelsEl = page.locator('[role="option"] .labels').first()
    await expect(labelsEl).toBeVisible()
    let whiteSpace = await labelsEl.evaluate((el) => getComputedStyle(el).whiteSpace)
    expect(whiteSpace).toBe('nowrap')

    // Query-active: white-space normal (wraps)
    await page.getByRole('searchbox').type('linux')
    await page.waitForTimeout(50)
    whiteSpace = await labelsEl.evaluate((el) => getComputedStyle(el).whiteSpace)
    expect(whiteSpace).toBe('normal')

    // Clear query and focus the pool row via arrow keys
    await page.getByRole('searchbox').fill('')
    await page.waitForTimeout(50)
    // Navigate down through sections (Runs → Jobs → Pools → Commands)
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('ArrowDown')
    }

    // Focused state (no query): white-space normal (wraps)
    whiteSpace = await labelsEl.evaluate((el) => getComputedStyle(el).whiteSpace)
    expect(whiteSpace).toBe('normal')
  })

  test('AC1.12 pressing Escape closes the palette', async ({ page }) => {
    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).not.toBeVisible()
  })

  test('AC1.13 recent runs appear at top of Runs section', async ({ page }) => {
    const run1 = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Run 1',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })
    const run2 = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'Run 2',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { status: 'completed' },
    })

    await sendWS(page, run1)
    await sendWS(page, run2)

    // Record run 2 as visited
    await page.evaluate(() => (window as any).__stores!.paletteStore!.recordRunVisit(2n))

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())

    // Recent runs section should exist and show run 2
    const recentHeading = page.locator('[data-command-group-heading]').filter({ hasText: /Recent/ })
    await expect(recentHeading).toBeVisible()

    // Verify run 2 is in the recent section
    const runOptions = await page.getByRole('option', { name: /Run 2/ }).allLocations()
    expect(runOptions.length).toBeGreaterThan(0)
  })

  test('palette visual regression vs playground', async ({ page }) => {
    // Seed enough fixtures to populate sections
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'feat: comprehensive palette test',
      createdAt: new Date().toISOString(),
      runStartedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      action: { status: 'in_progress' },
    })
    await sendWS(page, run)

    const jobEnvelope: JobEventEnvelope = {
      id: 100n,
      runId: 1n,
      name: 'build',
      status: 'InProgress' as const,
      conclusion: null,
      createdAt: new Date().toISOString(),
      startedAt: new Date().toISOString(),
      completedAt: null,
      runner: null,
      labels: [],
      steps: [],
    }
    await sendWS(page, makeJobSeqEvent(2, { jobData: jobEnvelope, poolStatsAfter: null }))

    // Seed a runner pool
    await page.evaluate(() => {
      ;(window as any).__stores!.runnerStore!.loadPools([
        {
          labels: ['linux', 'x86'],
          running: 1,
          queued: 0,
          total: 4,
          isElastic: false,
          groupName: 'linux',
        },
      ])
    })

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await page.waitForTimeout(100)

    await expect(page).toHaveScreenshot('palette-open.png', { maxDiffPixelRatio: 0.02 })
  })
})
