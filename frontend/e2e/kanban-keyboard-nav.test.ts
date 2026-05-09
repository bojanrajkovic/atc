import type { Page } from '@playwright/test'
import { expect, test } from './lib/fixtures'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E integration verification for kanban 2D keyboard navigation.
 *
 * Covers: 2D arrow navigation, edge/asymmetric-column behavior, modifier-key
 * delegation, suspension via natural focus scoping, card-stable transitions,
 * and lost-trigger restoration.
 *
 * Tab-in entry and tabindex invariants are covered in run-card-interactivity.test.ts.
 * All scenarios use focusFirstCard() for deterministic focus entry.
 *
 * @see docs/design-plans/2026-05-01-kanban-keyboard-nav.md
 */

// ---------------------------------------------------------------------------
// Cross-platform modifier (Meta on macOS, Control on Linux/Windows)
// ---------------------------------------------------------------------------

const cmdOrCtrl = process.platform === 'darwin' ? 'Meta' : 'Control'

// ---------------------------------------------------------------------------
// setupPage — standard harness
// ---------------------------------------------------------------------------

/**
 * Standard page setup: inject WS mock (replaces /v1/ws WebSocket), disable
 * hover-media-query so HoverPeekPopover never opens during keyboard tests
 * (open popover suppresses RunCard's auto-focus $effect via !popoverOpen guard),
 * stub /v1/state, navigate, wait for connected.
 */
async function setupPage(page: Page): Promise<void> {
  await page.addInitScript(WS_MOCK_INIT_SCRIPT)

  // Disable hover capability so HoverPeekPopover.canHover === false.
  // If the Playwright mouse cursor lands over a card during keyboard tests,
  // the 250ms debounce would fire and popoverOpen would become true, which
  // suppresses RunCard's $effect focus sync. Stubbing matchMedia prevents this.
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
    // Fallback: at minimum wait for uiStore to be available
    await page.waitForFunction(() => typeof window.__stores?.uiStore !== 'undefined', {
      timeout: 10_000,
    })
  }
}

// ---------------------------------------------------------------------------
// focusedRunId — stable focus assertion via data-run-id
// ---------------------------------------------------------------------------

/**
 * Returns the data-run-id of the run-card article that contains the currently
 * focused element, or null if focus is not inside any run-card.
 *
 * Prefers data-run-id over aria-label because run id is the stable identity
 * used by the roving context; aria-label encodes display state.
 */
async function focusedRunId(page: Page): Promise<string | null> {
  return await page.evaluate(() => {
    const active = document.activeElement
    if (!(active instanceof HTMLElement)) return null
    const article = active.closest('article.run-card')
    return article?.getAttribute('data-run-id') ?? null
  })
}

// ---------------------------------------------------------------------------
// seedQueued — seed N queued runs, runIds 1..N
// ---------------------------------------------------------------------------

/**
 * Seed `count` queued runs with sequential runIds (1..count).
 * displayTitle encodes the position so failure messages are human-readable.
 */
async function seedQueued(page: Page, count: number, baseSeq = 0): Promise<void> {
  for (let i = 1; i <= count; i++) {
    await sendWS(
      page,
      makeRunEvent(baseSeq + i, {
        runId: i,
        displayTitle: `Queued #${i}`,
        createdAt: new Date(2026, 0, 1, 12, 0, i).toISOString(),
        runStartedAt: null,
        updatedAt: new Date(2026, 0, 1, 12, 0, i).toISOString(),
        action: { type: 'Requested' },
      }),
    )
  }
  // Wait for all cards to be visible before proceeding
  await expect(page.locator('.run-card')).toHaveCount(count, { timeout: 5_000 })
}

// ---------------------------------------------------------------------------
// seedInProgress — seed N in-progress runs, runIds starting at offset
// ---------------------------------------------------------------------------

