import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { poolKey } from '$lib/filters/pool'
import type { JobStats } from '$lib/stores/runs.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
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
  const { default: Harness } = await import('./test-utils/RovingFocusProvider.test-harness.svelte')

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

    // Focus on the second run's button to set kanbanHasFocus=true via the
    // roving action's focusin listener. Without kanbanHasFocus=true, the
    // eviction $effect correctly resets state without touching DOM focus
    // (Task 2 fix: background eviction must not steal focus from palette/panel).
    const btn200 = container.querySelector<HTMLButtonElement>(
      '.run-card[data-run-id="200"] .run-card-activate',
    )
    expect(btn200).toBeTruthy()
    btn200!.focus()
    await tick()
    expect(ctx.kanbanHasFocus).toBe(true)
    expect(ctx.focusedRunId).toBe(200n)
    expect(ctx.currentFocusRunId).toBe(200n)

    // Evict run 200n from the store — should trigger restoreFocusToInitial
    // because kanbanHasFocus is true.
    runStore.runs.delete(200n)

    // Plan adaptation: the implementation plan calls for `expect(focusedRunId).toBe(null)`,
    // but that state is unobservable — restoreFocusToInitial sets focusedRunId=null, awaits
    // tick, then calls el.focus(); the action's focusin handler synchronously re-syncs
    // focusedRunId back to the target id (100n) before any poll can observe null. We assert
    // the user-observable end-state instead: focus landed on the right button, and
    // currentFocusRunId reflects the effective focus.
    //
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

// ---------------------------------------------------------------------------
// Pool-filter arrow nav (Task 3b RED gate)
// ---------------------------------------------------------------------------

describe('RovingFocusProvider.browser.test — pool-filter arrow nav', () => {
  const emptyStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

  function statsMapFor(ids: bigint[]): Map<bigint, JobStats> {
    const m = new Map<bigint, JobStats>()
    for (const id of ids) m.set(id, emptyStats)
    return m
  }

  afterEach(() => {
    runStore.clear()
    uiStore.activePoolFilter = null
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  })

  it('arrow nav respects activePoolFilter — ArrowDown skips filter-hidden cards', async () => {
    // Seed three queued runs: run 1n (pool A), run 2n (pool B), run 3n (pool A).
    // With filter = poolKey(['A']), only runs 1n and 3n are visible in the DOM.
    // The bug: action.ts reads raw runStore.queuedRuns → [1n, 2n, 3n], so ArrowDown
    // from run 1n resolves to run 2n (hidden). After the fix, geometry uses
    // visibleColumns → [1n, 3n], so ArrowDown from run 1n correctly lands on run 3n.

    for (const [id, ts] of [
      [1n, '2026-01-01T00:00:01Z'],
      [2n, '2026-01-01T00:00:02Z'],
      [3n, '2026-01-01T00:00:03Z'],
    ] as [bigint, string][]) {
      runStore.applyRunEvent(
        createMockRunEvent({ runId: id, action: { type: 'Requested' }, createdAt: ts }),
      )
    }
    // Assign jobs with labels so filterRunsByPool works
    runStore.applyJobEvent({
      runId: 1n,
      jobId: 10n,
      org: 'o',
      repo: 'r',
      name: 'j1',
      createdAt: '2026-01-01T00:00:01Z',
      startedAt: null,
      completedAt: null,
      action: { type: 'Queued', data: { labels: ['A'], steps: [] } },
    })
    runStore.applyJobEvent({
      runId: 2n,
      jobId: 20n,
      org: 'o',
      repo: 'r',
      name: 'j2',
      createdAt: '2026-01-01T00:00:02Z',
      startedAt: null,
      completedAt: null,
      action: { type: 'Queued', data: { labels: ['B'], steps: [] } },
    })
    runStore.applyJobEvent({
      runId: 3n,
      jobId: 30n,
      org: 'o',
      repo: 'r',
      name: 'j3',
      createdAt: '2026-01-01T00:00:03Z',
      startedAt: null,
      completedAt: null,
      action: { type: 'Queued', data: { labels: ['A'], steps: [] } },
    })
    await tick()

    // Set the pool filter to 'A' — only runs 1n and 3n are visible
    uiStore.activePoolFilter = poolKey(['A'])
    await tick()

    // Dynamic import to avoid ordering issues
    const { default: Harness } = await import(
      '../test-utils/KanbanBoardInvariant.test-harness.svelte'
    )

    let capturedCtx: RovingFocusContext | undefined
    const { container } = render(Harness, {
      props: {
        queuedRuns: runStore.queuedRuns,
        inProgressRuns: [],
        completedRuns: [],
        jobStatsByRun: statsMapFor([1n, 2n, 3n]),
        jobsByRunId: runStore.jobsByRunId,
        activePoolFilter: uiStore.activePoolFilter,
        onCtxReady: (ctx: RovingFocusContext) => {
          capturedCtx = ctx
        },
      },
    })
    await tick()

    // Verify the DOM has only runs 1n and 3n (run 2n is filter-hidden)
    expect(container.querySelector('[data-run-id="2"]')).toBeNull()
    expect(container.querySelector('[data-run-id="1"]')).not.toBeNull()
    expect(container.querySelector('[data-run-id="3"]')).not.toBeNull()

    // Programmatically focus run 1n's button to start navigation
    const card1Button = container.querySelector<HTMLElement>(
      '.run-card[data-run-id="1"] .run-card-activate',
    )
    expect(card1Button).toBeTruthy()
    card1Button!.focus()
    await tick()

    // capturedCtx should now see run 1n focused
    expect(capturedCtx?.kanbanHasFocus).toBe(true)

    // Dispatch ArrowDown on the kanban grid — roving action handles it.
    // With the bug: geometry uses raw [1n, 2n, 3n] → resolves to run 2n (hidden).
    // After fix: geometry uses visible [1n, 3n] → resolves to run 3n (correct).
    const grid = container.querySelector<HTMLElement>('[data-testid="kanban-grid"]')
    expect(grid).toBeTruthy()
    grid!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }))
    await tick()

    // Wait for RunCard's $effect to call .focus() on the resolved card
    const card3Button = container.querySelector<HTMLElement>(
      '.run-card[data-run-id="3"] .run-card-activate',
    )
    expect(card3Button).toBeTruthy()

    await vi.waitFor(
      () => {
        expect(document.activeElement).toBe(card3Button)
      },
      { timeout: 2000 },
    )
  })
})

