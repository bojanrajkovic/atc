import { expect, test } from '@playwright/test'
import { makeJobSeqEvent, makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Pool filter integration E2E (Phase 5, Task 5).
 *
 * Verifies the wiring from `uiStore.activePoolFilter` through KanbanBoard,
 * KanbanColumn, RunnerBar/RunnerPool, and PoolFilterPill. Where the palette
 * UI is involved (AC5.1, AC5.4) we drive it; for the rest we set the filter
 * key directly via the dev bridge to keep these tests focused on the
 * integration (palette behavior is covered by Phase 2's E2E suite).
 *
 * The PoolKey brand is `labels.sort().join('|')` — we cast through `as any`
 * in test setup blocks only (per project guidance: localized to test code).
 */

const LINUX_LABELS = ['linux', 'self-hosted', 'x86']
const WINDOWS_LABELS = ['self-hosted', 'windows']

/** Compute the PoolKey brand string for a label set. Mirrors `poolKey()` in `$lib/filters/pool`. */
function brandKey(labels: readonly string[]): string {
  return [...labels].sort().join('|')
}

async function setupPage(page: import('@playwright/test').Page) {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ seq: 1, runs: [], jobs: [], poolStats: [] }),
    })
  })
  await page.goto('/')
  await page.waitForFunction(
    () => {
      const s = window.__stores
      return (
        typeof s?.uiStore !== 'undefined' &&
        typeof s?.runnerStore !== 'undefined' &&
        s.connectionStore?.status === 'connected'
      )
    },
    { timeout: 15_000 },
  )
}

async function seedRunsAndPools(page: import('@playwright/test').Page) {
  // Run 1 (InProgress) — its job uses linux/x86/self-hosted labels.
  await sendWS(
    page,
    makeRunEvent(2, {
      runId: 1,
      displayTitle: 'CI — linux',
      createdAt: '2026-04-29T12:00:00Z',
      runStartedAt: '2026-04-29T12:00:30Z',
      updatedAt: '2026-04-29T12:00:30Z',
      action: { type: 'InProgress' },
    }),
  )
  await sendWS(
    page,
    makeJobSeqEvent(3, {
      jobData: {
        runId: 1n,
        jobId: 100n,
        org: 'test-org',
        repo: 'test-repo',
        name: 'build-linux',
        createdAt: '2026-04-29T12:00:01Z',
        startedAt: '2026-04-29T12:00:30Z',
        completedAt: null,
        action: {
          type: 'InProgress',
          data: { runner: null, labels: LINUX_LABELS, steps: [] },
        },
      },
      poolStatsAfter: null,
    }),
  )

  // Run 3 (Queued) — its job uses windows/self-hosted labels.
  await sendWS(
    page,
    makeRunEvent(4, {
      runId: 3,
      displayTitle: 'CI — windows',
      createdAt: '2026-04-29T12:01:00Z',
      runStartedAt: null,
      updatedAt: '2026-04-29T12:01:00Z',
      action: { type: 'Requested' },
    }),
  )
  await sendWS(
    page,
    makeJobSeqEvent(5, {
      jobData: {
        runId: 3n,
        jobId: 200n,
        org: 'test-org',
        repo: 'test-repo',
        name: 'build-windows',
        createdAt: '2026-04-29T12:01:01Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: WINDOWS_LABELS, steps: [] } },
      },
      poolStatsAfter: [
        {
          labels: LINUX_LABELS,
          queued: 0,
          running: 1,
          groupName: 'linux-builders',
          isElastic: false,
          total: 4,
        },
        {
          labels: WINDOWS_LABELS,
          queued: 1,
          running: 0,
          groupName: 'windows-builders',
          isElastic: false,
          total: 2,
        },
      ],
    }),
  )
}

test.describe('Pool filter integration', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
    await seedRunsAndPools(page)
    // Both runs visible by default (in their respective columns)
    await expect(page.locator('.run-card[data-run-id="1"]')).toBeVisible()
    await expect(page.locator('.run-card[data-run-id="3"]')).toBeVisible()
  })

  test('AC5.1 setting active pool filter hides non-matching runs across all columns', async ({
    page,
  }) => {
    const linuxKey = brandKey(LINUX_LABELS)
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, linuxKey)

    await expect(page.locator('.run-card[data-run-id="1"]')).toBeVisible()
    await expect(page.locator('.run-card[data-run-id="3"]')).toBeHidden()
  })

  test('AC5.2 matching TopBar RunnerPool gets is-active-filter class; others do not', async ({
    page,
  }) => {
    const linuxKey = brandKey(LINUX_LABELS)
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, linuxKey)

    const matching = page.locator('[data-testid="runner-pool-linux-builders"]')
    const other = page.locator('[data-testid="runner-pool-windows-builders"]')
    await expect(matching).toHaveClass(/is-active-filter/)
    await expect(other).not.toHaveClass(/is-active-filter/)
  })

  test('AC5.3 PoolFilterPill renders with sorted labels and clear button clears the filter', async ({
    page,
  }) => {
    const linuxKey = brandKey(LINUX_LABELS)
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, linuxKey)

    const pill = page.locator('.pool-filter-pill')
    await expect(pill).toBeVisible()
    await expect(pill).toContainText('Filtering by')
    // Sorted-then-dot-separated form: 'linux · self-hosted · x86'
    await expect(pill).toContainText('linux · self-hosted · x86')

    await pill.getByRole('button', { name: 'Clear pool filter' }).click()
    expect(await page.evaluate(() => window.__stores!.uiStore!.activePoolFilter)).toBeNull()
    await expect(pill).toBeHidden()
  })

  test('AC5.4 "Clear pool filter" command in palette clears the filter', async ({ page }) => {
    const linuxKey = brandKey(LINUX_LABELS)
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, linuxKey)

    await expect(page.locator('.pool-filter-pill')).toBeVisible()

    await page.evaluate(() => {
      window.__stores!.paletteStore!.open()
    })
    await page.getByText('Clear pool filter', { exact: true }).click()

    expect(await page.evaluate(() => window.__stores!.uiStore!.activePoolFilter)).toBeNull()
    await expect(page.locator('.pool-filter-pill')).toBeHidden()
  })

  test('AC5.5 with no filter, no pill renders, no TopBar pool highlights, all runs visible', async ({
    page,
  }) => {
    // beforeEach left activePoolFilter null (no setter called).
    await expect(page.locator('.pool-filter-pill')).toBeHidden()
    await expect(page.locator('.runner-pool.is-active-filter')).toHaveCount(0)
    await expect(page.locator('.run-card[data-run-id="1"]')).toBeVisible()
    await expect(page.locator('.run-card[data-run-id="3"]')).toBeVisible()
  })

  test('AC5.6 filter that matches no jobs renders empty columns; pill still shows', async ({
    page,
  }) => {
    const noMatchKey = brandKey(['nonexistent-label'])
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, noMatchKey)

    await expect(page.locator('.run-card')).toHaveCount(0)
    await expect(page.locator('.pool-filter-pill')).toBeVisible()
  })
})
