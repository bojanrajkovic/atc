import type { Page } from '@playwright/test'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import type { WorkflowRun } from '../src/lib/types/generated/WorkflowRun'
import { expect, test } from './lib/fixtures'
import { WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

function makeWorkflowRun(id: number): WorkflowRun {
  return {
    id: id as unknown as bigint,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: `commit ${id}`,
    event: 'push',
    displayTitle: `CI — run ${id}`,
    status: 'InProgress',
    conclusion: null,
    htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${id}`,
    createdAt: '2026-04-17T09:59:00Z',
    runStartedAt: '2026-04-17T09:59:30Z',
    updatedAt: '2026-04-17T10:00:00Z',
  }
}

function snapshotWith(runs: WorkflowRun[]): StateSnapshot {
  return {
    lastSeq: 1 as unknown as bigint,
    runs,
    jobs: [],
    runnerPoolCapacities: [],
    displayTtlSeconds: 0,
  }
}

/** Inject WS mock + route /v1/state with the given snapshot. Must be called before goto. */
async function preparePage(page: Page, snapshot: StateSnapshot): Promise<void> {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  await page.route('**/v1/state', (route) =>
    route.fulfill({ contentType: 'application/json', body: JSON.stringify(snapshot) }),
  )
}

/** Wait for connectionStore.status === 'connected' (hydration moment). */
async function waitForConnected(page: Page): Promise<void> {
  await page.waitForFunction(() => window.__stores?.connectionStore?.status === 'connected', {
    timeout: 15_000,
  })
}

function getHistoryLength(page: Page): Promise<number> {
  return page.evaluate(() => window.history.length)
}

function getRelativeUrl(page: Page): Promise<string> {
  return page.evaluate(
    () => window.location.pathname + window.location.search + window.location.hash,
  )
}

test.describe('URL deep link', () => {
  test('deep link — visiting /?run=<known-id> opens the panel and leaves the URL unchanged', async ({
    page,
  }) => {
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/?run=1')

    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
    await expect(page.getByRole('dialog')).toBeVisible()

    const selectedRunId = await page.evaluate(
      () => window.__stores!.uiStore!.selectedRunId?.toString() ?? null,
    )
    expect(selectedRunId).toBe('1')

    expect(await getRelativeUrl(page)).toBe('/?run=1')
  })

  test('deep link — visiting /?run=<unknown-id> never opens the panel and strips run via replaceState', async ({
    page,
  }) => {
    // Baseline: a normal goto('/') hydrates with no replaceState, so its
    // history.length is the reference for "one goto entry, no extra push".
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    const baselineHistoryLength = await getHistoryLength(page)

    // Now exercise the unknown-id path in a fresh tab so the baseline
    // measurement isn't contaminated by the previous goto's history entry.
    const fresh = await page.context().newPage()
    await preparePage(fresh, snapshotWith([makeWorkflowRun(1)]))
    await fresh.goto('/?run=99999')
    await fresh.waitForFunction(() => window.__stores?.connectionStore?.status === 'connected', {
      timeout: 15_000,
    })
    await fresh.waitForFunction(() => !window.location.search.includes('run='), {
      timeout: 5_000,
    })

    await expect(fresh.getByRole('dialog')).not.toBeVisible()
    expect(
      await fresh.evaluate(
        () => window.location.pathname + window.location.search + window.location.hash,
      ),
    ).toBe('/')

    const selectedRunId = await fresh.evaluate(
      () => window.__stores!.uiStore!.selectedRunId?.toString() ?? null,
    )
    expect(selectedRunId).toBeNull()

    // replaceState used: history.length matches the no-deep-link baseline.
    const lengthAfter = await fresh.evaluate(() => window.history.length)
    expect(lengthAfter).toBe(baselineHistoryLength)

    await fresh.close()
  })

  test('deep link — clicking a RunCard adds ?run=<id> via pushState (history.length +1)', async ({
    page,
  }) => {
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)

    // Card has hydrated; wait for the run to render.
    await page.waitForSelector('article[data-run-id="1"]', { timeout: 5_000 })

    const lengthBefore = await getHistoryLength(page)
    expect(await getRelativeUrl(page)).toBe('/')

    await page.locator('article[data-run-id="1"] .run-card-activate').click()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    expect(await getRelativeUrl(page)).toBe('/?run=1')
    expect(await getHistoryLength(page)).toBe(lengthBefore + 1)
  })

  test('deep link — closing the panel via Esc strips ?run= via pushState (history.length +1)', async ({
    page,
  }) => {
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    await page.waitForSelector('article[data-run-id="1"]', { timeout: 5_000 })

    // Open the panel via click so the open-write enters history.
    await page.locator('article[data-run-id="1"] .run-card-activate').click()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
    expect(await getRelativeUrl(page)).toBe('/?run=1')

    const lengthBeforeClose = await getHistoryLength(page)

    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })

    expect(await getRelativeUrl(page)).toBe('/')
    expect(await getHistoryLength(page)).toBe(lengthBeforeClose + 1)
  })

  test('deep link — open → close → back reopens the panel; forward closes it again', async ({
    page,
  }) => {
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    await page.waitForSelector('article[data-run-id="1"]', { timeout: 5_000 })

    // Open.
    await page.locator('article[data-run-id="1"] .run-card-activate').click()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
    expect(await getRelativeUrl(page)).toBe('/?run=1')

    // Close via Esc.
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })
    expect(await getRelativeUrl(page)).toBe('/')

    // Back — should reopen.
    await page.goBack()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
    expect(await getRelativeUrl(page)).toBe('/?run=1')

    // Forward — should close again.
    await page.goForward()
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })
    expect(await getRelativeUrl(page)).toBe('/')
  })

  test('deep link — mount-time hydration does not pollute history with extra entries', async ({
    page,
  }) => {
    // Baseline: goto('/') with the same snapshot — record history.length after connected.
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    const baselineLength = await getHistoryLength(page)

    // Now measure: goto('/?run=1') from a fresh tab — history.length should match baseline.
    // A new page context starts at history.length=1, same as the baseline goto('/').
    const ctx = page.context()
    const fresh = await ctx.newPage()
    await preparePage(fresh, snapshotWith([makeWorkflowRun(1)]))
    await fresh.goto('/?run=1')
    await waitForConnected(fresh)
    await fresh.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    const measuredLength = await fresh.evaluate(() => window.history.length)
    expect(measuredLength).toBe(baselineLength)
    expect(
      await fresh.evaluate(
        () => window.location.pathname + window.location.search + window.location.hash,
      ),
    ).toBe('/?run=1')

    await fresh.close()
  })

  test('deep link — popstate inbound assignment does not echo a new pushState entry', async ({
    page,
  }) => {
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    await page.waitForSelector('article[data-run-id="1"]', { timeout: 5_000 })

    // Click card → history.length = N + 1 (the open-write test already covers this).
    await page.locator('article[data-run-id="1"] .run-card-activate').click()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
    const lengthAfterOpen = await getHistoryLength(page)

    // Back: popstate fires; inbound handler clears selectedRunId. Outbound
    // effect must short-circuit on the target === current guard — no extra
    // history entry.
    await page.goBack()
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })
    expect(await getRelativeUrl(page)).toBe('/')

    // history.length is unchanged across the back-button transition.
    expect(await getHistoryLength(page)).toBe(lengthAfterOpen)
  })

  test('deep link — hydration does not push a spurious entry when unrelated query params use non-canonical encoding', async ({
    page,
  }) => {
    // Loading with %20 in another param: URLSearchParams serializes as `+`,
    // so a string-equality loop guard would treat target and current as
    // different and push. The semantic run-id guard must no-op here.
    await preparePage(page, snapshotWith([makeWorkflowRun(1)]))
    await page.goto('/')
    await waitForConnected(page)
    const baselineLength = await getHistoryLength(page)

    const fresh = await page.context().newPage()
    await preparePage(fresh, snapshotWith([makeWorkflowRun(1)]))
    await fresh.goto('/?q=my%20term')
    await fresh.waitForFunction(() => window.__stores?.connectionStore?.status === 'connected', {
      timeout: 15_000,
    })

    // URL preserved verbatim (no canonicalization pass).
    expect(
      await fresh.evaluate(
        () => window.location.pathname + window.location.search + window.location.hash,
      ),
    ).toBe('/?q=my%20term')
    // history.length matches the no-deep-link baseline — no spurious entry.
    expect(await fresh.evaluate(() => window.history.length)).toBe(baselineLength)

    await fresh.close()
  })

  test('deep link — Esc-closing a deep-linked panel restores focus to the URL run card', async ({
    page,
  }) => {
    // Deep-linked panel never had a card click, so without seeding
    // lastTriggerRunId during hydration the close handler exits early and
    // focus falls to `<body>`. Hydration must mirror a card click and
    // populate the trigger.
    await preparePage(page, snapshotWith([makeWorkflowRun(1), makeWorkflowRun(2)]))
    await page.goto('/?run=1')
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })

    // Focus lands on card 1 (the URL's run), not card 2 (initial-focus) or body.
    const focusedRunId = await page.evaluate(() => {
      const active = document.activeElement as HTMLElement | null
      return active?.closest('.run-card')?.getAttribute('data-run-id') ?? null
    })
    expect(focusedRunId).toBe('1')
  })

  test('deep link — back-button close restores focus to the originating card', async ({ page }) => {
    // Closing via popstate-to-/ must restore focus to the card the user
    // originally clicked. The handler must preserve lastTriggerRunId so
    // RunDetailPanel.onCloseAutoFocus can find the trigger.
    await preparePage(page, snapshotWith([makeWorkflowRun(1), makeWorkflowRun(2)]))
    await page.goto('/')
    await waitForConnected(page)
    await page.waitForSelector('article[data-run-id="1"]', { timeout: 5_000 })

    await page.locator('article[data-run-id="1"] .run-card-activate').click()
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    await page.goBack()
    await expect(page.getByRole('dialog')).not.toBeVisible({ timeout: 3_000 })

    // Focus landed on the originating card (run 1), not on the kanban's
    // initial-focus card (which would be the first non-empty column's first
    // card — same card here but distinguishable when the kanban is wider).
    const focusedRunId = await page.evaluate(() => {
      const active = document.activeElement as HTMLElement | null
      return active?.closest('.run-card')?.getAttribute('data-run-id') ?? null
    })
    expect(focusedRunId).toBe('1')
  })

  test('deep link — popstate updates lastTriggerRunId to match the displayed run (focus-restoration sync)', async ({
    page,
  }) => {
    // RunDetailPanel.onCloseAutoFocus reads lastTriggerRunId to restore focus
    // to a specific card when the panel closes. If popstate moves the panel
    // from run B to run A but lastTriggerRunId still points at B, closing
    // the panel jumps focus to card B — an accessibility regression.
    await preparePage(page, snapshotWith([makeWorkflowRun(1), makeWorkflowRun(2)]))
    await page.goto('/')
    await waitForConnected(page)

    // Open run 1 then run 2 via direct store writes (clicking a second card
    // while the sheet is open would close it via interactOutsideBehavior).
    await page.evaluate(() => {
      window.__stores!.uiStore!.lastTriggerRunId = 1n
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForFunction(() => window.location.search === '?run=1', { timeout: 3_000 })
    await page.evaluate(() => {
      window.__stores!.uiStore!.lastTriggerRunId = 2n
      window.__stores!.uiStore!.selectedRunId = 2n
    })
    await page.waitForFunction(() => window.location.search === '?run=2', { timeout: 3_000 })

    // Back → /?run=1; selectedRunId AND lastTriggerRunId must both update.
    await page.goBack()
    await page.waitForFunction(() => window.location.search === '?run=1', { timeout: 3_000 })

    expect(
      await page.evaluate(() => window.__stores!.uiStore!.selectedRunId?.toString() ?? null),
    ).toBe('1')
    expect(
      await page.evaluate(() => window.__stores!.uiStore!.lastTriggerRunId?.toString() ?? null),
    ).toBe('1')
  })

  test('deep link — stale popstate strips ?run= and closes the panel, keeping URL and UI in sync', async ({
    page,
  }) => {
    // Scenario: history contains /?run=1 (later evicted), current state is on
    // /?run=2 with selectedRunId=2. Back-navigating to the stale entry must
    // strip the URL to `/` AND clear selectedRunId — otherwise URL says `/`
    // while the panel still shows run 2, and a refresh or shared link would
    // silently lose the selection.
    await preparePage(page, snapshotWith([makeWorkflowRun(1), makeWorkflowRun(2)]))
    await page.goto('/')
    await waitForConnected(page)

    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForFunction(() => window.location.search === '?run=1', { timeout: 3_000 })

    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 2n
    })
    await page.waitForFunction(() => window.location.search === '?run=2', { timeout: 3_000 })

    const lengthBeforeBack = await getHistoryLength(page)

    // Evict run 1 from the store (simulate retention sweep). Run 2 stays.
    await page.evaluate(() => {
      window.__stores!.runStore!.runs.delete(1n)
    })

    // Back: browser navigates to /?run=1; popstate fires; stale path.
    await page.goBack()
    await page.waitForFunction(() => window.location.search === '', { timeout: 3_000 })

    expect(await getRelativeUrl(page)).toBe('/')
    // Panel closes — selectedRunId is cleared so URL and UI stay in sync.
    expect(
      await page.evaluate(() => window.__stores!.uiStore!.selectedRunId?.toString() ?? null),
    ).toBeNull()
    // replaceState path — no extra history entry.
    expect(await getHistoryLength(page)).toBe(lengthBeforeBack)
  })
})
