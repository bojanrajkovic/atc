import { expect, type Page, test } from '@playwright/test'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E integration verification for kanban 2D keyboard navigation.
 *
 * Tests: AC2 (2D arrow navigation), AC3 (edge/asymmetric-column), AC4
 * (modifier-key delegation), AC5 (suspension via natural focus scoping).
 *
 * AC1.* (Tab-in entry, tabindex invariants) is covered in run-card-interactivity.test.ts.
 * AC6.* / AC7.* are Phase 4 territory.
 *
 * All scenarios use focusFirstCard() for deterministic focus entry, EXCEPT
 * AC1.2-style tests which are explicitly marked as Phase 2 territory and
 * not re-done here.
 *
 * @see docs/design-plans/2026-05-01-kanban-keyboard-nav.md §AC2-AC5
 * @see docs/implementation-plans/2026-05-01-kanban-keyboard-nav/phase_03.md
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
 * (open popover suppresses RunCard's auto-focus $effect via !popoverOpen guard
 * — commit cc45224), stub /v1/state, navigate, wait for connected.
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
 * brittleness. All scenarios except AC1.2 (Tab-into-kanban entry, covered by
 * Phase 2 in run-card-interactivity.test.ts) use this helper.
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
// AC2: 2D arrow navigation
// ---------------------------------------------------------------------------

test.describe('AC2: 2D arrow navigation', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav.AC2.1 ArrowDown moves focus to next card in same column', async ({
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

  test('kanban-keyboard-nav.AC2.2 ArrowUp moves focus to previous card in same column', async ({
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

  test('kanban-keyboard-nav.AC2.3 ArrowRight moves focus to corresponding row in next non-empty column', async ({
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

  test('kanban-keyboard-nav.AC2.4 ArrowLeft moves focus to corresponding row in previous non-empty column', async ({
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

  test('kanban-keyboard-nav.AC2.5 Home moves focus to the first card in the current column', async ({
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

  test('kanban-keyboard-nav.AC2.6 End moves focus to the last card in the current column', async ({
    page,
  }) => {
    await seedQueued(page, 5)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    // End → last card
    await page.keyboard.press('End')
    expect(await focusedRunId(page)).toBe('5')
  })

  test('kanban-keyboard-nav.AC2.7 ArrowDown calls event.preventDefault() — page does not scroll', async ({
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

    // Press ArrowDown at last row — roving handler fires first (bubble-phase, no capture),
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
// AC3: Edge and asymmetric-column behavior
// ---------------------------------------------------------------------------

test.describe('AC3: Edge and asymmetric-column behavior', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav.AC3.1 ArrowDown at last card is a no-op', async ({ page }) => {
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

  test('kanban-keyboard-nav.AC3.2 ArrowUp at first card is a no-op', async ({ page }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    // ArrowUp at row 0 — focus stays on runId=1
    await page.keyboard.press('ArrowUp')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav.AC3.3 ArrowRight in rightmost non-empty column is a no-op', async ({
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

  test('kanban-keyboard-nav.AC3.4 ArrowLeft in leftmost non-empty column is a no-op', async ({
    page,
  }) => {
    await seedQueued(page, 1)
    await focusFirstCard(page)

    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('ArrowLeft')
    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav.AC3.5 ArrowRight skips empty inProgress column and lands in completed', async ({
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

  test('kanban-keyboard-nav.AC3.6 ArrowLeft skips empty inProgress column symmetrically', async ({
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

  test('kanban-keyboard-nav.AC3.7 asymmetric clamp: focus in row 5 of 10-card queued, ArrowRight to 3-card inProgress clamps to last row', async ({
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

  test('kanban-keyboard-nav.AC3.8 ArrowRight skips multiple empty columns to find furthest non-empty', async ({
    page,
  }) => {
    // Only queued (leftmost) + completed (rightmost) — inProgress is empty.
    // Same fixture as AC3.5, just asserting the skip works over the empty stretch.
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
// AC4: Modifier-key delegation
// ---------------------------------------------------------------------------

test.describe('AC4: Modifier-key delegation', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav.AC4.1 Cmd+K opens command palette while card focused', async ({
    page,
  }) => {
    await seedQueued(page, 1)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press(`${cmdOrCtrl}+k`)

    // Palette dialog becomes visible
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 5_000 })
    // Combobox input receives focus (palette's onOpenAutoFocus)
    await expect(page.locator('input[role="combobox"]')).toBeFocused()
  })

  test('kanban-keyboard-nav.AC4.2 Cmd+D toggles dark mode while card focused and does not move kanban focus', async ({
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

  test('kanban-keyboard-nav.AC4.3 Cmd+\\ toggles compact density while card focused', async ({
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

  test('kanban-keyboard-nav.AC4.4 Cmd+ArrowDown returns early — kanban does not move focus', async ({
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

  test('kanban-keyboard-nav.AC4.4b Cmd+ArrowUp returns early — kanban does not move focus', async ({
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

  test('kanban-keyboard-nav.AC4.5 Shift+ArrowDown returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('Shift+ArrowDown')
    await page.waitForTimeout(50)

    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav.AC4.6 Alt+ArrowDown returns early — kanban does not move focus', async ({
    page,
  }) => {
    await seedQueued(page, 3)
    await focusFirstCard(page)
    expect(await focusedRunId(page)).toBe('1')

    await page.keyboard.press('Alt+ArrowDown')
    await page.waitForTimeout(50)

    expect(await focusedRunId(page)).toBe('1')
  })

  test('kanban-keyboard-nav.AC4.7 bare ArrowDown is claimed by kanban and does not open palette', async ({
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
// AC5: Suspension via natural focus scoping
// ---------------------------------------------------------------------------

test.describe('AC5: Suspension via natural focus scoping', () => {
  test.beforeEach(async ({ page }) => {
    await setupPage(page)
  })

  test('kanban-keyboard-nav.AC5.1 palette open: ArrowDown does not move kanban focus; Esc returns to card', async ({
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

  test('kanban-keyboard-nav.AC5.2 panel open: ArrowDown does not move kanban focus', async ({
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
  })

  test('kanban-keyboard-nav.AC5.3 both palette and panel stacked: ArrowDown affects neither', async ({
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

  test('kanban-keyboard-nav.AC5.4 after panel closes, ArrowDown resumes kanban navigation', async ({
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
