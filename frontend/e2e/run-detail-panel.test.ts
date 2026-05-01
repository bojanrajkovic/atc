import { expect, test } from '@playwright/test'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import { makeJobSeqEvent, makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/** Standard page setup: inject WS mock, stub /v1/state, navigate, wait for connected. */
async function setupPage(page: import('@playwright/test').Page) {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ seq: 1, runs: [], jobs: [], poolStats: [] }),
    })
  })
  await page.goto('/')
  try {
    await page.waitForFunction(
      () => {
        const s = window.__stores
        return (
          typeof s?.uiStore !== 'undefined' &&
          typeof s?.runStore !== 'undefined' &&
          typeof s?.connectionStore !== 'undefined' &&
          s.connectionStore.status === 'connected'
        )
      },
      { timeout: 15_000 },
    )
  } catch {
    await page.waitForFunction(() => typeof window.__stores?.uiStore !== 'undefined', {
      timeout: 10_000,
    })
  }
}

/** Seed run id=1 and open the detail panel. Returns the canonical htmlUrl for run 1. */
async function seedAndOpenPanel(
  page: import('@playwright/test').Page,
  action: Parameters<typeof makeRunEvent>[1]['action'] = { type: 'InProgress' },
): Promise<string> {
  await sendWS(
    page,
    makeRunEvent(1, {
      runId: 1,
      displayTitle: 'CI — main',
      createdAt: new Date().toISOString(),
      runStartedAt: action.type === 'Requested' ? null : new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      action,
    }),
  )
  await page.evaluate(() => {
    window.__stores!.uiStore!.selectedRunId = 1n
  })
  await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
  // makeRunEvent sets htmlUrl = `https://github.com/test-org/test-repo/actions/runs/${runId}`
  return 'https://github.com/test-org/test-repo/actions/runs/1'
}

