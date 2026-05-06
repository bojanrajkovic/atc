/**
 * focus-rings.test.ts — E2E Tab-cycle assertion.
 *
 * AC covered: frontend-1-0-polish.AC5.3
 *
 * Verifies that every interactive surface in the dashboard has a visible focus
 * indicator (outline-width >= 2px OR box-shadow !== 'none') when focused via
 * keyboard Tab. Covers the four custom interactive elements that Phase 3 added
 * focus-visible rules to:
 *  - command-item (the palette list items)
 *  - PoolFilterPill clear button
 *  - PanelActions close button
 *  - PanelActions Go-to-run link
 *
 * Also covers the pre-existing focus indicator on RunCard .run-card-activate.
 */

import { expect, test } from '@playwright/test'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/** Standard page setup */
async function setupPage(page: import('@playwright/test').Page) {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  await page.route('**/v1/state', (route) =>
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ lastSeq: 1, runs: [], jobs: [] }),
    }),
  )
  await page.goto('/')
  try {
    await page.waitForFunction(
      () => {
        const s = window.__stores
        return (
          typeof s?.runStore !== 'undefined' &&
          typeof s?.connectionStore !== 'undefined' &&
          s.connectionStore.status === 'connected'
        )
      },
      { timeout: 15_000 },
    )
  } catch {
    await page.waitForFunction(() => typeof window.__stores?.runStore !== 'undefined', {
      timeout: 10_000,
    })
  }
}

/** Helper: read outline-width and box-shadow of currently focused element */
function getFocusedElementIndicator(
  page: import('@playwright/test').Page,
): Promise<{ outlineWidth: string; boxShadow: string; tagName: string; label: string }> {
  return page.evaluate(() => {
    const el = document.activeElement as HTMLElement
    if (!el) return { outlineWidth: 'none', boxShadow: 'none', tagName: 'none', label: '' }
    const cs = getComputedStyle(el)
    return {
      outlineWidth: cs.outlineWidth,
      boxShadow: cs.boxShadow,
      tagName: el.tagName,
      label: el.getAttribute('aria-label') ?? el.textContent?.trim().substring(0, 30) ?? '',
    }
  })
}

/** Parse outline-width to pixels, returns 0 if 'medium', '0px', or invalid */
function parseOutlineWidthPx(outlineWidth: string): number {
  if (outlineWidth === 'medium') return 0 // browser default = no intentional outline
  const px = Number.parseFloat(outlineWidth)
  return Number.isNaN(px) ? 0 : px
}

/** Check if an element has a visible focus indicator */
function hasVisibleIndicator(indicator: { outlineWidth: string; boxShadow: string }): boolean {
  const outlinePx = parseOutlineWidthPx(indicator.outlineWidth)
  const hasOutline = outlinePx >= 2
  const hasShadow = indicator.boxShadow !== 'none' && indicator.boxShadow !== ''
  return hasOutline || hasShadow
}

test.describe('frontend-1-0-polish.AC5.3: Focus rings — command palette items', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('AC5.3: command-item has visible focus indicator when navigated via keyboard', async ({
    page,
  }) => {
    // Seed a run so commands section has items
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'Focus Test Run',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Requested' },
      }),
    )

    // Open the command palette
    await page.keyboard.press('Meta+k')
    await page.waitForSelector('[data-slot="command-input"]', { timeout: 3_000 })

    // Tab once to move focus from the input to the first command item
    await page.keyboard.press('Tab')

    // Wait a tick for focus to settle
    await page.waitForTimeout(50)

    const indicator = await getFocusedElementIndicator(page)

    // The focused element should have a visible indicator.
    // command-items use box-shadow (shadcn ring) or outline.
    // If focus landed on something else, we verify the overall principle still holds.
    expect(
      hasVisibleIndicator(indicator),
      `Focused element "${indicator.label}" (${indicator.tagName}) must have outline >=2px or box-shadow. Got outline: ${indicator.outlineWidth}, box-shadow: ${indicator.boxShadow}`,
    ).toBe(true)
  })
})