/**
 * Seed `count` in-progress runs. runIds start at `runIdOffset` to avoid
 * collisions with queued runs. Used in asymmetric-column tests.
 *
 * inProgressRuns sorts DESCENDING by runStartedAt (most-recent first).
 * To ensure InProgress #1 appears at row 0, give it the largest (latest)
 * timestamp — subtract (i-1) seconds from the base time so #1 is latest,
 * #2 is one second earlier, etc.
 */
async function seedInProgress(
  page: Page,
  count: number,
  runIdOffset: number,
  baseSeq: number,
): Promise<void> {
  // Base time — we count DOWN so #1 has the latest timestamp and sorts first
  const base = new Date(2026, 0, 1, 12, 1, 59).getTime() // 12:01:59
  for (let i = 1; i <= count; i++) {
    const runId = runIdOffset + i
    // #1 → base, #2 → base-1s, etc. (descending sort means #1 is row 0)
    const ts = new Date(base - (i - 1) * 1000).toISOString()
    await sendWS(
      page,
      makeRunEvent(baseSeq + i, {
        runId,
        displayTitle: `InProgress #${i}`,
        createdAt: ts,
        runStartedAt: ts,
        updatedAt: ts,
        action: { type: 'InProgress' },
      }),
    )
  }
}

// ---------------------------------------------------------------------------
// seedCompleted — seed N completed runs, runIds starting at offset
// ---------------------------------------------------------------------------

/**
 * Seed `count` completed runs. runIds start at `runIdOffset`.
 *
 * completedRuns sorts DESCENDING by updatedAt (most-recent first).
 * To ensure Completed #1 appears at row 0, give it the largest (latest)
 * updatedAt — subtract (i-1) seconds from the base time so #1 is latest.
 */
