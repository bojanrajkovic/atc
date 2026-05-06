import { expect, test } from '@playwright/test'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * Cross-platform Cmd/Ctrl+K chord. Uses Meta on macOS, Control elsewhere.
 * Applied to every chord press so CI (Linux) works alongside local macOS.
 */
const cmdOrCtrl = process.platform === 'darwin' ? 'Meta' : 'Control'

/** Standard page setup: inject WS mock, stub /v1/state, navigate, wait for connected. */
async function setupPage(page: import('@playwright/test').Page) {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  // Stub matchMedia so HoverPeekPopover's canHover flag is always false.
  // This prevents the 250ms hover timer from firing during keyboard-driven tests
  // and stealing focus from the element that onCloseAutoFocus just restored.
  await page.addInitScript(() => {
    const original = window.matchMedia
    window.matchMedia = (query: string): MediaQueryList => {
      if (query === '(hover: hover) and (pointer: fine)') {
        return {
          matches: false,
          media: query,
          addListener: () => {},
          removeListener: () => {},
          addEventListener: () => {},
          removeEventListener: () => {},
          dispatchEvent: () => false,
          onchange: null,
        } as unknown as MediaQueryList
      }
      return original.call(window, query)
    }
  })
  await page.route('**/v1/state', (route) => {
    route.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({ lastSeq: 1, runs: [], jobs: [] }),
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

/**
 * Seed run id=1 via WS and open the detail panel by clicking the RunCard's
 * activator button. This path sets uiStore.lastTriggerRunId so that the
 * panel's onCloseAutoFocus can restore focus to the card.
 */
async function seedAndOpenPanelViaClick(page: import('@playwright/test').Page) {
  await sendWS(
    page,
    makeRunEvent(1, {
      runId: 1,
      displayTitle: 'CI — stacking-test',
      createdAt: new Date().toISOString(),
      runStartedAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      action: { type: 'InProgress' },
    }),
  )
  // Wait for the card to render before clicking
  await page.waitForSelector('.run-card', { timeout: 5_000 })
  const card = page.locator('.run-card').first()
  await card.locator('.run-card-activate').click()
  await page.waitForSelector('[role="dialog"]', { timeout: 5_000 })
}

test.describe('Sheet + Command stacking', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
    await seedAndOpenPanelViaClick(page)
  })

  // -----------------------------------------------------------------------
  // AC6.1 + AC6.6 — Cmd+K opens palette on top of panel; second Cmd+K closes it
  // -----------------------------------------------------------------------
  test('interactivity.AC6.1 + AC6.6 Cmd+K opens palette on top of panel; second Cmd+K closes it', async ({
    page,
  }) => {
    await page.keyboard.press(`${cmdOrCtrl}+k`)

    // Both stores reflect the open state
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(true)
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()

    // Palette dialog is visible — assert on palette-specific element
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()

    // Both dialogs are in the DOM with data-state="open" (panel + palette)
    await expect(page.locator('[data-dialog-content][data-state="open"]')).toHaveCount(2)

    // Second Cmd+K closes the palette (toggle behavior)
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)

    // Panel is still open
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()
  })

  // -----------------------------------------------------------------------
  // AC6.5 — Only one backdrop overlay visible when both dialogs are open
  // -----------------------------------------------------------------------
  test('interactivity.AC6.5 only one backdrop overlay is visible when both dialogs are open', async ({
    page,
  }) => {
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()

    // Collect both overlay elements in DOM order
    const overlays = await page.locator('[data-dialog-overlay]').all()
    expect(overlays).toHaveLength(2) // both exist in the DOM

    const displays = await Promise.all(
      overlays.map((el) => el.evaluate((node) => getComputedStyle(node as Element).display)),
    )
    // First overlay (panel, mounted first) must be visible
    expect(displays[0]).not.toBe('none')
    // Second overlay (palette, mounted second) is hidden by the sibling-combinator CSS rule:
    //   [data-dialog-overlay] ~ [data-dialog-overlay] { display: none }
    expect(displays[1]).toBe('none')
  })

  // -----------------------------------------------------------------------
  // AC6.2 + AC6.3 — Esc unwinds palette first, then panel; focus restoration order
  //
  // Uses the click-activated panel (beforeEach) so uiStore.lastTriggerRunId is
  // set and the panel's onCloseAutoFocus can restore focus to the RunCard button.
  // -----------------------------------------------------------------------
  test('interactivity.AC6.2 + AC6.3 Esc unwinds palette first then panel; focus restoration order', async ({
    page,
  }) => {
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()

    // First Esc: closes palette only (AC6.2)
    await page.keyboard.press('Escape')

    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()

    // Focus should have restored to panel's close button via CommandPalette's onCloseAutoFocus
    expect(await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))).toBe(
      'Close detail panel',
    )

    // Second Esc: closes the panel (AC6.3)
    await page.keyboard.press('Escape')

    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).toBeNull()

    // Focus should have restored to the triggering RunCard's .run-card-activate button
    // via RunDetailPanel's onCloseAutoFocus (reads uiStore.lastTriggerRunId, set by card click)
    expect(
      await page.evaluate(() => document.activeElement?.classList.contains('run-card-activate')),
    ).toBe(true)
  })

  // AC6.4: click outside palette content closes the palette only; panel defers.
  // See docs/architecture/frontend-app.md "Sheet + Command Dialog Stacking" for
  // how bits-ui's dismissable-layer treats clicks inside vs. outside content refs.
  test('interactivity.AC6.4 click outside palette closes palette while panel stays open', async ({
    page,
  }) => {
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()

    // Wait until BOTH dialog contents have transitioned to data-state="open".
    // `[data-slot="command-input"]` becoming visible only confirms the input
    // is in the DOM; Bits UI's dismissable-layer registers its global
    // pointerdown listener inside an $effect that runs after the dialog
    // content mounts. data-state="open" is set synchronously when the dialog
    // primitive's open prop flips, so it can be observed BEFORE the $effect
    // has flushed.
    await expect(page.locator('[data-dialog-content][data-state="open"]')).toHaveCount(2)

    // Wait one rAF + a microtask drain inside the page so Svelte's effect
    // queue runs and Bits UI's dismissable-layer binds its pointerdown
    // listener. This is what makes the click below land deterministically.
    // We avoid retrying the click because once the palette closes, the panel
    // becomes the topmost layer and a follow-up click would trigger its
    // defer-otherwise-close fallback (close, since nothing's above it),
    // tearing down the panel and breaking this test's invariant.
    await page.evaluate(
      () =>
        new Promise<void>((resolve) =>
          requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
        ),
    )

    // Use absolute viewport coordinates rather than a region-relative click
    // with force:true. The panel overlay sits on top of the kanban in the
    // compositing order; the resolved coordinates of any region-targeted
    // click also shift when panel/palette CSS changes. Direct coords at
    // (5,5) are unambiguously outside both dialogs (panel: right slide-over,
    // palette: centered) regardless of layout.
    await page.mouse.click(5, 5)

    // Palette closes (default 'close' on the topmost layer); the panel's
    // 'defer-otherwise-close' defers because the palette is above it.
    await page.waitForFunction(() => window.__stores!.paletteStore!.paletteOpen === false, {
      timeout: 3_000,
    })
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()
  })
})
