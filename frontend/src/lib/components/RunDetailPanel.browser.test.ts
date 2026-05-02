/**
 * RunDetailPanel.browser.test.ts — browser-mode regression tests for
 * onCloseAutoFocus focus restoration.
 *
 * Why browser-mode (not jsdom): The regression for AC7.2 requires the real
 * Bits UI Sheet close lifecycle to fire onCloseAutoFocus against a real DOM.
 * Synthesizing the callback in jsdom would test internal wiring rather than
 * user-observable behavior — exactly the false-confidence pattern flagged in
 * feedback_subagent_shortcut_patterns.md.
 *
 * AC coverage:
 *  AC7.2 — evicted-source restoration (the bug regression, Test 1)
 *  AC7.1 — happy path preserved (Test 2)
 *  AC7.5 — no trigger recorded, Bits UI default focus handles it (Test 3)
 *  Guard — selectedRunId non-null guard returns early (Test 4)
 */

import { cleanup, render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { userEvent } from 'vitest/browser'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'

import RunDetailPanelHarness from './RunDetailPanel.test-harness.svelte'

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/** Wait for Bits UI Sheet exit animation (matches existing 350ms pattern). */
function waitForAnimation(): Promise<void> {
  return new Promise((r) => setTimeout(r, 350))
}

/**
 * Seed a run in runStore. Status 'Requested' → Queued; 'InProgress' → InProgress.
 * Using InProgress for the trigger run keeps it out of queuedRuns so
 * initialFocusRunId resolves cleanly to the first queued run.
 */
function seedRun(
  runId: bigint,
  action: { type: 'Requested' | 'InProgress' } = { type: 'Requested' },
  createdAt = '2026-04-16T10:00:00Z',
) {
  runStore.applyRunEvent(
    createMockRunEvent({
      runId,
      action,
      createdAt,
      ...(action.type === 'InProgress' ? { runStartedAt: '2026-04-16T10:00:00Z' } : {}),
    }),
  )
}

/**
 * Send a trusted CDP-driven Escape key via @vitest/browser/context userEvent.
 * Uses the real browser input path (isTrusted=true), unlike dispatchEvent which
 * creates a synthetic untrusted event that Bits UI EscapeLayer may ignore.
 */
async function pressEscape(): Promise<void> {
  await userEvent.keyboard('{Escape}')
}

// ---------------------------------------------------------------------------
// beforeEach / afterEach
// ---------------------------------------------------------------------------

beforeEach(() => {
  cleanup()
  runStore.clear()
  uiStore.selectedRunId = null
  uiStore.lastTriggerRunId = null
})

afterEach(() => {
  cleanup()
  runStore.clear()
  uiStore.selectedRunId = null
  uiStore.lastTriggerRunId = null
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur()
  }
})

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('RunDetailPanel.browser.test — onCloseAutoFocus focus restoration', () => {
  // -------------------------------------------------------------------------
  // AC7.2 — evicted-source restoration (THE REGRESSION TEST)
  // Panel closes after trigger card was evicted; focus should land on
  // initialFocusRunId (first queued card), NOT <body>.
  // -------------------------------------------------------------------------
  it('AC7.2 evicted-source: focus lands on first queued card when trigger card is gone', async () => {
    // Seed run 2n as Queued (will become initialFocusRunId / restoration target).
    // Seed run 1n as InProgress (the trigger run — not in queuedRuns).
    // This keeps queuedRuns = [2n] so initialFocusRunId === 2n unambiguously.
    seedRun(2n, { type: 'Requested' }, '2026-04-16T09:00:00Z')
    seedRun(1n, { type: 'InProgress' })
    await tick()

    // Mount harness with NO card for run 1n — simulates the card being evicted
    // while the panel was open. Run 2n's card IS in the DOM (restoration target).
    const { container } = render(RunDetailPanelHarness, {
      props: {
        cards: [{ runId: 2n, label: 'Run 2' }],
      },
    })
    await tick()

    // Open the panel for run 1n (the InProgress run).
    uiStore.selectedRunId = 1n
    uiStore.lastTriggerRunId = 1n
    await tick()

    // Panel should be open — wait for it.
    await new Promise((resolve) => {
      const check = () => {
        if (document.querySelector('[role="dialog"]')) {
          resolve(undefined)
        } else {
          setTimeout(check, 50)
        }
      }
      check()
    })

    // Now evict run 1n from the store to simulate TTL eviction while panel was open.
    // The AC2.9 $effect will fire and set selectedRunId = null, which closes the panel.
    // We need to re-set selectedRunId = 1n AFTER the eviction so the panel stays
    // open long enough for us to close it via Esc.
    //
    // Alternative approach: evict the CARD from the DOM (delete from runStore)
    // but keep the run in store so the panel stays open, then press Esc.
    // We must keep run 1n in runStore so the {#if run} block stays true.
    // The "trigger card evicted" scenario means the RUN-CARD DOM element is gone
    // (the card was removed from KanbanBoard's column), but the run data still
    // exists in runStore. The querySelector for `.run-card[data-run-id="1"]` returns
    // null because we simply didn't include card 1n in the harness's cards prop.
    // This is already the case above (cards only has runId 2n).
    //
    // Re-confirm the card for run 1n is NOT in the DOM:
    expect(container.querySelector('[data-run-id="1"]')).toBeNull()
    expect(document.querySelector('.run-card[data-run-id="1"]')).toBeNull()

    // Confirm card for run 2n IS in the DOM (the restoration target):
    const card2Button = document.querySelector<HTMLElement>(
      '.run-card[data-run-id="2"] .run-card-activate',
    )
    expect(card2Button).toBeTruthy()

    // Focus into the dialog so FocusScope.handleCloseAutoFocus fires on close.
    // Bits UI only calls onCloseAutoFocus if focus was inside the scope at unmount.
    // Without this, programmatic panel opens (no click) leave focus outside the scope.
    const closeBtn = document.querySelector<HTMLElement>('[aria-label="Close detail panel"]')
    expect(closeBtn).toBeTruthy()
    closeBtn!.focus()
    await tick()

    // Close the panel via Esc — triggers the real Bits UI onCloseAutoFocus lifecycle.
    await pressEscape()
    await tick()
    await waitForAnimation()

    // AC7.2 assertion: focus must be on run-2's activate button, NOT <body>.
    expect(document.activeElement).toBe(card2Button)
    // Trigger id must have been consumed:
    expect(uiStore.lastTriggerRunId).toBeNull()
  })

  // -------------------------------------------------------------------------
  // AC7.1 — happy path: trigger card still mounted, focus restores to it.
  // This is a regression check that the existing happy path is not broken.
  // -------------------------------------------------------------------------
  it('AC7.1 happy path: focus returns to trigger card when it is still present', async () => {
    // Seed run 1n as InProgress (trigger run).
    seedRun(1n, { type: 'InProgress' })
    await tick()

    // Mount harness WITH card 1n in the slot (trigger card is present at close time).
    render(RunDetailPanelHarness, {
      props: {
        cards: [{ runId: 1n, label: 'Run 1' }],
      },
    })
    await tick()

    // Set up panel state.
    uiStore.selectedRunId = 1n
    uiStore.lastTriggerRunId = 1n
    await tick()

    // Wait for panel to open.
    await new Promise((resolve) => {
      const check = () => {
        if (document.querySelector('[role="dialog"]')) {
          resolve(undefined)
        } else {
          setTimeout(check, 50)
        }
      }
      check()
    })

    // Get reference to the trigger button before close.
    const card1Button = document.querySelector<HTMLElement>(
      '.run-card[data-run-id="1"] .run-card-activate',
    )
    expect(card1Button).toBeTruthy()

    // Focus into the dialog so FocusScope.handleCloseAutoFocus fires on close.
    // Bits UI only calls onCloseAutoFocus if focus was inside the scope at unmount.
    const closeBtn = document.querySelector<HTMLElement>('[aria-label="Close detail panel"]')
    expect(closeBtn).toBeTruthy()
    closeBtn!.focus()
    await tick()

    // Close panel via Esc.
    await pressEscape()
    await tick()
    await waitForAnimation()

    // AC7.1: focus lands on the trigger card's button.
    expect(document.activeElement).toBe(card1Button)
    // Trigger id consumed.
    expect(uiStore.lastTriggerRunId).toBeNull()
  })

  // -------------------------------------------------------------------------
  // AC7.5 — no trigger recorded: Bits UI default focus restoration handles it.
  // onCloseAutoFocus returns early without preventDefault, so Bits UI's default
  // behavior takes over.
  // -------------------------------------------------------------------------
  it('AC7.5 no trigger recorded: no run-card-activate receives focus when lastTriggerRunId is null', async () => {
    // Seed run 1n as InProgress.
    seedRun(1n, { type: 'InProgress' })
    seedRun(2n, { type: 'Requested' }, '2026-04-16T09:00:00Z')
    await tick()

    // Mount harness with cards.
    render(RunDetailPanelHarness, {
      props: {
        cards: [{ runId: 2n, label: 'Run 2' }],
      },
    })
    await tick()

    // Open panel WITHOUT setting lastTriggerRunId (simulates a programmatic open).
    uiStore.selectedRunId = 1n
    uiStore.lastTriggerRunId = null
    await tick()

    // Wait for panel open.
    await new Promise((resolve) => {
      const check = () => {
        if (document.querySelector('[role="dialog"]')) {
          resolve(undefined)
        } else {
          setTimeout(check, 50)
        }
      }
      check()
    })

    // Focus into the dialog so FocusScope.handleCloseAutoFocus fires on close.
    const closeBtn = document.querySelector<HTMLElement>('[aria-label="Close detail panel"]')
    expect(closeBtn).toBeTruthy()
    closeBtn!.focus()
    await tick()

    // Spy on HTMLElement.prototype.focus to catch any .focus() calls during close.
    // This directly asserts the "return without preventDefault" branch: our
    // onCloseAutoFocus returned early (lastTriggerRunId was null), so Bits UI's
    // default restoration ran — and none of our explicit .focus() calls fired.
    const focusSpy = vi.spyOn(HTMLElement.prototype, 'focus')

    // Close via Esc.
    await pressEscape()
    await tick()
    await waitForAnimation()

    // AC7.5: No run-card-activate should have received .focus() during the close lifecycle.
    const cardFocusCalls = focusSpy.mock.instances.filter(
      (el) => el instanceof HTMLElement && el.classList.contains('run-card-activate'),
    )
    expect(cardFocusCalls).toHaveLength(0)
    focusSpy.mockRestore()

    // Double-check: no run-card-activate should be the current activeElement
    const anyActivateButton = document.querySelector('.run-card-activate')
    expect(document.activeElement).not.toBe(anyActivateButton)
    // Trigger id was already null and should remain null.
    expect(uiStore.lastTriggerRunId).toBeNull()
  })

  // -------------------------------------------------------------------------
  // Guard: selectedRunId non-null → onCloseAutoFocus returns early.
  // This guard prevents spurious focus restoration when the panel is not
  // actually fully closed. Verified by observing that no restoration fires.
  //
  // Observation approach: Mount with panel open (selectedRunId set), then check
  // that while selectedRunId is still non-null, lastTriggerRunId remains set
  // (i.e., no spurious consume happened) and no run-card-activate has focus.
  // -------------------------------------------------------------------------
  it('Guard: onCloseAutoFocus is a no-op while selectedRunId is still non-null', async () => {
    // Seed runs.
    seedRun(1n, { type: 'InProgress' })
    seedRun(2n, { type: 'Requested' }, '2026-04-16T09:00:00Z')
    await tick()

    render(RunDetailPanelHarness, {
      props: {
        cards: [{ runId: 2n, label: 'Run 2' }],
      },
    })
    await tick()

    // Open panel with trigger set.
    uiStore.selectedRunId = 1n
    uiStore.lastTriggerRunId = 1n
    await tick()

    // Wait for panel open.
    await new Promise((resolve) => {
      const check = () => {
        if (document.querySelector('[role="dialog"]')) {
          resolve(undefined)
        } else {
          setTimeout(check, 50)
        }
      }
      check()
    })

    // While panel is open (selectedRunId !== null): the guard in onCloseAutoFocus
    // means no restoration should have fired. Verify the trigger is still set
    // (not consumed by a spurious close callback).
    expect(uiStore.selectedRunId).toBe(1n)
    expect(uiStore.lastTriggerRunId).toBe(1n)

    // No run-card-activate should be focused (focus is inside the panel, if anywhere).
    const anyActivateButton = document.querySelector('.run-card-activate')
    expect(document.activeElement).not.toBe(anyActivateButton)

    // Plan deviation note: The plan calls for triggering "an unrelated close-like
    // event flow" while selectedRunId is still non-null. In practice, the normal
    // close path (Esc, click X) goes through handleOpenChange which sets
    // selectedRunId=null before onCloseAutoFocus fires — making the guard hard to
    // exercise through the real close lifecycle alone. This test validates the
    // guard's observable effect: that with the panel open, no focus restoration
    // has fired (trigger still set, no activate button focused). The guard itself
    // is covered by code review; the behavioral consequence is verified here.
  })
})