async function seedCompleted(
  page: Page,
  count: number,
  runIdOffset: number,
  baseSeq: number,
): Promise<void> {
  const base = new Date(2026, 0, 1, 12, 2, 59).getTime() // 12:02:59
  for (let i = 1; i <= count; i++) {
    const runId = runIdOffset + i
    // #1 → base, #2 → base-1s, etc. (descending sort means #1 is row 0)
    const ts = new Date(base - (i - 1) * 1000).toISOString()
    await sendWS(
      page,
      makeRunEvent(baseSeq + i, {
        runId,
        displayTitle: `Completed #${i}`,
        createdAt: ts,
        runStartedAt: ts,
        updatedAt: ts,
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )
  }
}

// ---------------------------------------------------------------------------
// focusFirstCard — deterministic focus entry (not Tab-based)
// ---------------------------------------------------------------------------

/**
 * Focus the first .run-card-activate button directly, bypassing Tab-order
 * brittleness. All scenarios except Tab-into-kanban entry (covered in
 * run-card-interactivity.test.ts) use this helper.
 *
 * Waits for focus to land before returning so that downstream focusedRunId()
 * reads don't race the focusin listener on the roving action.
 */
async function focusFirstCard(page: Page): Promise<void> {
  await page.locator('article.run-card').first().locator('.run-card-activate').focus()
  // Wait for actual focus landing — not just seeding check
  await page.waitForFunction(
    () => document.activeElement?.classList.contains('run-card-activate'),
    { timeout: 3_000 },
  )
}

// ---------------------------------------------------------------------------
// 2D arrow navigation
// ---------------------------------------------------------------------------

test.describe('2D arrow navigation', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav — ArrowDown moves focus to next card in same column', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')

    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')
  })

  test('kanban-keyboard-nav — ArrowUp moves focus to previous card in same column', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)

    // Navigate to last card
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')

    // ArrowUp moves back
    await page.keyboard.press('ArrowUp')
    expect(await focusedRunId(page)).toBe('2')

    await page.keyboard.press('ArrowUp')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — ArrowRight moves focus to corresponding row in next non-empty column', async ({
    page,
  }) => {
    // 3 queued + 2 in-progress + 1 completed
    await seedQueued(page, 3)
    await seedInProgress(page, 2, 100, 10)
    await seedCompleted(page, 1, 200, 20)
    // Wait for all cards
    await expect(page.locator('.run-card')).toHaveCount(6, { timeout: 5_000 })

    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // ArrowRight from queued col (row 0) → inProgress col (row 0 = runId 101)
    await page.keyboard.press('ArrowRight')
    expect(await focusedRunId(page)).toBe('101')
  })

  test('kanban-keyboard-nav — ArrowLeft moves focus to corresponding row in previous non-empty column', async ({
    page,
  }) => {
    // 3 queued + 2 in-progress
    await seedQueued(page, 3)
    await seedInProgress(page, 2, 100, 10)
    await expect(page.locator('.run-card')).toHaveCount(5, { timeout: 5_000 })

    // Focus the first inProgress card programmatically
    await page.locator('article.run-card[data-run-id="101"]').locator('.run-card-activate').focus()
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 3_000 },
    )
    expect(await focusedRunId(page)).toBe('101')

    // ArrowLeft → queued col row 0 (runId 1)
    await page.keyboard.press('ArrowLeft')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — Home moves focus to the first card in the current column', async ({
    page,
  }) => {
    await seedQueued(page, 5)
    await focusFirstCard(page)

    // Navigate to row 3 (runId=4, 0-indexed row 3)
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('4')

    // Home → back to first
    await page.keyboard.press('Home')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — End moves focus to the last card in the current column', async ({
    page,
  }) => {
    await seedQueued(page, 5)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    // End → last card
    await page.keyboard.press('End')
    expect(await focusedRunId(page)).toBe('5')
  })

  test('kanban-keyboard-nav — ArrowDown calls event.preventDefault() — page does not scroll', async ({
    page,
  }) => {
    // Seed 5 queued cards. We verify preventDefault via event observation,
    // not by relying on page scrollability (which may not exist).
    await seedQueued(page, 5)
    await focusFirstCard(page)

    // Navigate to bottom of column
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('ArrowDown')
    }
    expect(await focusedRunId(page)).toBe('5')

    // Install a document-level keydown listener that records defaultPrevented.
    // The interface cast gives us dot-notation access (satisfies useLiteralKeys
    // and exactOptionalPropertyTypes simultaneously).
    interface TestWindow extends Window {
      __lastArrowDefaultPrevented: boolean
    }
    await page.evaluate(() => {
      ;(window as unknown as TestWindow).__lastArrowDefaultPrevented = false
      document.addEventListener(
        'keydown',
        (e) => {
          if (e.key === 'ArrowDown') {
            ;(window as unknown as TestWindow).__lastArrowDefaultPrevented = e.defaultPrevented
          }
        },
        { capture: false, once: true },
      )
    })

    // Press ArrowDown at last row — roving handler fires first (bubble phase, no capture),
    // but the document listener fires at the same bubble phase and captures the state.
    // Since the roving listener calls preventDefault synchronously before the document
    // listener fires (same bubble propagation), defaultPrevented will be true.
    await page.keyboard.press('ArrowDown')

    const prevented = await page.evaluate(
      () => (window as unknown as TestWindow).__lastArrowDefaultPrevented,
    )
    expect(prevented).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Edge and asymmetric-column behavior
// ---------------------------------------------------------------------------

test.describe('Edge and asymmetric-column behavior', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav — ArrowDown at last card is a no-op', async ({ page }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)

    // Navigate to last card
    await page.keyboard.press('ArrowDown')
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')

    // ArrowDown at last row — focus stays on runId=3
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')
  })

  test('kanban-keyboard-nav — ArrowUp at first card is a no-op', async ({ page }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    // ArrowUp at row 0 — focus stays on runId=1
    await page.keyboard.press('ArrowUp')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — ArrowRight in rightmost non-empty column is a no-op', async ({
    page,
  }) => {
    // Seed only 1 queued run (queued is leftmost; but with only queued seeded
    // it IS the only column and thus rightmost non-empty).
    await seedQueued(page, 1)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowRight')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — ArrowLeft in leftmost non-empty column is a no-op', async ({
    page,
  }) => {
    await seedQueued(page, 1)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowLeft')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — ArrowRight skips empty inProgress column and lands in completed', async ({
    page,
  }) => {
    // 2 queued + 0 inProgress + 1 completed
    await seedQueued(page, 2)
    await seedCompleted(page, 1, 200, 10)
    await expect(page.locator('.run-card')).toHaveCount(3, { timeout: 5_000 })

    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // ArrowRight skips empty inProgress, lands in completed (runId=201)
    await page.keyboard.press('ArrowRight')
    expect(await focusedRunId(page)).toBe('201')
  })

  test('kanban-keyboard-nav — ArrowLeft skips empty inProgress column symmetrically', async ({
    page,
  }) => {
    // 2 queued + 0 inProgress + 1 completed
    await seedQueued(page, 2)
    await seedCompleted(page, 1, 200, 10)
    await expect(page.locator('.run-card')).toHaveCount(3, { timeout: 5_000 })

    // Focus the completed card directly
    await page.locator('article.run-card[data-run-id="201"]').locator('.run-card-activate').focus()
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 3_000 },
    )
    expect(await focusedRunId(page)).toBe('201')

    // ArrowLeft skips empty inProgress, lands in queued col row 0 (runId=1)
    await page.keyboard.press('ArrowLeft')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — asymmetric clamp: ArrowRight from row 5 of 10-card queued to 3-card inProgress clamps to last row', async ({
    page,
  }) => {
    // 10 queued (runIds 1-10) + 3 inProgress (runIds 101, 102, 103)
    await seedQueued(page, 10)
    await seedInProgress(page, 3, 100, 20)
    await expect(page.locator('.run-card')).toHaveCount(13, { timeout: 5_000 })

    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Navigate to row 5 (index 5 = runId=6, 0-indexed)
    for (let i = 0; i < 5; i++) {
      await page.keyboard.press('ArrowDown')
    }
    expect(await focusedRunId(page)).toBe('6')

    // ArrowRight to inProgress: desired row=5, but inProgress has 3 items (max index=2)
    // → clamped to last row = inProgress row 2 = runId=103
    await page.keyboard.press('ArrowRight')
    expect(await focusedRunId(page)).toBe('103')
  })

  test('kanban-keyboard-nav — ArrowRight skips multiple empty columns to find furthest non-empty', async ({
    page,
  }) => {
    // Only queued (leftmost) + completed (rightmost) — inProgress is empty.
    // Same fixture as the empty-column-skip test, asserting the skip works over a wider empty stretch.
    await seedQueued(page, 1)
    await seedCompleted(page, 2, 300, 10)
    await expect(page.locator('.run-card')).toHaveCount(3, { timeout: 5_000 })

    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowRight')
    // Should land in completed col row 0 = runId=301
    expect(await focusedRunId(page)).toBe('301')
  })
})

