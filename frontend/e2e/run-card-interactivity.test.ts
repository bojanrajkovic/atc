import { expect, test } from './lib/fixtures'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Standard page setup: inject WS mock, stub /v1/state, navigate, wait for
 * connected. Mirrors the pattern from run-detail-panel.test.ts.
 */
async function setupPage(page: import('@playwright/test').Page) {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ lastSeq: 1, runs: [], jobs: [] }),
    })
  })
  await page.setViewportSize({ width: 1280, height: 720 })
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
    // Fallback: at minimum wait for uiStore to be available
    await page.waitForFunction(() => typeof window.__stores?.uiStore !== 'undefined', {
      timeout: 10_000,
    })
  }
}

/**
 * Seed one run into each of the three kanban columns:
 *   run 1 → Queued (left column)
 *   run 2 → InProgress (middle column)
 *   run 3 → Completed/Success (right column)
 *
 * Distinct displayTitle values per run so AC4.5 aria-label assertions are
 * unambiguous.
 */
async function seedThreeRuns(page: import('@playwright/test').Page) {
  await sendWS(
    page,
    makeRunEvent(1, {
      runId: 1,
      displayTitle: 'CI — queued',
      createdAt: '2026-04-29T10:00:00Z',
      runStartedAt: null,
      updatedAt: '2026-04-29T10:00:00Z',
      action: { type: 'Requested' },
    }),
  )
  await sendWS(
    page,
    makeRunEvent(2, {
      runId: 2,
      displayTitle: 'CI — running',
      createdAt: '2026-04-29T10:00:00Z',
      runStartedAt: '2026-04-29T10:00:05Z',
      updatedAt: '2026-04-29T10:00:05Z',
      action: { type: 'InProgress' },
    }),
  )
  await sendWS(
    page,
    makeRunEvent(3, {
      runId: 3,
      displayTitle: 'CI — done',
      createdAt: '2026-04-29T10:00:00Z',
      runStartedAt: '2026-04-29T10:00:05Z',
      updatedAt: '2026-04-29T10:00:10Z',
      action: { type: 'Completed', data: { conclusion: 'Success' } },
    }),
  )
  // Wait for all three columns to have a card so subsequent locators are stable
  await expect(page.locator('.run-card')).toHaveCount(3, { timeout: 5_000 })
}

