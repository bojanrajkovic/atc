import { expect, test } from './lib/fixtures'
import { setupMockedPage } from './lib/page-setup'
import { makeRunEvent, sendWS } from './lib/ws-mock'

/**
 * Seed one run into each of the three kanban columns:
 *   run 1 → Queued (left column)
 *   run 2 → InProgress (middle column)
 *   run 3 → Completed/Success (right column)
 *
 * Distinct displayTitle values per run so aria-label assertions are unambiguous.
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
    await setupMockedPage(page, { viewport: { width: 1280, height: 720 } })
    await seedThreeRuns(page)
  })

  test('interactivity — click on activator button opens RunDetailPanel', async ({ page }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    const selectedRunId = await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)
    expect(selectedRunId).not.toBeNull()
  })

  test('interactivity — Enter on focused activator button opens RunDetailPanel', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').focus()
    await page.keyboard.press('Enter')
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
  })

  test('interactivity — Space on focused activator button opens RunDetailPanel', async ({
    page,
  }) => {
    const card = page.locator('.run-card').first()
    await card.locator('.run-card-activate').focus()
    await page.keyboard.press('Space')
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
  })

  test('interactivity — Tab from outside kanban lands on the single tabindex=0 card, second Tab exits', async ({
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

    // Priority: Queued > InProgress > Completed — the focused card is the first of the Queued column.
    const landedLabel = await page.evaluate(
      () => document.activeElement?.getAttribute('aria-label') ?? '',
    )
    expect(landedLabel).toBe('CI — queued, Queued, test-repo·main')

    // Exactly one card has tabindex=0 at this point.
    const tabzeroCount = await page.locator('.run-card-activate[tabindex="0"]').count()
    expect(tabzeroCount).toBe(1)

    // A second Tab moves focus OUT of the kanban (no second .run-card-activate).
    await page.keyboard.press('Tab')
    const stillOnCard = await page.evaluate(
      () => document.activeElement?.classList.contains('run-card-activate') ?? false,
    )
    expect(
      stillOnCard,
      'Second Tab should exit the kanban (no longer on a run-card-activate)',
    ).toBe(false)
  })

  test('interactivity — click on title text bubbles through transparent overlay to inner button', async ({
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

  test('interactivity — hover 250ms shows popover anchored to the right of the card', async ({
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

  test('interactivity — popover auto-flips left for card in rightmost column', async ({ page }) => {
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

  test('interactivity — mouse-leave dismisses popover immediately', async ({ page }) => {
    const card = page.locator('.run-card').first()
    await card.hover()
    await page.waitForTimeout(300)
    await expect(page.locator('.hover-peek-popover')).toBeVisible({ timeout: 2_000 })

    // Move cursor far from the card (top-left corner of viewport)
    await page.mouse.move(0, 0)

    await expect(page.locator('.hover-peek-popover')).toBeHidden({ timeout: 2_000 })
  })

  test('interactivity — click on hovered card opens panel and dismisses popover', async ({
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