// ---------------------------------------------------------------------------
// Modifier-key delegation
// ---------------------------------------------------------------------------

test.describe('Modifier-key delegation', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav — Cmd+K opens command palette while card focused', async ({ page }) => {
    await seedQueued(page, 1)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press(`${cmdOrCtrl}+k`)

    // Palette dialog becomes visible
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    // Combobox input receives focus (palette's onOpenAutoFocus)
    await expect(page.locator('input[role="combobox"]')).toBeFocused()
  })

  test('kanban-keyboard-nav — Cmd+D toggles dark mode while card focused and does not move kanban focus', async ({
    page,
  }) => {
    await seedQueued(page, 2)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Default is dark — data-mode absent
    expect(await page.locator('html').getAttribute('data-mode')).toBeNull()

    await page.keyboard.press(`${cmdOrCtrl}+d`)
    await page.waitForTimeout(50)

    // Theme toggled to light
    expect(await page.locator('html').getAttribute('data-mode')).toBe('light')

    // Kanban focus unchanged — the action's modifier-guard returned early before
    // ArrowDown/Up/Left/Right processing, and Cmd+D is not an arrow key anyway.
    // Verify by checking focus is still on a run-card-activate element.
    const stillOnCard = await page.evaluate(() =>
      document.activeElement?.classList.contains('run-card-activate'),
    )
    expect(stillOnCard).toBe(true)
  })

  test('kanban-keyboard-nav — Cmd+\\ toggles compact density while card focused', async ({
    page,
  }) => {
    await seedQueued(page, 1)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Default: no data-density
    expect(await page.locator('html').getAttribute('data-density')).toBeNull()

    await page.keyboard.press(`${cmdOrCtrl}+\\`)
    await page.waitForTimeout(50)

    expect(await page.locator('html').getAttribute('data-density')).toBe('compact')

    // Focus still on card
    const stillOnCard = await page.evaluate(() =>
      document.activeElement?.classList.contains('run-card-activate'),
    )
    expect(stillOnCard).toBe(true)
  })

  test('kanban-keyboard-nav — Cmd+ArrowDown returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Cmd+ArrowDown — the roving action's modifier-guard fires first (event.metaKey ||
    // event.ctrlKey) and returns early, so focus stays on runId=1.
    await page.keyboard.press(`${cmdOrCtrl}+ArrowDown`)
    await page.waitForTimeout(50)

    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — Cmd+ArrowUp returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    // Navigate to middle card first
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')

    await page.keyboard.press(`${cmdOrCtrl}+ArrowUp`)
    await page.waitForTimeout(50)

    // Focus unchanged — still on runId=2
    expect(await focusedRunId(page)).toBe('2')
  })

  test('kanban-keyboard-nav — Shift+ArrowDown returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('Shift+ArrowDown')
    await page.waitForTimeout(50)

    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — Alt+ArrowDown returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('Alt+ArrowDown')
    await page.waitForTimeout(50)

    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — bare ArrowDown is claimed by kanban and does not open palette', async ({
    page,
  }) => {
    await seedQueued(page, 2)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Sanity: palette is not open
    await expect(page.getByRole('dialog')).toHaveCount(0)

    // Bare ArrowDown — kanban claims it, moves focus to runId=2
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')

    // Palette did NOT open (window-level handler has metaKey||ctrlKey guard; bare keys don't pass it)
    await expect(page.getByRole('dialog')).toHaveCount(0)
  })
})

