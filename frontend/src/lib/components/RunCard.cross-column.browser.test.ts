/**
 * RunCard cross-column re-focus tests (AC6.1, AC6.2, AC6.3, AC6.4).
 *
 * These tests need the REAL RovingFocusProvider so that RunCard's $effect
 * (which calls buttonEl.focus() when isFocused && ctx.kanbanHasFocus &&
 * buttonEl !== undefined) is driven by live $state/$derived reactivity.
 *
 * They live in a separate file from RunCard.browser.test.ts because that file
 * applies a top-level vi.mock('$lib/components/roving/context') stub, which is
 * hoisted by Vitest and would poison the real RovingFocusProvider if this
 * describe block were appended there. This is the same environmental exception
 * that justified KanbanColumn.tabindex.browser.test.ts. The "do NOT split"
 * rule from feedback_no_split_ts_test_files.md applies to line-count splits;
 * this is a module-mock conflict split.
 */
import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { RovingFocusContext } from '$lib/components/roving/context'
import type { JobStats } from '$lib/stores/runs.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import KanbanBoardInvariantHarness from './KanbanBoardInvariant.test-harness.svelte'

import '../../app.css'

const emptyStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

function statsMapFor(runs: readonly WorkflowRun[]): Map<bigint, JobStats> {
  const m = new Map<bigint, JobStats>()
  for (const r of runs) m.set(r.id, emptyStats)
  return m
}

function emptyJobsByRunId(): Map<bigint, readonly Job[]> {
  return new Map()
}

/** Seed a queued run into runStore and return the WorkflowRun object. */
function addQueued(id: bigint, createdAt?: string): WorkflowRun {
  runStore.applyRunEvent(
    createMockRunEvent({
      runId: id,
      action: { type: 'Requested' },
      createdAt: createdAt ?? `2026-05-01T10:00:0${Number(id)}Z`,
    }),
  )
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
  return runStore.runs.get(id)!
}

/** Seed an in-progress run into runStore and return the WorkflowRun object. */
function addInProgress(id: bigint): WorkflowRun {
  runStore.applyRunEvent(
    createMockRunEvent({
      runId: id,
      action: { type: 'InProgress' },
      runStartedAt: `2026-05-01T10:00:0${Number(id)}Z`,
    }),
  )
  // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
  return runStore.runs.get(id)!
}

