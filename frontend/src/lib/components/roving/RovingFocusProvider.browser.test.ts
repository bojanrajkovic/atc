import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { runStore } from '$lib/stores/runs.svelte'
import { createMockRun, createMockRunEvent } from '$lib/test-utils/factories'
import type { RovingFocusContext } from './context'

// ---------------------------------------------------------------------------
// afterEach: tear down shared store and focus state
// ---------------------------------------------------------------------------

afterEach(() => {
  runStore.clear()
  // Reset focus to body so tests don't bleed into each other
  if (document.activeElement instanceof HTMLElement) {
    document.activeElement.blur()
  }
})

// ---------------------------------------------------------------------------
// Helper: mount the harness and await ctx delivery
// ---------------------------------------------------------------------------

async function mountHarness(runs: ReturnType<typeof createMockRun>[] = []) {
  // Dynamic import to avoid Svelte 5 module-init ordering issues in tests
  const { default: Harness } = await import('./RovingFocusProvider.test-harness.svelte')

  let capturedCtx: RovingFocusContext | undefined
  const onCtxReady = vi.fn((ctx: RovingFocusContext) => {
    capturedCtx = ctx
  })

  const result = render(Harness, {
    props: { runs, onCtxReady },
  })

  // Allow the $effect in RovingHarnessGrid to fire
  await tick()

  if (capturedCtx === undefined) {
    throw new Error('onCtxReady was never called — RovingHarnessGrid did not mount')
  }

  return { ...result, ctx: capturedCtx }
}

// ---------------------------------------------------------------------------
// Test 1: kanbanHasFocus toggles via focusin/focusout
// ---------------------------------------------------------------------------