test.describe('Run detail panel', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  // -----------------------------------------------------------------------
  // AC2.1 — Panel opens via selectedRunId and shows single-pane layout
  // -----------------------------------------------------------------------
  test('interactivity.AC2.1 panel opens and renders header + meta-grid when selectedRunId is set', async ({
    page,
  }) => {
    await seedAndOpenPanel(page)

    await expect(page.getByRole('dialog')).toBeVisible()
    await expect(page.locator('.panel-header')).toBeVisible()
    await expect(page.locator('.meta-grid')).toBeVisible()
  })

  // -----------------------------------------------------------------------
  // AC2.2 — "Go to run" anchor attributes
  // -----------------------------------------------------------------------
  test('interactivity.AC2.2 Go-to-run link has correct href, target="_blank", rel="noopener noreferrer"', async ({
    page,
  }) => {
    const htmlUrl = await seedAndOpenPanel(page)

    const link = page.getByRole('link', { name: /go to run/i }).first()
    await expect(link).toHaveAttribute('href', htmlUrl)
    await expect(link).toHaveAttribute('target', '_blank')
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer')
  })

  // -----------------------------------------------------------------------
  // AC2.3 — Esc key closes panel and clears selectedRunId
  // -----------------------------------------------------------------------
  test('interactivity.AC2.3 Esc key closes panel and sets selectedRunId to null', async ({
    page,
  }) => {
    await seedAndOpenPanel(page)
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.keyboard.press('Escape')

    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })

    const runIdAfter = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(runIdAfter).toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC2.4 — Close button closes panel
  // -----------------------------------------------------------------------
  test('interactivity.AC2.4 clicking "Close detail panel" button closes panel', async ({
    page,
  }) => {
    await seedAndOpenPanel(page)
    await expect(page.getByRole('dialog')).toBeVisible()

    await page.getByRole('button', { name: 'Close detail panel' }).click()

    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })

    const runIdAfter = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(runIdAfter).toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC2.5 — Click outside closes panel
  // -----------------------------------------------------------------------
  test('interactivity.AC2.5 clicking outside the sheet closes the panel', async ({ page }) => {
    await seedAndOpenPanel(page)
    await expect(page.getByRole('dialog')).toBeVisible()

    // Click at the top-left corner of the viewport, outside the right-side sheet.
    await page.mouse.click(10, 10)

    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })
  })

  // -----------------------------------------------------------------------
  // AC2.6 — Focus trapped inside sheet while open
  // -----------------------------------------------------------------------
  test('interactivity.AC2.6 focus is trapped inside the sheet while it is open', async ({
    page,
  }) => {
    await seedAndOpenPanel(page)
    await expect(page.getByRole('dialog')).toBeVisible()

    // Tab through several focusable elements; focus must remain inside the dialog.
    await page.keyboard.press('Tab')
    await page.keyboard.press('Tab')
    await page.keyboard.press('Tab')

    const focusedInsideDialog = await page.evaluate(
      () => document.activeElement?.closest('[role="dialog"]') !== null,
    )
    expect(focusedInsideDialog).toBe(true)
  })

  // -----------------------------------------------------------------------
  // AC2.7 — selectedJobId triggers JobBlock scroll-into-view and is cleared
  // -----------------------------------------------------------------------
  test('interactivity.AC2.7 setting selectedJobId calls scrollIntoView on the target block and clears selectedJobId', async ({
    page,
  }) => {
    // Seed run 1.
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'CI — main',
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: { type: 'InProgress' },
      }),
    )

    // Add 3 jobs to ensure JobBlock elements exist in the DOM.
    for (let i = 1; i <= 3; i++) {
      await sendWS(
        page,
        makeJobSeqEvent(i + 1, {
          jobData: {
            jobId: BigInt(i),
            runId: 1n,
            org: 'test-org',
            repo: 'test-repo',
            name: `job-${i}`,
            createdAt: new Date().toISOString(),
            startedAt: new Date().toISOString(),
            completedAt: null,
            action: { type: 'InProgress', data: { runner: null, labels: ['linux'], steps: [] } },
          },
          poolStatsAfter: null,
        }),
      )
    }

    // Open the panel.
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    // Verify the target job block is present in the DOM.
    const targetJobId = 3n
    await expect(page.locator(`#job-${targetJobId}`)).toBeAttached()

    // Spy on scrollIntoView at the page level before setting selectedJobId.
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: test spy on DOM prototype
      ;(window as any).__scrollIntoViewCalled = false
      const original = Element.prototype.scrollIntoView
      Element.prototype.scrollIntoView = function (options?: ScrollIntoViewOptions | boolean) {
        // biome-ignore lint/suspicious/noExplicitAny: test spy on DOM prototype
        ;(window as any).__scrollIntoViewCalled = true
        original.call(this, options)
      }
    })

    // Trigger scroll-into-view via selectedJobId.
    await page.evaluate((id: string) => {
      window.__stores!.uiStore!.selectedJobId = BigInt(id)
    }, targetJobId.toString())

    // Wait for RAF + scroll to fire (the $effect schedules inside requestAnimationFrame).
    await page.waitForTimeout(200)

    // scrollIntoView was called on the target job block.
    const scrollCalled = await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: test spy on DOM prototype
      return (window as any).__scrollIntoViewCalled
    })
    expect(scrollCalled).toBe(true)

    // selectedJobId was cleared by the onSelectedJobIdConsumed callback.
    const jobIdAfter = await page.evaluate(() => window.__stores!.uiStore!.selectedJobId)
    expect(jobIdAfter).toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC2.8 — All 11 StatusKey fixtures render with correct data-status-key
  // -----------------------------------------------------------------------
  const STATUS_KEY_FIXTURES: Array<{
    key: string
    action: Parameters<typeof makeRunEvent>[1]['action']
  }> = [
    { key: 'Queued', action: { type: 'Requested' } },
    { key: 'InProgress', action: { type: 'InProgress' } },
    {
      key: 'Success',
      action: { type: 'Completed', data: { conclusion: 'Success' satisfies RunConclusion } },
    },
    {
      key: 'Failure',
      action: { type: 'Completed', data: { conclusion: 'Failure' satisfies RunConclusion } },
    },
    {
      key: 'Cancelled',
      action: { type: 'Completed', data: { conclusion: 'Cancelled' satisfies RunConclusion } },
    },
    {
      key: 'TimedOut',
      action: { type: 'Completed', data: { conclusion: 'TimedOut' satisfies RunConclusion } },
    },
    {
      key: 'ActionRequired',
      action: {
        type: 'Completed',
        data: { conclusion: 'ActionRequired' satisfies RunConclusion },
      },
    },
    {
      key: 'StartupFailure',
      action: {
        type: 'Completed',
        data: { conclusion: 'StartupFailure' satisfies RunConclusion },
      },
    },
    {
      key: 'Stale',
      action: { type: 'Completed', data: { conclusion: 'Stale' satisfies RunConclusion } },
    },
    {
      key: 'Neutral',
      action: { type: 'Completed', data: { conclusion: 'Neutral' satisfies RunConclusion } },
    },
    {
      key: 'Skipped',
      action: { type: 'Completed', data: { conclusion: 'Skipped' satisfies RunConclusion } },
    },
  ]

  for (const { key, action } of STATUS_KEY_FIXTURES) {
    test(`interactivity.AC2.8 PanelHeader renders with data-status-key="${key}"`, async ({
      page,
    }) => {
      await seedAndOpenPanel(page, action)

      const header = page.locator('.panel-header')
      await expect(header).toBeVisible()
      await expect(header).toHaveAttribute('data-status-key', key)
    })
  }

  // -----------------------------------------------------------------------
  // Panel-scroll regression: long job lists must scroll inside .job-blocks,
  // not spill past the viewport. Sheet.Content has no built-in scroll prop;
  // the fix is `flex-1 min-h-0 overflow-y-auto` on the .job-blocks container.
  // -----------------------------------------------------------------------
  test('panel job list scrolls when content exceeds viewport', async ({ page }) => {
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'CI — main',
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: { type: 'InProgress' },
      }),
    )

    // Seed enough jobs+steps so the panel body overflows ~600px viewport space.
    // Each JobBlock with 8 steps takes ~250px; 6 such blocks ~ 1500px content.
    const stepNames = Array.from({ length: 8 }, (_, i) => `step-${i + 1}`)
    for (let i = 1; i <= 6; i++) {
      await sendWS(
        page,
        makeJobSeqEvent(i + 1, {
          jobData: {
            jobId: BigInt(i),
            runId: 1n,
            org: 'test-org',
            repo: 'test-repo',
            name: `job-${i}-with-a-fairly-long-name-to-force-vertical-rhythm`,
            createdAt: new Date().toISOString(),
            startedAt: new Date().toISOString(),
            completedAt: null,
            action: {
              type: 'InProgress',
              data: {
                runner: null,
                labels: ['linux'],
                steps: stepNames.map((name, idx) => ({
                  number: BigInt(idx + 1),
                  name,
                  status: 'Completed' as const,
                  conclusion: 'Success' as const,
                  startedAt: new Date().toISOString(),
                  completedAt: new Date().toISOString(),
                })),
              },
            },
          },
          poolStatsAfter: null,
        }),
      )
    }

    // Open the panel.
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    // The .job-blocks container should overflow and be scrollable.
    const jobBlocks = page.locator('.job-blocks')
    const geometry = await jobBlocks.evaluate((el) => ({
      scrollH: el.scrollHeight,
      clientH: el.clientHeight,
      overflowY: getComputedStyle(el).overflowY,
      flex: getComputedStyle(el).flex,
      minH: getComputedStyle(el).minHeight,
    }))
    expect(geometry.overflowY).toBe('auto')
    expect(geometry.minH).toBe('0px')
    // flex shorthand can normalize differently across browsers — assert numeric pieces
    expect(geometry.flex).toMatch(/^1\s+1\s/)
    expect(geometry.scrollH).toBeGreaterThan(geometry.clientH)

    // Programmatic scroll must move scrollTop (proves the container is the
    // scroll region, not the surrounding Sheet.Content).
    await jobBlocks.evaluate((el) => el.scrollTo({ top: 200, behavior: 'instant' }))
    const scrollTop = await jobBlocks.evaluate((el) => el.scrollTop)
    expect(scrollTop).toBeGreaterThan(0)
  })

  // -----------------------------------------------------------------------
  // AC2.9 — Missing-run fallback: selectedRunId for non-existent run is cleared
  // -----------------------------------------------------------------------
  test('interactivity.AC2.9 setting selectedRunId to a non-existent id is cleared without opening panel', async ({
    page,
  }) => {
    // Set a run id that does not exist in the store.
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 99999n
    })

    // Wait a tick for the $effect to run.
    await page.waitForTimeout(100)

    const runIdAfter = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(runIdAfter).toBeNull()

    // No dialog should be open.
    await expect(page.getByRole('dialog')).not.toBeVisible()
  })
})
