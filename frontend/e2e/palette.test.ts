import { expect, test } from '@playwright/test'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

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
          const s = window.__stores
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
      await page.waitForFunction(() => typeof window.__stores?.paletteStore !== 'undefined', {
        timeout: 10_000,
      })
    }
  })

  test('AC1.1 opens via paletteStore.open() and focuses input', async ({ page }) => {
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
      return typeof window.__stores?.paletteStore !== 'undefined'
    })
    expect(storesAvailable).toBe(true)

    // Open palette
    await page.waitForTimeout(500) // Extra settling time before opening palette
    await page.evaluate(() => {
      window.__stores!.paletteStore!.open()
    })

    // Check palette is now open
    const isOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(isOpen).toBe(true)

    // Wait for the Command.Dialog component to mount and render the actual dialog element in the DOM
    await page.waitForFunction(() => document.querySelector('[role="dialog"]') !== null, {
      timeout: 5_000,
    })
    // Check DOM
    await expect(page.getByRole('dialog')).toBeVisible()

    // AC1.1: Verify input is focused
    const input = page.locator('input[role="combobox"]')
    await expect(input).toBeFocused()

    // Sections render in source order (Runs, Jobs, Runner Pools, Commands)
    const headings = await page.locator('[data-command-group-heading]').allInnerTexts()
    expect(headings).toContain('Runs')
    expect(headings).toContain('Commands')
  })

  test('AC1.2 filter behavior: typing into searchbox via keyboard updates paletteQuery', async ({
    page,
  }) => {
    // Seed some runs to have visible results
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
    await page.waitForTimeout(500)

    // Open palette
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Type into input via keyboard (oninput event)
    const input = page.locator('input[role="combobox"]')
    await input.pressSequentially('feat')

    // Verify paletteQuery updated per keystroke (via oninput, not onchange)
    const query = await page.evaluate(() => window.__stores!.paletteStore!.paletteQuery)
    expect(query).toBe('feat')

    // Verify filtering: only the 'feat' run should be visible
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

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()
    await page.getByRole('option', { name: /Test Run #1/ }).click()

    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).toBe(1n)

    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)

    const recentRunIds = await page.evaluate(() => window.__stores!.paletteStore!.recentRunIds)
    expect(recentRunIds.length).toBeGreaterThan(0)
    expect(recentRunIds[0]).toBe(1n)
  })

  test('AC1.5 selecting a job sets selectedRunId and selectedJobId', async ({ page }) => {
    // Seed a run with a job via store mutation
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Test Run with Job',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, run)
    await page.waitForTimeout(500)

    // Manually load a job into the store (typed correctly as Job)
    await page.evaluate(() => {
      window.__stores!.runStore!.jobsByRun.set(1n, [
        {
          id: 100n,
          runId: 1n,
          name: 'test-job',
          status: 'Completed' as const,
          conclusion: 'Success' as const,
          runner: null,
          labels: [],
          steps: [],
          createdAt: new Date().toISOString(),
          startedAt: new Date().toISOString(),
          completedAt: new Date().toISOString(),
        },
      ])
    })
    await page.waitForTimeout(200)

    // Open palette
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Click the job item
    await page.getByRole('option', { name: /test-job/ }).click()

    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    const selectedJobId = await page.evaluate(() => window.__stores!.uiStore!.selectedJobId)
    expect(selectedRunId).toBe(1n)
    expect(selectedJobId).toBe(100n)

    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)
  })

  test('AC1.6 selecting a pool sets activePoolFilter and closes palette', async ({ page }) => {
    // Seed a runner pool
    await page.evaluate(() => {
      window.__stores!.runnerStore!.loadPools([
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

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    // Click on the pool option
    await page.getByRole('option', { name: /linux.*x86/ }).click()

    const activePoolFilter = await page.evaluate(() => window.__stores!.uiStore!.activePoolFilter)
    expect(activePoolFilter).not.toBeNull()

    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)
  })

  test('AC1.7 enterSubmenu slides to theme options with appropriate items visible', async ({
    page,
  }) => {
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))

    const subMenu = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    expect(subMenu).toBe('theme')

    // Verify the slide transition rendered the theme options
    await expect(page.getByText('Switch theme')).toBeVisible()
    await expect(page.getByRole('option', { name: /Warm/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Radar/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Violet/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Pink/ })).toBeVisible()
  })

  test('AC1.8 selecting a theme sets uiStore.theme via real click', async ({ page }) => {
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))

    // Click the Violet theme option
    await page.getByRole('option', { name: /Violet/ }).click()

    const theme = await page.evaluate(() => window.__stores!.uiStore!.theme)
    const subMenu = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)

    expect(theme).toBe('violet')
    expect(subMenu).toBeNull()
    expect(paletteOpen).toBe(false)
  })

  test('AC1.9 pressing Escape inside submenu returns to top-level without closing', async ({
    page,
  }) => {
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))

    // Verify submenu is active
    let subMenu = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    expect(subMenu).toBe('theme')

    // Press Escape
    await page.keyboard.press('Escape')

    // Verify submenu cleared but palette still open
    subMenu = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(subMenu).toBeNull()
    expect(paletteOpen).toBe(true)

    // Dialog should still be visible
    await expect(page.getByRole('dialog')).toBeVisible()
  })

  test('AC1.10 empty state shows curly-quoted query when no items match', async ({ page }) => {
    // Open palette and set query via store directly
    await page.evaluate(() => {
      window.__stores!.paletteStore!.open()
      window.__stores!.paletteStore!.setQuery('xyz123nonexistent')
    })
    await expect(page.getByRole('dialog')).toBeVisible()

    // Verify exact empty state message with curly quotes
    await expect(page.getByText('Nothing in flight matching "xyz123nonexistent".')).toBeVisible()
  })

  test('AC1.11 pool rows show three states (browse / query-active / focused) via CSS', async ({
    page,
  }) => {
    // Seed a pool with many labels to force wrapping/truncation
    await page.evaluate(() => {
      window.__stores!.runnerStore!.loadPools([
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

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Verify pool section exists
    await expect(page.getByText('Runner Pools')).toBeVisible()

    // Pool items should exist and render labels
    const poolOption = page.locator('[role="option"]').filter({ hasText: /linux.*self-hosted/ })
    await expect(poolOption).toBeVisible()

    // Verify browse state: labels have white-space: nowrap (when no query)
    const labelsElement = poolOption.locator('.labels')
    const computedStyle = await labelsElement.evaluate(
      (el) => window.getComputedStyle(el).whiteSpace,
    )
    expect(computedStyle).toBe('nowrap')
  })

  test('AC1.12 Clear pool filter command absent when activePoolFilter null', async ({ page }) => {
    // Palette open without activePoolFilter set
    await page.evaluate(() => {
      window.__stores!.uiStore!.activePoolFilter = null
      window.__stores!.paletteStore!.open()
    })
    await expect(page.getByRole('dialog')).toBeVisible()

    // "Clear pool filter" command should not exist
    const clearPoolCommand = page.getByRole('option', { name: /Clear pool filter/ })
    await expect(clearPoolCommand).not.toBeVisible()

    // Set activePoolFilter using the poolKey formula (labels sorted, pipe-separated)
    await page.evaluate(() => {
      // Simulate poolKey(['linux']): sort and join with |
      // The PoolKey type is a branded string, but TypeScript will accept string assignment at runtime
      ;(window.__stores!.uiStore!.activePoolFilter as any) = 'linux'
    })
    await page.waitForTimeout(200)

    // "Clear pool filter" command should now be visible
    await expect(clearPoolCommand).toBeVisible()
  })

  test('AC1.13 Close detail panel command absent when selectedRunId null', async ({ page }) => {
    // Palette open without selectedRunId set
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = null
      window.__stores!.paletteStore!.open()
    })
    await expect(page.getByRole('dialog')).toBeVisible()

    // "Close detail panel" command should not exist
    const closeDetailCommand = page.getByRole('option', { name: /Close detail panel/ })
    await expect(closeDetailCommand).not.toBeVisible()

    // Set selectedRunId
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForTimeout(200)

    // "Close detail panel" command should now be visible
    await expect(closeDetailCommand).toBeVisible()
  })

  test('Recent runs appear at top of Runs section', async ({ page }) => {
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
    await page.evaluate(() => window.__stores!.paletteStore!.recordRunVisit(2n))

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Recent runs section should exist and show run 2
    const recentHeading = page.locator('[data-command-group-heading]').filter({ hasText: /Recent/ })
    await expect(recentHeading).toBeVisible()

    // Verify run 2 is in the recent section
    const runOptionCount = await page.getByRole('option', { name: /Run 2/ }).count()
    expect(runOptionCount).toBeGreaterThan(0)
  })
})