describe('RovingFocusProvider.browser.test', () => {
  it('kanbanHasFocus toggles true on focusin and false on focusout', async () => {
    // Populate the store with one queued run so the harness renders a button
    const run = createMockRun({ id: 1n, status: 'Queued' })
    runStore.applyRunEvent(createMockRunEvent({ runId: 1n, action: { type: 'Requested' } }))
    await tick()

    const { container, ctx } = await mountHarness([run])
    await tick()

    // Initial state: no focus in kanban
    expect(ctx.kanbanHasFocus).toBe(false)

    // Find the inner button and focus it — triggers focusin on the grid
    const innerButton = container.querySelector<HTMLButtonElement>('button.run-card-activate')
    expect(innerButton).toBeTruthy()
    innerButton!.focus()
    await tick()

    expect(ctx.kanbanHasFocus).toBe(true)

    // Create a button outside the kanban and focus it — triggers focusout
    const outsideButton = document.createElement('button')
    outsideButton.textContent = 'outside'
    document.body.appendChild(outsideButton)
    outsideButton.focus()
    await tick()

    expect(ctx.kanbanHasFocus).toBe(false)

    // Cleanup
    outsideButton.remove()
  })

  // ---------------------------------------------------------------------------
  // Test 2: currentFocusRunId falls back to initialFocusRunId when focusedRunId is null
  // ---------------------------------------------------------------------------

  it('currentFocusRunId falls back to initialFocusRunId when focusedRunId is null', async () => {
    // Populate store with three queued runs
    const runs = [
      createMockRun({ id: 10n, status: 'Queued', createdAt: '2026-01-01T00:00:00Z' }),
      createMockRun({ id: 20n, status: 'Queued', createdAt: '2026-01-02T00:00:00Z' }),
      createMockRun({ id: 30n, status: 'Queued', createdAt: '2026-01-03T00:00:00Z' }),
    ]
    for (const run of runs) {
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: run.id,
          action: { type: 'Requested' },
          createdAt: run.createdAt,
        }),
      )
    }
    await tick()

    const { ctx } = await mountHarness(runs)
    await tick()

    // focusedRunId is null by default, so currentFocusRunId should fall back
    expect(ctx.focusedRunId).toBe(null)
    // initialFocusRunId is the first queued run (sorted by createdAt asc: 10n)
    expect(ctx.initialFocusRunId).toBe(10n)
    expect(ctx.currentFocusRunId).toBe(10n)
  })

  // ---------------------------------------------------------------------------
  // Test 3: currentFocusRunId follows focusedRunId when set
  // ---------------------------------------------------------------------------

  it('currentFocusRunId follows focusedRunId when set via setFocus', async () => {
    const runs = [
      createMockRun({ id: 10n, status: 'Queued', createdAt: '2026-01-01T00:00:00Z' }),
      createMockRun({ id: 20n, status: 'Queued', createdAt: '2026-01-02T00:00:00Z' }),
      createMockRun({ id: 30n, status: 'Queued', createdAt: '2026-01-03T00:00:00Z' }),
    ]
    for (const run of runs) {
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: run.id,
          action: { type: 'Requested' },
          createdAt: run.createdAt,
        }),
      )
    }
    await tick()

    const { ctx } = await mountHarness(runs)
    await tick()

    // Set focus to the third run
    ctx.setFocus(30n)
    await tick()

    expect(ctx.focusedRunId).toBe(30n)
    expect(ctx.currentFocusRunId).toBe(30n)

    // Set focus to second run
    ctx.setFocus(20n)
    await tick()

    expect(ctx.focusedRunId).toBe(20n)
    expect(ctx.currentFocusRunId).toBe(20n)
  })

  // ---------------------------------------------------------------------------
  // Test 4: eviction $effect triggers restoreFocusToInitial
  // ---------------------------------------------------------------------------

  it('eviction $effect triggers restoreFocusToInitial when focused run is deleted from store', async () => {
    const runs = [
      createMockRun({ id: 100n, status: 'Queued', createdAt: '2026-01-01T00:00:00Z' }),
      createMockRun({ id: 200n, status: 'Queued', createdAt: '2026-01-02T00:00:00Z' }),
    ]
    for (const run of runs) {
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: run.id,
          action: { type: 'Requested' },
          createdAt: run.createdAt,
        }),
      )
    }
    await tick()

    const { container, ctx } = await mountHarness(runs)
    await tick()

    // Focus on the second run (id 200n)
    ctx.setFocus(200n)
    await tick()
    expect(ctx.focusedRunId).toBe(200n)
    expect(ctx.currentFocusRunId).toBe(200n)

    // Evict run 200n from the store — should trigger restoreFocusToInitial
    runStore.runs.delete(200n)

    // Wait for the $derived arrays and the eviction $effect to propagate,
    // then for restoreFocusToInitial to call el.focus(). After focus lands on
    // the 100n button, the action's focusin handler fires and sets focusedRunId
    // back to 100n. So we assert currentFocusRunId (the effective focus) rather
    // than focusedRunId (which will be 100n, not null, by the time we can observe).
    const expectedButton = container.querySelector<HTMLButtonElement>(
      '.run-card[data-run-id="100"] .run-card-activate',
    )
    expect(expectedButton).toBeTruthy()

    await vi.waitFor(
      () => {
        expect(document.activeElement).toBe(expectedButton)
      },
      { timeout: 2000 },
    )

    // After focus lands on 100n's button, currentFocusRunId must be 100n.
    expect(ctx.currentFocusRunId).toBe(100n)
    // initialFocusRunId after eviction is run 100n (first remaining queued run)
    expect(ctx.initialFocusRunId).toBe(100n)
  })

  // ---------------------------------------------------------------------------
  // Test 5: all-empty fallback — no throw, no weird focus
  // ---------------------------------------------------------------------------

  it('restoreFocusToInitial does not throw and does not focus body when all columns empty', async () => {
    // No runs in store, no runs passed to harness
    const { ctx } = await mountHarness([])
    await tick()

    expect(ctx.initialFocusRunId).toBe(null)
    expect(ctx.currentFocusRunId).toBe(null)

    // Calling restoreFocusToInitial on empty store should be a no-op
    await expect(ctx.restoreFocusToInitial()).resolves.toBeUndefined()
    await tick()

    // body should remain the active element (no unexpected focus side-effect)
    expect(document.activeElement).toBe(document.body)
  })

  // ---------------------------------------------------------------------------
  // Test 6: children snippet renders — no DOM wrapper from provider
  // ---------------------------------------------------------------------------

  it('renders children directly without a DOM wrapper element from the provider', async () => {
    const run = createMockRun({ id: 1n, status: 'Queued' })
    runStore.applyRunEvent(createMockRunEvent({ runId: 1n, action: { type: 'Requested' } }))
    await tick()

    const { container } = await mountHarness([run])
    await tick()

    // The provider produces no DOM of its own — the first child of container
    // must be the grid div from RovingHarnessGrid, not a provider wrapper.
    const firstChild = container.firstElementChild
    expect(firstChild).not.toBeNull()
    expect(firstChild?.getAttribute('data-testid')).toBe('grid')

    // Verify grid children (the run card article) are also present
    const grid = container.querySelector('[data-testid="grid"]')
    expect(grid).toBeTruthy()
    expect(grid?.querySelector('.run-card')).toBeTruthy()
  })
})
