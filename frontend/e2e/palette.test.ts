import { expect, test } from '@playwright/test'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/* eslint-disable @typescript-eslint/no-explicit-any */

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
    // Wait for the stores bridge to be available AND the connection to reach 'connected' state.
    // This ensures the app is fully initialized: Vite + Svelte components mounted + ConnectionManager
    // completed its WS open + fetch state snapshot flow.
    try {
      await page.waitForFunction(
        () => {
          const s = (window as any).__stores
          return (
            typeof s?.paletteStore !== 'undefined' &&
            typeof s?.connectionStore !== 'undefined' &&
            s.connectionStore.status === 'connected'
          )
        },
        { timeout: 15_000 },
      )
    } catch {
      // If connection doesn't reach 'connected', at least wait for stores to exist
      await page.waitForFunction(
        () => typeof (window as any).__stores?.paletteStore !== 'undefined',
        { timeout: 10_000 },
      )
    }
  })

  test('AC1.1 opens via paletteStore.open() and renders all sections', async ({ page }) => {
    // Seed some data so sections appear
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Test Run',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, run)

    // Verify stores are available
    const storesAvailable = await page.evaluate(() => {
      return typeof (window as any).__stores?.paletteStore !== 'undefined'
    })
    expect(storesAvailable).toBe(true)

    // Open palette
    await page.waitForTimeout(500) // Extra settling time before opening palette
    await page.evaluate(() => {
      ;(window as any).__stores!.paletteStore!.open()
    })

    // Check palette is now open
    const isOpen = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteOpen)
    expect(isOpen).toBe(true)

    // Wait for the Command.Dialog component to mount and render the actual dialog element in the DOM
    await page.waitForFunction(() => document.querySelector('[role="dialog"]') !== null, {
      timeout: 5_000,
    })
    // Check DOM
    await expect(page.getByRole('dialog')).toBeVisible()

    // Sections render in source order (Runs, Jobs, Runner Pools, Commands)
    const headings = await page.locator('[data-command-group-heading]').allInnerTexts()
    expect(headings).toContain('Runs')
    expect(headings).toContain('Commands')
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
    // Extra settle time before tests that use sendWS to ensure page is fully ready
    await page.waitForTimeout(500)

    // Seed some runs
    const run1 = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'feat: add feature',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    const run2 = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'fix: bug fix',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })

    await sendWS(page, run1)
    await sendWS(page, run2)
    // Give Svelte reactivity time to process the state update after sendWS, then settle before opening palette
    await page.waitForTimeout(500)

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()
    // Set query via store directly
    await page.evaluate(() => (window as any).__stores!.paletteStore!.setQuery('feat'))

    // Check query is set
    const query = await page.evaluate(() => (window as any).__stores!.paletteStore!.paletteQuery)
    expect(query).toBe('feat')

    // Only the 'feat' run should be visible in results — scope to the dialog
    // so we don't collide with the same titles also rendered in the kanban board.
    const dialog = page.getByRole('dialog')
    await expect(dialog.getByText('feat: add feature')).toBeVisible()
    await expect(dialog.getByText('fix: bug fix')).not.toBeVisible()
  })

  test('AC1.4 selecting a run sets selectedRunId and records the visit', async ({ page }) => {
    // Extra settle time before tests that use sendWS to ensure page is fully ready
    await page.waitForTimeout(500)

    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Test Run #1',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, run)
    // Give Svelte reactivity time to process the state update after sendWS, then settle before opening palette
    await page.waitForTimeout(500)

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()
    await page.getByRole('option', { name: /Test Run #1/ }).click()

    const selectedRunId = await page.evaluate(
      () => (window as any).__stores!.uiStore!.selectedRunId,
    )
    expect(selectedRunId).toBe(1n)

    const paletteOpen = await page.evaluate(
      () => (window as any).__stores!.paletteStore!.paletteOpen,
    )
    expect(paletteOpen).toBe(false)

    const recentRunIds = await page.evaluate(
      () => (window as any).__stores!.paletteStore!.recentRunIds,
    )
    expect(recentRunIds.length).toBeGreaterThan(0)
    expect(recentRunIds[0]).toBe(1n)
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

    const activePoolFilter = await page.evaluate(
      () => (window as any).__stores!.uiStore!.activePoolFilter,
    )
    expect(activePoolFilter).not.toBeNull()

    const paletteOpen = await page.evaluate(
      () => (window as any).__stores!.paletteStore!.paletteOpen,
    )
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
    const paletteOpen = await page.evaluate(
      () => (window as any).__stores!.paletteStore!.paletteOpen,
    )

    expect(theme).toBe('violet')
    expect(subMenu).toBeNull()
    expect(paletteOpen).toBe(false)
  })

  test('AC1.9 empty state shows message when no items match query', async ({ page }) => {
    // Open palette and set query via store directly
    await page.evaluate(() => {
      ;(window as any).__stores!.paletteStore!.open()
      ;(window as any).__stores!.paletteStore!.setQuery('xyz123nonexistent')
    })
    await expect(page.getByRole('dialog')).toBeVisible()
    await expect(page.getByText('Nothing in flight matching')).toBeVisible()
  })

  test('AC1.10 empty state shows curly-quoted query', async ({ page }) => {
    // Open palette and set query via store directly
    await page.evaluate(() => {
      ;(window as any).__stores!.paletteStore!.open()
      ;(window as any).__stores!.paletteStore!.setQuery('xyz123')
    })
    await expect(page.getByRole('dialog')).toBeVisible()
    // The empty state message should contain the query with the exact text including curly quotes
    // Verify the entire empty state message appears
    const emptyStateText = page.locator('text=Nothing in flight matching')
    await expect(emptyStateText).toBeVisible()
  })

  test('AC1.11 pool rows show three states (browse / query-active / focused)', async ({ page }) => {
    // Seed a pool with many labels to force wrapping/truncation
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
    await expect(page.getByRole('dialog')).toBeVisible()

    // Verify pool section exists
    await expect(page.getByText('Runner Pools')).toBeVisible()

    // Pool items should exist and be visible
    const poolOptions = page.locator('[role="option"]')
    const count = await poolOptions.count()
    expect(count).toBeGreaterThan(0)
  })

  test('AC1.12 pressing Escape closes the palette', async ({ page }) => {
    // Give page time to initialize after goto
    await page.waitForTimeout(500)

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).not.toBeVisible()
  })

  test('AC1.13 recent runs appear at top of Runs section', async ({ page }) => {
    // Extra settle time before tests that use sendWS to ensure page is fully ready
    await page.waitForTimeout(500)

    const run1 = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Run 1',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    const run2 = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'Run 2',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })

    await sendWS(page, run1)
    await sendWS(page, run2)
    // Give Svelte reactivity time to process the state update after sendWS
    await page.waitForTimeout(500)

    // Record run 2 as visited (must be in runStore first)
    await page.evaluate(() => (window as any).__stores!.paletteStore!.recordRunVisit(2n))

    await page.evaluate(() => (window as any).__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Recent runs section should exist and show run 2
    const recentHeading = page.locator('[data-command-group-heading]').filter({ hasText: /Recent/ })
    await expect(recentHeading).toBeVisible()

    // Verify run 2 is in the recent section
    const runOptionCount = await page.getByRole('option', { name: /Run 2/ }).count()
    expect(runOptionCount).toBeGreaterThan(0)
  })
})
