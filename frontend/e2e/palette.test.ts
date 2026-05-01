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

  test('AC1.3 sections render in fixed source order: Recent → Runs → Jobs → Runner Pools → Commands', async ({
    page,
  }) => {
    // Extra settle time to ensure all stores are ready
    await page.waitForTimeout(500)

    // Seed a run via WS (required before recordRunVisit so recentRunIds is populated)
    const run = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'AC1.3 Test Run',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, run)
    await page.waitForTimeout(500)

    // Add a job so the Jobs section appears
    await page.evaluate(() => {
      window.__stores!.runStore!.jobsByRun.set(1n, [
        {
          id: 200n,
          runId: 1n,
          name: 'ac13-job',
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

    // Seed a runner pool so the Runner Pools section appears
    await page.evaluate(() => {
      window.__stores!.runnerStore!.loadPools([
        {
          labels: ['linux'],
          running: 1,
          queued: 0,
          total: 2,
          isElastic: false,
          groupName: 'linux',
        },
      ])
    })

    // Record a recent visit so the Recent section appears
    await page.evaluate(() => window.__stores!.paletteStore!.recordRunVisit(1n))

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Read the section headings in DOM order — they must match the fixed source order
    const headings = await page.locator('[data-command-group-heading]').allInnerTexts()
    expect(headings).toEqual(['Recent', 'Runs', 'Jobs', 'Runner Pools', 'Commands'])
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

  test('selecting a different run from palette updates lastTriggerRunId', async ({ page }) => {
    // Regression: previously, selectRun() only wrote selectedRunId. When the
    // panel was already open from a RunCard click (lastTriggerRunId = A) and
    // the user navigated to run B via the palette, lastTriggerRunId stayed
    // pointing at A. Closing the panel then restored focus to the original
    // card on the kanban instead of the run currently being viewed.
    await page.waitForTimeout(500)

    const a = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Run A',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    const b = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'Run B',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, a)
    await sendWS(page, b)
    await page.waitForTimeout(500)

    // Simulate prior RunCard click for run A — panel open, trigger source = A.
    await page.evaluate(() => {
      window.__stores!.uiStore!.lastTriggerRunId = 1n
      window.__stores!.uiStore!.selectedRunId = 1n
    })

    // Open palette and pick run B. With the panel already open, role=dialog
    // resolves to two elements (panel + palette) — assert on the palette's
    // input selector instead, matching the stacking-test convention.
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()
    await page.getByRole('option', { name: /Run B/ }).click()

    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).toBe(2n)
    // The trigger source must follow the panel: closing the panel should
    // restore focus to run B's card, not run A's.
    expect(await page.evaluate(() => window.__stores!.uiStore!.lastTriggerRunId)).toBe(2n)
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

    // Install scrollIntoView spy before clicking the job item.
    // JobBlock's $effect calls scrollIntoView only when selectedJobId === job.id,
    // so a 'job-100' entry in __scrolledIds proves the dispatch wrote selectedJobId = 100n.
    await page.evaluate(() => {
      window.__scrolledIds = []
      const original = Element.prototype.scrollIntoView
      Element.prototype.scrollIntoView = function (options?: ScrollIntoViewOptions | boolean) {
        window.__scrolledIds!.push((this as Element).id)
        original.call(this, options)
      }
    })

    // Click the job item
    await page.getByRole('option', { name: /test-job/ }).click()

    // Wait for RAF + scroll to fire (the $effect schedules inside requestAnimationFrame).
    await page.waitForTimeout(200)

    // selectedRunId is set and palette closed.
    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).toBe(1n)

    const paletteOpen = await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)
    expect(paletteOpen).toBe(false)

    // The panel should have opened (run 1 exists in the store).
    await expect(page.getByRole('dialog')).toBeVisible()

    // selectedJobId = 100n was dispatched: JobBlock's $effect scrolled job-100 into view,
    // proving the CommandPalette.svelte dispatch wrote uiStore.selectedJobId = job.id.
    const scrolled = await page.evaluate(() => window.__scrolledIds!)
    expect(scrolled).toContain('job-100')
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

  test('AC1.7 clicking Switch theme… sets subMenu=theme and slides to theme options', async ({
    page,
  }) => {
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Click the real "Switch theme…" command item — exercises the actual UI interaction
    await page.getByRole('option', { name: /Switch theme/ }).click()

    // subMenu should now be 'theme' (set via enterThemeSubmenu handler)
    const subMenu = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    expect(subMenu).toBe('theme')

    // Slide transition renders the theme submenu heading and all four theme options
    await expect(page.getByText('Switch theme')).toBeVisible()
    await expect(page.getByRole('option', { name: /Warm/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Radar/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Violet/ })).toBeVisible()
    await expect(page.getByRole('option', { name: /Pink/ })).toBeVisible()
  })

  test('AC1.8 selecting a theme via click sets uiStore.theme, clears subMenu, closes palette', async ({
    page,
  }) => {
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Enter the theme submenu via real click (same path as AC1.7)
    await page.getByRole('option', { name: /Switch theme/ }).click()

    // Verify we're in the submenu before clicking a theme
    const subMenuBefore = await page.evaluate(() => window.__stores!.paletteStore!.subMenu)
    expect(subMenuBefore).toBe('theme')

    // Click the Violet theme option in the submenu
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

  test('recentRunIds prunes ids whose runs were evicted from runStore', async ({ page }) => {
    // Seed two runs and record visits for both
    const a = makeRunEvent(1, {
      runId: 1,
      displayTitle: 'Recent A',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    const b = makeRunEvent(2, {
      runId: 2,
      displayTitle: 'Recent B',
      createdAt: new Date().toISOString(),
      runStartedAt: null,
      updatedAt: new Date().toISOString(),
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    })
    await sendWS(page, a)
    await sendWS(page, b)
    await page.waitForTimeout(200)
    await page.evaluate(() => {
      window.__stores!.paletteStore!.recordRunVisit(1n)
      window.__stores!.paletteStore!.recordRunVisit(2n)
    })

    expect(await page.evaluate(() => window.__stores!.paletteStore!.recentRunIds.length)).toBe(2)

    // Evict run 1 from the run store (simulating TTL eviction)
    await page.evaluate(() => window.__stores!.runStore!.runs.delete(1n))
    await page.waitForTimeout(50) // let the prune effect run

    // recentRunIds should be pruned to only the surviving id
    const remaining = await page.evaluate(() =>
      window.__stores!.paletteStore!.recentRunIds.map((b) => b.toString()),
    )
    expect(remaining).toEqual(['2'])
  })

  test('palette submenu state clears on external dismiss (backdrop click)', async ({ page }) => {
    // Regression: previously, Bits UI dialog mechanics (backdrop click, X button) only
    // mutated paletteOpen via bind:open and never cleared subMenu, so reopening landed
    // on the stale theme submenu. Routing close through onOpenChange → paletteStore.close()
    // ensures both fields reset together.
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Enter the theme submenu
    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBe('theme')

    // Simulate backdrop click — Bits UI portals the overlay with data-dialog-overlay
    await page
      .locator('[data-dialog-overlay]')
      .first()
      .click({ position: { x: 5, y: 5 } })

    // Both fields should clear together
    await expect(page.getByRole('dialog')).not.toBeVisible()
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBeNull()

    // Reopening should land on top-level (Switch theme… command), not the theme list
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()
    await expect(page.getByRole('option', { name: /Switch theme/ })).toBeVisible()
  })

  test('palette submenu state clears when Cmd+D closes the palette', async ({ page }) => {
    // Regression: the global Cmd+D handler in App.svelte previously closed the
    // palette by writing paletteOpen = false directly, bypassing close() and
    // leaving subMenu === 'theme'. Reopening then landed on the stale theme
    // submenu instead of the top-level command list.
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBe('theme')

    await page.keyboard.press('Meta+d')

    await expect(page.getByRole('dialog')).not.toBeVisible()
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBeNull()

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('option', { name: /Switch theme/ })).toBeVisible()
  })

  test('palette submenu state clears when Cmd+\\ closes the palette', async ({ page }) => {
    // Same regression as the Cmd+D case, for the density-toggle chord.
    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.evaluate(() => window.__stores!.paletteStore!.enterSubmenu('theme'))
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBe('theme')

    await page.keyboard.press('Meta+\\')

    await expect(page.getByRole('dialog')).not.toBeVisible()
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)
    expect(await page.evaluate(() => window.__stores!.paletteStore!.subMenu)).toBeNull()
  })

  test('AC1.10 empty state shows curly-quoted query when no items match', async ({ page }) => {
    // Open palette and set query via store directly
    await page.evaluate(() => {
      window.__stores!.paletteStore!.open()
      window.__stores!.paletteStore!.setQuery('xyz123nonexistent')
    })
    await expect(page.getByRole('dialog')).toBeVisible()

    // Verify exact empty state message with curly quotes (U+201C and U+201D, matching the Svelte source)
    await expect(page.getByText('Nothing in flight matching “xyz123nonexistent”.')).toBeVisible()
  })

  test('AC1.11 pool rows show three states (browse / query-active / focused) via CSS', async ({
    page,
  }) => {
    // Seed THREE pools. Bits UI Command auto-selects the first option in the list
    // (sets data-selected on it). Probing pool #2 guarantees it does NOT carry
    // data-selected, so the browse-state nowrap assertion is valid.
    await page.evaluate(() => {
      window.__stores!.runnerStore!.loadPools([
        {
          labels: ['windows', 'x64'],
          running: 0,
          queued: 0,
          total: 2,
          isElastic: false,
          groupName: 'windows',
        },
        {
          labels: ['linux', 'self-hosted', 'x86', 'big-runners'],
          running: 2,
          queued: 1,
          total: 4,
          isElastic: true,
          groupName: 'foo',
        },
        {
          labels: ['macos', 'arm64'],
          running: 1,
          queued: 0,
          total: 3,
          isElastic: false,
          groupName: 'macos',
        },
      ])
    })

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Verify pool section exists
    await expect(page.getByText('Runner Pools')).toBeVisible()

    // Pool #2 (linux/self-hosted/x86/big-runners) is NOT the first option in the list —
    // it will NOT have data-selected auto-applied by Bits UI Command.
    const poolOption = page.locator('[role="option"]').filter({ hasText: /linux.*self-hosted/ })
    await expect(poolOption).toBeVisible()

    // Confirm this row does NOT carry data-selected (belt-and-braces guard)
    await expect(poolOption).not.toHaveAttribute('data-selected', '')

    // Verify browse state: labels have white-space: nowrap (no query, no selection)
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

    // Set activePoolFilter using the poolKey helper exposed on the __stores bridge
    await page.evaluate(() => {
      window.__stores!.uiStore!.activePoolFilter = window.__stores!.poolKey!(['linux'])
    })
    await page.waitForTimeout(200)

    // "Clear pool filter" command should now be visible
    await expect(clearPoolCommand).toBeVisible()
  })

  test('AC1.13 Close detail panel command absent when selectedRunId null', async ({ page }) => {
    // Seed run 1 in the store so that setting selectedRunId = 1n does not
    // trigger the RunDetailPanel's AC2.9 fallback (which clears selectedRunId
    // when the id references a run not in the store).
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'AC1.13 test run',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )
    await page.waitForTimeout(200)

    // Palette open without selectedRunId set
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = null
      window.__stores!.paletteStore!.open()
    })
    // Wait for the palette dialog specifically (the panel dialog is absent since selectedRunId=null)
    await page.waitForFunction(() => window.__stores!.paletteStore!.paletteOpen === true, {
      timeout: 3_000,
    })
    // Ensure the palette dialog is visible (not the panel Sheet which is closed)
    await expect(page.locator('[data-command-dialog]').or(page.getByRole('dialog'))).toBeVisible()

    // "Close detail panel" command should not exist while selectedRunId is null
    const closeDetailCommand = page.getByRole('option', { name: /Close detail panel/ })
    await expect(closeDetailCommand).not.toBeVisible()

    // Set selectedRunId to a real run — this opens the panel Sheet AND the
    // command should become visible when the palette is re-opened.
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForTimeout(200)

    // "Close detail panel" command should now be visible inside the palette
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

  test('a run rendered in both Recent and Runs has distinct Command.Item values', async ({
    page,
  }) => {
    // Regression: previously both `recentRuns` and `allRuns` rendered
    // PaletteRunItem with `value="run-${run.id}"`, so the same run produced
    // duplicate Command.Item values when present in both sections. cmdk uses
    // `value` as a unique selection key, so duplicates broke keyboard
    // navigation and selection state. Fix scopes the value with a section
    // prefix (`recent-run-` vs `run-`).
    await page.waitForTimeout(300)

    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'Dup Run',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )
    await page.waitForTimeout(300)
    await page.evaluate(() => window.__stores!.paletteStore!.recordRunVisit(1n))

    await page.evaluate(() => window.__stores!.paletteStore!.open())
    await expect(page.getByRole('dialog')).toBeVisible()

    // Run 1 should appear twice — once under Recent, once under Runs.
    const dupOptions = page.getByRole('option', { name: /Dup Run/ })
    await expect(dupOptions).toHaveCount(2)

    // Each Command.Item carries its `value` on a `data-value` attribute. The
    // two rows must report distinct values so cmdk treats them as separate
    // selection targets.
    const values = await dupOptions.evaluateAll((els) =>
      els.map((el) => el.getAttribute('data-value')),
    )
    expect(new Set(values).size).toBe(2)
    expect(values).toEqual(expect.arrayContaining(['recent-run-1', 'run-1']))
  })
})