// ---------------------------------------------------------------------------
// Dual-path restoration: panel-close-evicted and keyboard-nav-eviction
// ---------------------------------------------------------------------------

describe('RovingFocusProvider.browser.test — dual-path restoration', () => {
  afterEach(() => {
    runStore.clear()
    uiStore.activePoolFilter = null
    uiStore.lastTriggerRunId = null
    if (document.activeElement instanceof HTMLElement) {
      document.activeElement.blur()
    }
  })

  it('panel-close-evicted path and keyboard-nav-eviction path land on the same DOM node', async () => {
    // Seed runs 1n, 2n, 3n as Queued (ascending createdAt so 1n = initialFocusRunId).
    // Run 2n will be evicted in both paths; the restoration target is run 1n.
    for (const [id, ts] of [
      [1n, '2026-01-01T00:00:01Z'],
      [2n, '2026-01-01T00:00:02Z'],
      [3n, '2026-01-01T00:00:03Z'],
    ] as [bigint, string][]) {
      runStore.applyRunEvent(
        createMockRunEvent({ runId: id, action: { type: 'Requested' }, createdAt: ts }),
      )
    }
    await tick()

    // -------------------------------------------------------------------------
    // Path A: keyboard-nav eviction
    // Focus run 2n explicitly, then delete run 2n from store.
    // The eviction $effect should restore focus to initialFocusRunId = run 1n.
    // -------------------------------------------------------------------------
    const { default: Harness } = await import(
      './test-utils/RovingFocusProvider.test-harness.svelte'
    )

    let capturedCtx: RovingFocusContext | undefined
    const { container } = render(Harness, {
      props: {
        runs: [runStore.runs.get(1n)!, runStore.runs.get(2n)!, runStore.runs.get(3n)!],
        onCtxReady: (ctx: RovingFocusContext) => {
          capturedCtx = ctx
        },
      },
    })
    await tick()

    if (capturedCtx === undefined) throw new Error('ctx not captured')

    // Focus run 2n — sets focusedRunId
    capturedCtx.setFocus(2n)
    // Also set kanbanHasFocus so the eviction $effect calls restoreFocusToInitial()
    capturedCtx.setKanbanHasFocus(true)
    await tick()
    expect(capturedCtx.focusedRunId).toBe(2n)

    // Evict run 2n
    runStore.runs.delete(2n)

    const card1Button = container.querySelector<HTMLElement>(
      '.run-card[data-run-id="1"] .run-card-activate',
    )
    expect(card1Button).toBeTruthy()

    await vi.waitFor(
      () => {
        expect(document.activeElement).toBe(card1Button)
      },
      { timeout: 2000 },
    )

    const pathANodeId = (document.activeElement as HTMLElement | null)
      ?.closest('.run-card')
      ?.getAttribute('data-run-id')

    // -------------------------------------------------------------------------
    // Path B: panel-close-evicted path via ctx.restoreFocusToInitial()
    // Simulate panel-close where trigger card is evicted: run 2n was the trigger,
    // but it's already evicted. restoreFocusToInitial() resolves to run 1n.
    // -------------------------------------------------------------------------
    // Restore run 2n so we can evict it cleanly again (re-seed)
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 2n,
        action: { type: 'Requested' },
        createdAt: '2026-01-01T00:00:02Z',
      }),
    )
    await tick()

    // Reset focus state
    capturedCtx.setFocus(null)
    capturedCtx.setKanbanHasFocus(false)
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
    await tick()

    // Evict run 2n again
    runStore.runs.delete(2n)
    await tick()

    // Simulate panel-close restoration: set lastTriggerRunId to evicted run 2n,
    // then call restoreFocusToInitial() directly (as RunDetailPanel.onCloseAutoFocus does)
    uiStore.lastTriggerRunId = 2n
    await capturedCtx.restoreFocusToInitial()
    await tick()

    const pathBNodeId = (document.activeElement as HTMLElement | null)
      ?.closest('.run-card')
      ?.getAttribute('data-run-id')

    // Both paths must land on the same node — run 1n (the new initialFocusRunId)
    expect(pathANodeId).toBe('1')
    expect(pathBNodeId).toBe('1')
    expect(pathANodeId).toBe(pathBNodeId)
  })
})