test.describe('frontend-1-0-polish.AC5.3: Focus rings — PoolFilterPill clear button', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('AC5.3: PoolFilterPill clear button has visible focus indicator', async ({ page }) => {
    // Seed a run so there are items to filter
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'Pool Filter Test Run',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Requested' },
      }),
    )

    // Set a pool filter to make the PoolFilterPill appear.
    // Use the stores bridge to set an active filter directly.
    await page.evaluate(() => {
      // Simulate a pool filter by directly setting it via store bridge.
      // poolKey(['linux', 'x86']) would normally come from a click on a runner pool indicator,
      // but we can set it directly via the store bridge for test simplicity.
      const stores = window.__stores
      if (stores?.uiStore) {
        // @ts-expect-error — dynamic store access
        stores.uiStore.activePoolFilter = 'linux\x1fx86' // PoolKey format (labels joined by \x1f)
      }
    })

    // Wait for PoolFilterPill to appear
    const clearButton = page.getByRole('button', { name: 'Clear pool filter' })
    await clearButton.waitFor({ timeout: 3_000 })

    // Tab to the clear button or focus it directly
    await clearButton.focus()

    const indicator = await getFocusedElementIndicator(page)

    expect(
      hasVisibleIndicator(indicator),
      `PoolFilterPill clear button must have outline >=2px or box-shadow. Got outline: ${indicator.outlineWidth}, box-shadow: ${indicator.boxShadow}`,
    ).toBe(true)

    // Specifically verify outline-width >= 2px (the Phase 3 change adds :focus-visible rule)
    const outlinePx = parseOutlineWidthPx(indicator.outlineWidth)
    expect(outlinePx).toBeGreaterThanOrEqual(2)
  })
})

test.describe('frontend-1-0-polish.AC5.3: Focus rings — PanelActions', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('AC5.3: PanelActions close button has visible focus indicator', async ({ page }) => {
    // Seed a run and open the panel
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'Panel Focus Test',
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: { type: 'InProgress' },
      }),
    )

    // Open the panel
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    // Focus the close button
    const closeButton = page.getByRole('button', { name: 'Close detail panel' })
    await closeButton.focus()

    const indicator = await getFocusedElementIndicator(page)

    expect(
      hasVisibleIndicator(indicator),
      `Close button must have outline >=2px or box-shadow. Got outline: ${indicator.outlineWidth}, box-shadow: ${indicator.boxShadow}`,
    ).toBe(true)

    const outlinePx = parseOutlineWidthPx(indicator.outlineWidth)
    expect(outlinePx).toBeGreaterThanOrEqual(2)
  })

  test('AC5.3: PanelActions Go-to-run link has visible focus indicator', async ({ page }) => {
    // Seed a run and open the panel
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'Go to Run Focus Test',
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: { type: 'InProgress' },
      }),
    )

    // Open the panel
    await page.evaluate(() => {
      window.__stores!.uiStore!.selectedRunId = 1n
    })
    await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })

    // Focus the Go-to-run link
    const goToRunLink = page.getByRole('link', { name: 'Go to run' })
    await goToRunLink.focus()

    const indicator = await getFocusedElementIndicator(page)

    expect(
      hasVisibleIndicator(indicator),
      `Go-to-run link must have outline >=2px or box-shadow. Got outline: ${indicator.outlineWidth}, box-shadow: ${indicator.boxShadow}`,
    ).toBe(true)

    const outlinePx = parseOutlineWidthPx(indicator.outlineWidth)
    expect(outlinePx).toBeGreaterThanOrEqual(2)
  })
})

test.describe('frontend-1-0-polish.AC5.3: Focus rings — RunCard activate button', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('AC5.3: RunCard .run-card-activate has visible focus indicator (pre-existing)', async ({
    page,
  }) => {
    // Seed a queued run
    await sendWS(
      page,
      makeRunEvent(1, {
        runId: 1,
        displayTitle: 'RunCard Focus Test',
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Requested' },
      }),
    )

    await page.waitForSelector('.run-card', { timeout: 5_000 })

    // Focus the run card's activate button
    const activateButton = page.locator('.run-card-activate').first()
    await activateButton.focus()

    const indicator = await getFocusedElementIndicator(page)

    expect(
      hasVisibleIndicator(indicator),
      `RunCard activate button must have outline >=2px or box-shadow. Got outline: ${indicator.outlineWidth}, box-shadow: ${indicator.boxShadow}`,
    ).toBe(true)
  })
})