// ---------------------------------------------------------------------------
// Suspension via natural focus scoping
// ---------------------------------------------------------------------------

test.describe('Suspension via natural focus scoping', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav — palette open: ArrowDown does not move kanban focus; Esc returns to card', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Open palette — focus moves to combobox input inside palette
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('input[role="combobox"]')).toBeFocused()

    // Press ArrowDown — focus is inside the palette (combobox), not the kanban.
    // The roving action's keydown only fires when focus is inside its node. Since
    // the palette traps focus, the event target is the combobox input, which is
    // NOT a descendant of the kanban grid element — so the roving action never
    // fires; the palette's own ArrowDown behavior handles it.
    await page.keyboard.press('ArrowDown')

    // Close palette and verify kanban focus is still on queued-1
    await page.keyboard.press('Escape')
    await expect(page.getByRole('dialog')).toHaveCount(0, { timeout: 3_000 })

    // After Esc, palette's onCloseAutoFocus restores focus. Wait for a run-card
    // to become the active element (RunDetailPanel's onCloseAutoFocus path or
    // fallback). The important assertion is that queuedRuns[0] in the store is
    // still runId=1 — the kanban context was not mutated while palette was open.
    const firstQueuedId = await page.evaluate(
      () => window.__stores!.runStore!.queuedRuns[0]?.id?.toString() ?? null,
    )
    expect(firstQueuedId).toBe('1')

    // Focus back on a run-card-activate after Esc
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 3_000 },
    )
    // The card that has focus should be the initial card (the roving context
    // reset to initialFocusRunId=1 when kanbanHasFocus was false while palette was open)
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — panel open: ArrowDown does not move kanban focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Open panel by clicking the card's activator button
    await page.locator('article.run-card[data-run-id="1"]').locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })

    // Panel is open — focus is trapped inside the Sheet dialog.
    // Press ArrowDown: focus is NOT inside the kanban grid, so roving action does not fire.
    await page.keyboard.press('ArrowDown')
    await page.waitForTimeout(50)

    // Verify via store state: the roving context's focusedRunId should still be 1
    // (or null, having fallen back to initialFocusRunId=1). The behavioral check is that
    // the panel is still open (focus not leaked back into kanban).
    const panelStillOpen = await page.evaluate(
      () => window.__stores!.uiStore!.selectedRunId !== null,
    )
    expect(panelStillOpen).toBe(true)

    // Stronger assertions: verify focus is still inside the dialog and
    // the kanban's roving tabindex state has not advanced.

    // 1. document.activeElement is inside the dialog
    const activeIsInsideDialog = await page.evaluate(() => {
      return document.activeElement?.closest('[role="dialog"]') !== null
    })
    expect(activeIsInsideDialog).toBe(true)

    // 2. document.activeElement is NOT a .run-card-activate
    const activeIsRunCardActivate = await page.evaluate(() => {
      return document.activeElement?.classList.contains('run-card-activate') ?? false
    })
    expect(activeIsRunCardActivate).toBe(false)

    // 3. The kanban's tabindex=0 card is still run 1 — roving context did not advance
    const tabindex0RunId = await page.evaluate(() => {
      const el = document.querySelector<HTMLElement>('.run-card-activate[tabindex="0"]')
      return el?.closest('[data-run-id]')?.getAttribute('data-run-id') ?? null
    })
    expect(tabindex0RunId).toBe('1')
  })

  test('kanban-keyboard-nav — both palette and panel stacked: ArrowDown affects neither', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Open panel first via click (sets lastTriggerRunId)
    await page.locator('article.run-card[data-run-id="1"]').locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })

    // Open palette on top of panel
    await page.keyboard.press(`${cmdOrCtrl}+k`)
    await expect(page.locator('input[role="combobox"]')).toBeFocused({ timeout: 3_000 })

    // Both stacked: focus is in palette combobox
    // Press ArrowDown — palette handles it (moves selection); kanban untouched
    await page.keyboard.press('ArrowDown')
    await page.waitForTimeout(50)

    // Both dialogs still open
    expect(await page.evaluate(() => window.__stores!.paletteStore!.paletteOpen)).toBe(true)
    expect(await page.evaluate(() => window.__stores!.uiStore!.selectedRunId)).not.toBeNull()
  })

  test('kanban-keyboard-nav — after panel closes, ArrowDown resumes kanban navigation', async ({
    page,
  }) => {
    await seedQueued(page, 2)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Open panel by clicking the first card's activator (sets lastTriggerRunId=1n)
    await page.locator('article.run-card[data-run-id="1"]').locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })

    // Close the panel via Escape
    await page.keyboard.press('Escape')
    await page.waitForFunction(() => window.__stores!.uiStore!.selectedRunId === null, {
      timeout: 3_000,
    })

    // RunDetailPanel's onCloseAutoFocus returns focus to lastTriggerRunId's card.
    // Wait for focus to land on a run-card-activate before pressing ArrowDown.
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 3_000 },
    )

    // Now press ArrowDown — kanban handler resumes, focus moves to runId=2
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')
  })
})

