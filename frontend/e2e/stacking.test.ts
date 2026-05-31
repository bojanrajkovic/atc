import { expect, test } from './lib/fixtures'
import { setupMockedPage } from './lib/page-setup'
import { makeRunEvent, sendWS } from './lib/ws-mock'

/**
 * Cross-platform Cmd/Ctrl+K chord. Uses Meta on macOS, Control elsewhere.
 * Applied to every chord press so CI (Linux) works alongside local macOS.
 */
const cmdOrCtrl = process.platform === 'darwin' ? 'Meta' : 'Control'

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
    await setupMockedPage(page, { stubHover: true })
    await seedAndOpenPanelViaClick(page)
  })

  test('interactivity — Cmd+K opens palette on top of panel; second Cmd+K closes it', async ({
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

  test('interactivity — only one backdrop overlay is visible when both dialogs are open', async ({
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

  test('interactivity — Esc unwinds palette first then panel; focus restoration order', async ({
    page,
  }) => {
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.locator('[data-slot="command-input"]')).toBeVisible()

    // First Esc: closes palette only
    await page.keyboard.press('Escape')

    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(false)
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()

    // Focus should have restored to panel's close button via CommandPalette's onCloseAutoFocus
    expect(await page.evaluate(() => document.activeElement?.getAttribute('aria-label'))).toBe(
      'Close detail panel',
    )

    // Second Esc: closes the panel
    await page.keyboard.press('Escape')

    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).toBeNull()

    // Focus should have restored to the triggering RunCard's .run-card-activate button
    // via RunDetailPanel's onCloseAutoFocus (reads uiStore.lastTriggerRunId, set by card click)
    expect(
      await page.evaluate(() => document.activeElement?.classList.contains('run-card-activate')),
    ).toBe(true)
  })

  test('interactivity — click outside palette closes palette while panel stays open', async ({
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