describe('roving cross-column re-focus (AC6)', () => {
  let capturedCtx: RovingFocusContext | undefined

  beforeEach(() => {
    runStore.clear()
    capturedCtx = undefined
  })

  afterEach(() => {
    runStore.clear()
  })

  it('AC6.1: in-column reorder preserves focus on same DOM node', async () => {
    // Seed 3 queued runs. createdAt timestamps determine sort order (ascending).
    addQueued(100n, '2026-05-01T10:00:01Z')
    addQueued(200n, '2026-05-01T10:00:02Z')
    addQueued(300n, '2026-05-01T10:00:03Z')

    const queued = [...runStore.queuedRuns]
    const inProgress: WorkflowRun[] = []
    const completed: WorkflowRun[] = []

    const { container, rerender } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queued,
        inProgressRuns: inProgress,
        completedRuns: completed,
        jobStatsByRun: statsMapFor(queued),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    // Wait for $effect (onCtxReady) to fire
    await tick()

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    const ctx = capturedCtx!

    // Capture card-200's DOM node before the reorder
    const card200Before = container.querySelector('[data-run-id="200"]')
    expect(card200Before).toBeTruthy()

    // Focus card-200 and enable kanbanHasFocus so the $effect will call .focus()
    ctx.setFocus(200n)
    ctx.setKanbanHasFocus(true)
    await tick()

    // Trigger in-column reorder by mutating card-200's createdAt to push it to the front.
    // applyRunEvent with 'Requested' action re-creates the run with an earlier createdAt.
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 200n,
        action: { type: 'Requested' },
        createdAt: '2026-05-01T09:00:00Z', // earlier → now first in sorted order
      }),
    )

    await tick()

    // Rerender harness with updated column arrays (runStore derived arrays are now reordered)
    await rerender({
      queuedRuns: [...runStore.queuedRuns],
      inProgressRuns: [],
      completedRuns: [],
      jobStatsByRun: statsMapFor([...runStore.queuedRuns]),
      jobsByRunId: emptyJobsByRunId(),
      onCtxReady: (ctx2) => {
        capturedCtx = ctx2
      },
    })

    // Wait for FLIP animation to settle (mirrors KanbanColumn.browser.test.ts:18-67 pattern)
    await new Promise((r) => setTimeout(r, 350))

    // AC6.1: same DOM node reference — FLIP reorders in place, does not remount
    const card200After = container.querySelector('[data-run-id="200"]')
    expect(card200After).toBeTruthy()
    expect(card200After).toBe(card200Before)

    // The inner button of card-200 must have focus
    const button200 = container.querySelector(
      'article[data-run-id="200"] .run-card-activate',
    ) as HTMLElement | null
    expect(button200).toBeTruthy()
    expect(document.activeElement).toBe(button200)
  })

  it('AC6.2: cross-column move lands focus on new DOM node in destination column', async () => {
    // Seed 3 queued + 2 in-progress
    addQueued(100n, '2026-05-01T10:00:01Z')
    addQueued(200n, '2026-05-01T10:00:02Z')
    addQueued(300n, '2026-05-01T10:00:03Z')
    addInProgress(400n)
    addInProgress(500n)

    const queuedBefore = [...runStore.queuedRuns]
    const inProgressBefore = [...runStore.inProgressRuns]

    // AC6.2 uses document.activeElement ancestor walk — container is not needed
    const { rerender } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queuedBefore,
        inProgressRuns: inProgressBefore,
        completedRuns: [],
        jobStatsByRun: statsMapFor([...queuedBefore, ...inProgressBefore]),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    await tick()

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    const ctx = capturedCtx!

    // Focus card-100 (queued) and activate kanbanHasFocus
    ctx.setFocus(100n)
    ctx.setKanbanHasFocus(true)
    await tick()

    // Transition run 100 from Queued → InProgress via applyRunEvent
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 100n,
        action: { type: 'InProgress' },
        runStartedAt: '2026-05-01T10:00:01Z',
      }),
    )

    await tick()

    // Rerender harness with updated column arrays (100n is now in inProgressRuns)
    await rerender({
      queuedRuns: [...runStore.queuedRuns],
      inProgressRuns: [...runStore.inProgressRuns],
      completedRuns: [],
      jobStatsByRun: statsMapFor([...runStore.queuedRuns, ...runStore.inProgressRuns]),
      jobsByRunId: emptyJobsByRunId(),
      onCtxReady: (ctx2) => {
        capturedCtx = ctx2
      },
    })

    // Wait for crossfade animation to settle
    await new Promise((r) => setTimeout(r, 350))

    // AC6.2: document.activeElement must be inside the article for run 100
    // (ancestor walk — coexistence during outgoing crossfade is acceptable)
    const activeArticle = document.activeElement?.closest('[data-run-id="100"]')
    expect(activeArticle).toBeTruthy()

    // The containing column must be the in-progress column
    const inProgressSection = activeArticle?.closest(
      'section[aria-labelledby="kanban-col-in-progress"]',
    )
    expect(inProgressSection).toBeTruthy()
  })

  it('AC6.3: old DOM node loses focus after cross-column move', async () => {
    // Seed runs
    addQueued(100n, '2026-05-01T10:00:01Z')
    addQueued(200n, '2026-05-01T10:00:02Z')
    addInProgress(300n)

    const queuedBefore = [...runStore.queuedRuns]
    const inProgressBefore = [...runStore.inProgressRuns]

    const { container, rerender } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queuedBefore,
        inProgressRuns: inProgressBefore,
        completedRuns: [],
        jobStatsByRun: statsMapFor([...queuedBefore, ...inProgressBefore]),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    await tick()

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    const ctx = capturedCtx!

    // Focus card-100 and enable kanbanHasFocus
    ctx.setFocus(100n)
    ctx.setKanbanHasFocus(true)
    await tick()

    // Capture the button node BEFORE the move — this is the "old" DOM node
    const card100ButtonBefore = container.querySelector(
      'article[data-run-id="100"] .run-card-activate',
    ) as HTMLElement | null
    expect(card100ButtonBefore).toBeTruthy()

    // Transition run 100: Queued → InProgress
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 100n,
        action: { type: 'InProgress' },
        runStartedAt: '2026-05-01T10:00:01Z',
      }),
    )

    await tick()

    await rerender({
      queuedRuns: [...runStore.queuedRuns],
      inProgressRuns: [...runStore.inProgressRuns],
      completedRuns: [],
      jobStatsByRun: statsMapFor([...runStore.queuedRuns, ...runStore.inProgressRuns]),
      jobsByRunId: emptyJobsByRunId(),
      onCtxReady: (ctx2) => {
        capturedCtx = ctx2
      },
    })

    // Wait for crossfade to settle
    await new Promise((r) => setTimeout(r, 350))

    // AC6.3: the old button is no longer document.activeElement
    // (the incoming crossfade node, which is a new DOM element, now has focus)
    // We do NOT require card100ButtonBefore to be unmounted — coexistence is fine.
    expect(document.activeElement).not.toBe(card100ButtonBefore)
  })

  it('AC6.4: kanbanHasFocus===false prevents focus migration on cross-column move', async () => {
    // Seed runs
    addQueued(100n, '2026-05-01T10:00:01Z')
    addQueued(200n, '2026-05-01T10:00:02Z')
    addInProgress(300n)

    const queuedBefore = [...runStore.queuedRuns]
    const inProgressBefore = [...runStore.inProgressRuns]

    const { rerender } = render(KanbanBoardInvariantHarness, {
      // Note: `container` intentionally not destructured — AC6.4 queries via document.querySelector
      props: {
        queuedRuns: queuedBefore,
        inProgressRuns: inProgressBefore,
        completedRuns: [],
        jobStatsByRun: statsMapFor([...queuedBefore, ...inProgressBefore]),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    await tick()

    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    const ctx = capturedCtx!

    // Set focusedRunId to 100n WITHOUT calling setKanbanHasFocus(true).
    // The $effect guard is: isFocused && ctx.kanbanHasFocus && buttonEl !== undefined.
    ctx.setFocus(100n)
    // Ensure any stray focus from mount is cleared before the pre-assert.
    document.activeElement instanceof HTMLElement && document.activeElement.blur()
    await tick()

    // AC6.4 three-step check:

    // STEP 1 — Pre-assert: kanbanHasFocus must be false before the move.
    // If the harness mount stole focus into a card and triggered setKanbanHasFocus(true)
    // via a focusin event, this fails fast with a clear cause.
    expect(ctx.kanbanHasFocus).toBe(false)

    // STEP 2 — Trigger cross-column move via applyRunEvent (public API)
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 100n,
        action: { type: 'InProgress' },
        runStartedAt: '2026-05-01T10:00:01Z',
      }),
    )

    await tick()

    await rerender({
      queuedRuns: [...runStore.queuedRuns],
      inProgressRuns: [...runStore.inProgressRuns],
      completedRuns: [],
      jobStatsByRun: statsMapFor([...runStore.queuedRuns, ...runStore.inProgressRuns]),
      jobsByRunId: emptyJobsByRunId(),
      onCtxReady: (ctx2) => {
        capturedCtx = ctx2
      },
    })

    await new Promise((r) => setTimeout(r, 350))

    // STEP 3a — Post-assert: flag is still false (no stray focusin fired during the move)
    expect(ctx.kanbanHasFocus).toBe(false)

    // STEP 3b — Post-assert: focus did NOT migrate into the new card button
    // Query the new card in the inProgress column (100n moved there)
    const newButton = document.querySelector(
      'section[aria-labelledby="kanban-col-in-progress"] article[data-run-id="100"] .run-card-activate',
    ) as HTMLElement | null
    expect(newButton).toBeTruthy()
    expect(document.activeElement).not.toBe(newButton)
  })
})