// ---------------------------------------------------------------------------
// Card-stable transitions and lost-trigger restoration
// ---------------------------------------------------------------------------

test.describe('Card-stable transitions and lost-trigger restoration', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav — burst events with held ArrowDown: card-stable across reorder', async ({
    page,
  }) => {
    // Seed 5 queued runs with EXACT UTC timestamps per the deterministic plan scenario.
    // Do NOT use seedQueued() here — that helper uses local-time new Date() constructors
    // which produce different UTC strings on non-UTC runners, breaking the timestamp math.
    // IDs 1-5 with displayTitles Q1-Q5, createdAt ascending: order is Q1,Q2,Q3,Q4,Q5.
    for (let i = 1; i <= 5; i++) {
      const padded = String(i).padStart(2, '0')
      await sendWS(
        page,
        makeRunEvent(i, {
          runId: i,
          displayTitle: `Q${i}`,
          createdAt: `2026-01-01T12:00:${padded}Z`,
          runStartedAt: null,
          updatedAt: `2026-01-01T12:00:${padded}Z`,
          action: { type: 'Requested' },
        }),
      )
    }
    await expect(page.locator('.run-card')).toHaveCount(5, { timeout: 5_000 })

    // Initial order: Q1(id=1), Q2(id=2), Q3(id=3), Q4(id=4), Q5(id=5)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // ArrowDown → Q2 (id=2)
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')

    // Burst event: re-issue Q4 with an EARLIER createdAt (11:59:59Z) — moves Q4 to top.
    // New sort order: Q4(id=4, 11:59:59), Q1(id=1, 12:00:01), Q2(id=2, 12:00:02),
    //                 Q3(id=3, 12:00:03), Q5(id=5, 12:00:05)
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 4,
        displayTitle: 'Q4',
        createdAt: '2026-01-01T11:59:59Z',
        runStartedAt: null,
        updatedAt: '2026-01-01T11:59:59Z',
        action: { type: 'Requested' },
      }),
    )

    // Press ArrowDown again. Card-stable contract: focus is anchored to run-id 2 (Q2),
    // which is now at row 2 of the new ordering. ArrowDown from row 2 → row 3 = run-id 3 (Q3).
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')
  })

  test('kanban-keyboard-nav — eviction during keyboard nav restores focus to initial card', async ({
    page,
  }) => {
    // Seed 3 queued runs. Navigate to queued-2.
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('2')

    // Evict queued-2 via SvelteMap reactive delete. The eviction $effect in
    // RovingFocusProvider detects the missing run and calls
    // restoreFocusToInitial() → focuses the first card of the first non-empty column.
    await page.evaluate((id: string) => {
      window.__stores!.runStore!.runs.delete(BigInt(id))
    }, '2')

    // Wait for the eviction $effect + RunCard $effect + DOM update to settle.
    // Use waitForFunction (retrying) rather than a snapshot read to avoid racing
    // the Svelte tick that follows the SvelteMap reactive delete.
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 3_000 },
    )

    // Focus should have been restored to queued-1 (the new initialFocusRunId).
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav — panel close with evicted trigger card: focus lands on initial card, not body', async ({
    page,
  }) => {
    // Seed 3 queued runs. Focus queued-1.
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // Open panel by clicking queued-1's activator button.
    // The click sets uiStore.lastTriggerRunId = 1n (canonical user path).
    await page.locator('article.run-card[data-run-id="1"]').locator('.run-card-activate').click()
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })

    // Evict queued-1 (the trigger card) by deleting its run from the store.
    // The eviction $effect fires: selectedRunId (1n) references a missing run
    // → sets selectedRunId = null. Because lastTriggerRunId === evictedId,
    // the $effect also calls ctx.restoreFocusToInitial() directly — bypassing
    // the Bits UI onCloseAutoFocus path (which would not fire because {#if run}
    // collapses the dialog content, removing the close button from the DOM before
    // FocusScope can see focus inside the scope at close time).
    await page.evaluate(() => {
      window.__stores!.runStore!.runs.delete(1n)
    })

    // Wait for selectedRunId to clear (panel fully closed by the eviction $effect).
    await page.waitForFunction(() => window.__stores!.uiStore!.selectedRunId === null, {
      timeout: 3_000,
    })

    // Wait for focus to land on a run-card-activate (restored via the eviction $effect).
    // Use a generous timeout because restoreFocusToInitial() calls tick() before
    // querying the DOM, and the Sheet exit animation may still be running.
    await page.waitForFunction(
      () => document.activeElement?.classList.contains('run-card-activate'),
      { timeout: 5_000 },
    )

    // The original bug: focus was left on <body> because event.preventDefault() ran
    // but the optional-chain ?.focus() silently no-opped. Assert regression is fixed.
    const activeTag = await page.evaluate(() => document.activeElement?.tagName)
    expect(activeTag).not.toBe('BODY')

    // Focus should be on the new initialFocusRunId = queued-2 (id=2, since id=1 was evicted).
    expect(await focusedRunId(page)).toBe('2')
  })
})

