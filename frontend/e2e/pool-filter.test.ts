import { expect, test } from './lib/fixtures'
import { makeJobCommittedEvent, makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Pool filter integration E2E.
 *
 * Verifies the wiring from `uiStore.activePoolFilter` through KanbanBoard,
 * KanbanColumn, RunnerBar/RunnerPool, and PoolFilterPill. Where the palette UI
 * is involved we drive it; for the rest we set the filter key directly via the
 * dev bridge to keep these tests focused on the integration.
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
      body: JSON.stringify({ lastSeq: 1, runs: [], jobs: [] }),
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
    makeJobCommittedEvent(3, {
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
          data: {
            runner: { id: 1n, name: 'runner-1', groupId: null, groupName: 'linux-builders' },
            labels: LINUX_LABELS,
            steps: [],
          },
        },
      },
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
    makeJobCommittedEvent(5, {
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

  test('setting active pool filter hides non-matching runs across all columns', async ({
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

  test('matching TopBar RunnerPool gets is-active-filter class; others do not', async ({
    page,
  }) => {
    const linuxKey = brandKey(LINUX_LABELS)
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, linuxKey)

    const matching = page.locator('[data-testid="runner-pool-linux-builders"]')
    const other = page.locator('[data-testid="runner-pool-self-hosted, windows"]')
    await expect(matching).toHaveClass(/is-active-filter/)
    await expect(other).not.toHaveClass(/is-active-filter/)
  })

  test('PoolFilterPill renders with sorted labels and clear button clears the filter', async ({
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

  test('"Clear pool filter" command in palette clears the filter', async ({ page }) => {
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

  test('with no filter, no pill renders, no TopBar pool highlights, all runs visible', async ({
    page,
  }) => {
    // beforeEach left activePoolFilter null (no setter called).
    await expect(page.locator('.pool-filter-pill')).toBeHidden()
    await expect(page.locator('.runner-pool.is-active-filter')).toHaveCount(0)
    await expect(page.locator('.run-card[data-run-id="1"]')).toBeVisible()
    await expect(page.locator('.run-card[data-run-id="3"]')).toBeVisible()
  })

  test('filter that matches no jobs renders empty columns; pill still shows', async ({ page }) => {
    const noMatchKey = brandKey(['nonexistent-label'])
    await page.evaluate((key) => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = key as any
    }, noMatchKey)

    await expect(page.locator('.run-card')).toHaveCount(0)
    const pill = page.locator('.pool-filter-pill')
    await expect(pill).toBeVisible()
    // Lock the fallback rendering contract: when the filter key has no matching
    // pool in runnerStore.pools, KanbanBoard's `activeFilterLabelText` falls
    // back to splitting the brand on '|' and joining with ' · '. For a single
    // label that produces just the label text itself.
    await expect(pill).toContainText('nonexistent-label')
  })
})