test.describe('RunCard interactivity', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
    await seedThreeRuns(page)
  })

  // -----------------------------------------------------------------------
  // AC4.2 — Click opens RunDetailPanel
  // -----------------------------------------------------------------------
  test('interactivity.AC4.2 click on activator button opens RunDetailPanel', async ({ page }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).not.toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC4.3 — Enter on focused button opens RunDetailPanel
  // -----------------------------------------------------------------------
  test('interactivity.AC4.3 Enter on focused activator button opens RunDetailPanel', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').focus()
    await page.keyboard.press('Enter')
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
  })

  // -----------------------------------------------------------------------
  // AC4.4 — Space on focused button opens RunDetailPanel
  // -----------------------------------------------------------------------
  test('interactivity.AC4.4 Space on focused activator button opens RunDetailPanel', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').focus()
    await page.keyboard.press('Space')
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
  })

  // -----------------------------------------------------------------------
  // AC4.5 — Roving tabindex: exactly one card has tabindex=0; Tab from
  //   outside the kanban lands on that card; a second Tab exits the kanban.
  //
  // Updated for kanban-keyboard-nav.AC1.2 + AC1.6 — roving tabindex means
  // exactly one card has tabindex=0; Tab moves OUT of the kanban after the
  // focused card (single tab stop). The old assertion that Tab cycles through
  // all three card buttons is superseded by the new roving-tabindex contract.
  // -----------------------------------------------------------------------
  test('interactivity.AC4.5 Tab from outside kanban lands on the single tabindex=0 card, second Tab exits', async ({
    page,
  }) => {
    // Start focus at the Settings button (TopBar, outside the kanban) so Tab
    // travels through the document in natural DOM order into the kanban.
    await page.getByRole('button', { name: 'Settings' }).focus()

    // Tab forward until we land on a .run-card-activate element.
    // Cap at 20 presses to avoid hanging on a regression where no card is reachable.
    let landedOnCard = false
    for (let i = 0; i < 20; i++) {
      await page.keyboard.press('Tab')
      landedOnCard = await page.evaluate(
        () => document.activeElement?.classList.contains('run-card-activate') ?? false,
      )
      if (landedOnCard) break
    }
    expect(landedOnCard, 'Tab from TopBar should reach a run-card-activate button').toBe(true)

    // AC1.1 priority: Queued > InProgress > Completed — the focused card is the
    // first card of the Queued column.
    const landedLabel = await page.evaluate(
      () => document.activeElement?.getAttribute('aria-label') ?? '',
    )
    expect(landedLabel).toBe('CI — queued, Queued, test-repo·main')

    // AC1.6: exactly one card has tabindex=0 at this point.
    const tabzeroCount = await page.locator('.run-card-activate[tabindex="0"]').count()
    expect(tabzeroCount).toBe(1)

    // AC1.2: a second Tab moves focus OUT of the kanban (no second .run-card-activate).
    await page.keyboard.press('Tab')
    const stillOnCard = await page.evaluate(
      () => document.activeElement?.classList.contains('run-card-activate') ?? false,
    )
    expect(
      stillOnCard,
      'Second Tab should exit the kanban (no longer on a run-card-activate)',
    ).toBe(false)
  })

  // -----------------------------------------------------------------------
  // AC4.6 — Clicks on title text (child element) bubble through the
  //          transparent overlay button to activate the card.
  //          The overlay button (.run-card-activate) covers the card with
  //          z-index:1; clicking any child element must hit the button since
  //          it is the topmost element at those coordinates.
  // -----------------------------------------------------------------------
  test('interactivity.AC4.6 click on title text bubbles through transparent overlay to inner button', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    // .run-card-name is the span inside JobHeader that shows the displayTitle.
    // It sits underneath the absolutely-positioned .run-card-activate overlay
    // (z-index: 1). Playwright's actionability check correctly identifies the
    // button as intercepting pointer events and blocks locator.click() on the
    // child span. Instead, use page.mouse.click() at the title's center
    // coordinates — this is a raw browser-level pointer event that lands on
    // the topmost element at those coordinates (the overlay button), exactly
    // as a real user click would. This exercises the z-stack contract: a click
    // anywhere on the card visual surface activates the inner button.
    const titleEl = card.locator('.run-card-name')
    const box = await titleEl.boundingBox()
    expect(box).not.toBeNull()
    await page.mouse.click(box!.x + box!.width / 2, box!.y + box!.height / 2)
    // Behavioral outcome: RunDetailPanel opened (dialog visible) AND selectedRunId set
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).not.toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC3.1 — Hover for 250 ms shows popover anchored to the right of the card
  // -----------------------------------------------------------------------
  test('interactivity.AC3.1 hover 250ms shows popover anchored to the right of the card', async ({
    page,
  }) => {
    // Use the first card (Queued column, left side) — plenty of viewport to
    // the right, so the popover should anchor right without flipping.
    const card = page.locator('.run-card').first()
    await card.hover()
    // Wait for the 250 ms debounce to fire and the popover to appear
    await page.waitForTimeout(300)
    const popover = page.locator('.hover-peek-popover')
    await expect(popover).toBeVisible({ timeout: 2_000 })

    const cardBox = await card.boundingBox()
    const popoverBox = await popover.boundingBox()
    expect(cardBox).not.toBeNull()
    expect(popoverBox).not.toBeNull()
    // Popover's left edge is at or beyond the card's right edge (with 10px tolerance)
    expect(popoverBox!.x).toBeGreaterThanOrEqual(cardBox!.x + cardBox!.width - 10)
  })

  // -----------------------------------------------------------------------
  // AC3.1 (auto-flip) — Rightmost-column card triggers Floating UI auto-flip
  //   so the popover appears to the LEFT of the card.
  // -----------------------------------------------------------------------
  test('interactivity.AC3.1 popover auto-flips left for card in rightmost column', async ({
    page,
  }) => {
    // The Completed column is rightmost; its card has displayTitle "CI — done"
    const completedSection = page.locator('section').filter({
      has: page.locator('[id="kanban-col-completed"]'),
    })
    const rightmostCard = completedSection.locator('.run-card').first()
    await rightmostCard.hover()
    await page.waitForTimeout(300)
    const popover = page.locator('.hover-peek-popover')
    await expect(popover).toBeVisible({ timeout: 2_000 })

    const cardBox = await rightmostCard.boundingBox()
    const popoverBox = await popover.boundingBox()
    expect(cardBox).not.toBeNull()
    expect(popoverBox).not.toBeNull()
    // Auto-flip: popover's right edge is at or before the card's left edge (10px tolerance)
    expect(popoverBox!.x + popoverBox!.width).toBeLessThanOrEqual(cardBox!.x + 10)
  })

  // -----------------------------------------------------------------------
  // AC3.2 — Mouse-leave immediately clears the popover
  // -----------------------------------------------------------------------
  test('interactivity.AC3.2 mouse-leave dismisses popover immediately', async ({ page }) => {
    const card = page.locator('.run-card').first()
    await card.hover()
    await page.waitForTimeout(300)
    await expect(page.locator('.hover-peek-popover')).toBeVisible({ timeout: 2_000 })

    // Move cursor far from the card (top-left corner of viewport)
    await page.mouse.move(0, 0)

    await expect(page.locator('.hover-peek-popover')).toBeHidden({ timeout: 2_000 })
  })

  // -----------------------------------------------------------------------
  // AC3.3 — Click on hovered card opens panel and dismisses popover
  // -----------------------------------------------------------------------
  test('interactivity.AC3.3 click on hovered card opens panel and dismisses popover', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    await card.hover()
    await page.waitForTimeout(300)
    // Wait for popover to be fully visible before clicking (avoids transition flake)
    await page.locator('.hover-peek-popover').waitFor({ state: 'visible', timeout: 2_000 })

    await card.locator('.run-card-activate').click()

    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.hover-peek-popover')).toBeHidden({ timeout: 2_000 })
  })
})