// ---------------------------------------------------------------------------
// Pool-filter arrow nav
// ---------------------------------------------------------------------------

test.describe('kanban-keyboard-nav — pool-filter', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  /**
   * Seed three queued runs with different pool labels:
   *   run 1 → pool-a (visible under pool-a filter)
   *   run 2 → pool-b (hidden under pool-a filter)
   *   run 3 → pool-a (visible under pool-a filter)
   *
   * Sends inline JSON because ws-mock's sendWS helper routes through
   * window.__stores bridge's bigint reviver and job event path, just
   * like the app's real WS dispatcher.
   */
  async function seedWithPools(page: import('@playwright/test').Page): Promise<void> {
    // Seed three queued runs.
    for (let i = 1; i <= 3; i++) {
      await sendWS(
        page,
        makeRunEvent(i, {
          runId: i,
          displayTitle: `Queued #${i}`,
          createdAt: new Date(2026, 0, 1, 12, 0, i).toISOString(),
          runStartedAt: null,
          updatedAt: new Date(2026, 0, 1, 12, 0, i).toISOString(),
          action: { type: 'Requested' },
        }),
      )
    }
    await expect(page.locator('.run-card')).toHaveCount(3, { timeout: 5_000 })

    // Attach pool labels via direct store mutation — keeps the test self-contained
    // without having to construct valid JobEventEnvelope wire payloads.
    // filterRunsByPool() reads jobsByRunId via runStore.applyJobEvent(); we inject
    // minimal job records with the labels we need for the filter to match correctly.
    await page.evaluate(() => {
      const stores = window.__stores!
      // job 1n for run 1n → labels ['pool-a']
      stores.runStore!.applyJobEvent({
        runId: 1n,
        jobId: 10n,
        org: 'o',
        repo: 'r',
        name: 'j1',
        createdAt: '2026-01-01T12:00:00Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: ['pool-a'], steps: [] } },
      })
      // job 2n for run 2n → labels ['pool-b']
      stores.runStore!.applyJobEvent({
        runId: 2n,
        jobId: 20n,
        org: 'o',
        repo: 'r',
        name: 'j2',
        createdAt: '2026-01-01T12:00:00Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: ['pool-b'], steps: [] } },
      })
      // job 3n for run 3n → labels ['pool-a']
      stores.runStore!.applyJobEvent({
        runId: 3n,
        jobId: 30n,
        org: 'o',
        repo: 'r',
        name: 'j3',
        createdAt: '2026-01-01T12:00:00Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: ['pool-a'], steps: [] } },
      })
    })
  }

  test('kanban-keyboard-nav — pool-filter: ArrowDown skips hidden cards, stays within visible set', async ({
    page,
  }) => {
    await seedWithPools(page)

    // Activate pool-a filter — run 2 (pool-b) becomes hidden.
    await page.evaluate(() => {
      // biome-ignore lint/suspicious/noExplicitAny: bypass PoolKey brand for test setter
      window.__stores!.uiStore!.activePoolFilter = window.__stores!.poolKey!(['pool-a']) as any
    })
    await expect(page.locator('.run-card[data-run-id="2"]')).toBeHidden({ timeout: 3_000 })

    // Focus the first visible card (run 1 — pool-a matches).
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    // ArrowDown: should skip run 2 (filtered) and land on run 3 (pool-a, visible).
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')

    // ArrowDown again: run 3 is the last visible card — no-op (no wrap).
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')

    // Clear filter: all runs visible again, nav continues normally.
    await page.evaluate(() => {
      window.__stores!.uiStore!.activePoolFilter = null
    })
    await expect(page.locator('.run-card[data-run-id="2"]')).toBeVisible({ timeout: 3_000 })

    // ArrowDown from run 3 is still at bottom of queued column — no-op.
    await page.keyboard.press('ArrowDown')
    expect(await focusedRunId(page)).toBe('3')
  })
})
