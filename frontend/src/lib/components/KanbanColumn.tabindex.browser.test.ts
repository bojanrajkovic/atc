/**
 * Kanban-level tabindex invariant tests (AC1.1, AC1.4, AC1.6, AC1.7).
 *
 * These tests need the REAL RovingFocusProvider (not a vi.mock stub) so that
 * tabindex derivation in RunCard is driven by live $state/$derived reactivity.
 * They live in a separate file from KanbanColumn.browser.test.ts which applies
 * a top-level vi.mock stub for the animation-focused tests.
 */
import { render } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { RovingFocusContext } from '$lib/components/roving/context'
import type { JobStats } from '$lib/stores/runs.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import KanbanBoardInvariantHarness from './KanbanBoardInvariant.test-harness.svelte'

const emptyStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

function statsMapFor(runs: readonly WorkflowRun[]): Map<bigint, JobStats> {
  const m = new Map<bigint, JobStats>()
  for (const r of runs) m.set(r.id, emptyStats)
  return m
}

function emptyJobsByRunId(): Map<bigint, readonly Job[]> {
  return new Map()
}

/** Helper: populate runStore with queued runs and return the created WorkflowRun objects. */
function addQueuedRuns(ids: bigint[]): WorkflowRun[] {
  const runs: WorkflowRun[] = []
  for (const id of ids) {
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: id,
        action: { type: 'Requested' },
        createdAt: `2026-05-01T10:00:0${Number(id)}Z`,
      }),
    )
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    runs.push(runStore.runs.get(id)!)
  }
  return runs
}

/** Helper: populate runStore with in-progress runs and return them. */
function addInProgressRuns(ids: bigint[]): WorkflowRun[] {
  const runs: WorkflowRun[] = []
  for (const id of ids) {
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: id,
        action: { type: 'InProgress' },
        runStartedAt: `2026-05-01T10:00:0${Number(id)}Z`,
      }),
    )
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    runs.push(runStore.runs.get(id)!)
  }
  return runs
}

/** Helper: populate runStore with completed runs and return them. */
function addCompletedRuns(ids: bigint[]): WorkflowRun[] {
  const runs: WorkflowRun[] = []
  for (const id of ids) {
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: id,
        action: { type: 'Completed', data: { conclusion: 'Success' } },
        updatedAt: `2026-05-01T10:00:0${Number(id)}Z`,
      }),
    )
    // eslint-disable-next-line @typescript-eslint/no-non-null-assertion
    runs.push(runStore.runs.get(id)!)
  }
  return runs
}

describe('roving-tabindex single-active invariant (AC1.1, AC1.4, AC1.6, AC1.7)', () => {
  let capturedCtx: RovingFocusContext | undefined

  beforeEach(() => {
    runStore.clear()
    uiStore.lastTriggerRunId = null
    capturedCtx = undefined
  })

  afterEach(() => {
    runStore.clear()
  })

  it('AC1.1: initial render with all three columns populated — exactly one tabindex=0 on first queued card', async () => {
    const queued = addQueuedRuns([10n, 20n, 30n])
    const inProgress = addInProgressRuns([40n, 50n])
    const completed = addCompletedRuns([60n])

    const { container } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queued,
        inProgressRuns: inProgress,
        completedRuns: completed,
        jobStatsByRun: statsMapFor([...queued, ...inProgress, ...completed]),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    await tick()

    // Exactly one card should have tabindex="0"
    const focused = container.querySelectorAll('button.run-card-activate[tabindex="0"]')
    expect(focused).toHaveLength(1)

    // That card must be the first queued run (highest priority per AC1.1)
    const firstQueuedId = runStore.queuedRuns[0]?.id
    expect(firstQueuedId).toBe(10n)
    const focusedCard = focused[0]?.closest('.run-card')
    expect(focusedCard?.getAttribute('data-run-id')).toBe(String(firstQueuedId))
  })

  it('AC1.1 (column priority): empty Queued, populated InProgress — tabindex=0 on first in-progress card', async () => {
    const inProgress = addInProgressRuns([40n, 50n])

    const { container } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: [],
        inProgressRuns: inProgress,
        completedRuns: [],
        jobStatsByRun: statsMapFor(inProgress),
        jobsByRunId: emptyJobsByRunId(),
      },
    })

    await tick()

    const focused = container.querySelectorAll('button.run-card-activate[tabindex="0"]')
    expect(focused).toHaveLength(1)

    // First in-progress run (sorted by runStartedAt desc — most recent first)
    const firstInProgressId = runStore.inProgressRuns[0]?.id
    const focusedCard = focused[0]?.closest('.run-card')
    expect(focusedCard?.getAttribute('data-run-id')).toBe(String(firstInProgressId))
  })

  it('AC1.4: all three columns empty — no card has tabindex=0, currentFocusRunId===null', async () => {
    // Store is already clear from beforeEach

    const { container } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: [],
        inProgressRuns: [],
        completedRuns: [],
        jobStatsByRun: new Map(),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx = ctx
        },
      },
    })

    await tick()

    // No run-card-activate buttons at all
    const allButtons = container.querySelectorAll('button.run-card-activate')
    expect(allButtons).toHaveLength(0)

    // ctx.currentFocusRunId must be null
    expect(capturedCtx?.currentFocusRunId).toBeNull()
  })

  it('AC1.6: mid-reorder — never two cards simultaneously have tabindex=0', async () => {
    const queued = addQueuedRuns([10n, 20n, 30n])

    let capturedCtx2: RovingFocusContext | undefined
    const { container, rerender } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queued,
        inProgressRuns: [],
        completedRuns: [],
        jobStatsByRun: statsMapFor(queued),
        jobsByRunId: emptyJobsByRunId(),
        onCtxReady: (ctx) => {
          capturedCtx2 = ctx
        },
      },
    })

    await tick()

    // Initially card 10n is focused (first queued)
    expect(container.querySelectorAll('button.run-card-activate[tabindex="0"]')).toHaveLength(1)

    // User navigates to card 20n
    capturedCtx2?.setFocus(20n)
    await tick()

    // Reorder: put 30n first, then 20n, then 10n (20n is still focused explicitly)
    const reorderedQueued = [
      runStore.runs.get(30n)!,
      runStore.runs.get(20n)!,
      runStore.runs.get(10n)!,
    ]

    await rerender({
      queuedRuns: reorderedQueued,
      inProgressRuns: [],
      completedRuns: [],
      jobStatsByRun: statsMapFor(reorderedQueued),
      jobsByRunId: emptyJobsByRunId(),
      onCtxReady: (ctx) => {
        capturedCtx2 = ctx
      },
    })

    // Wait for FLIP animation to settle
    await new Promise((r) => setTimeout(r, 350))

    // Still exactly one tabindex=0
    const focused = container.querySelectorAll('button.run-card-activate[tabindex="0"]')
    expect(focused).toHaveLength(1)

    // And it must still be card 20n (the explicitly focused card)
    const focusedCard = focused[0]?.closest('.run-card')
    expect(focusedCard?.getAttribute('data-run-id')).toBe('20')
  })

  it('AC1.7: no role="grid" or role="gridcell" exists in the kanban subtree', async () => {
    const queued = addQueuedRuns([10n])

    const { container } = render(KanbanBoardInvariantHarness, {
      props: {
        queuedRuns: queued,
        inProgressRuns: [],
        completedRuns: [],
        jobStatsByRun: statsMapFor(queued),
        jobsByRunId: emptyJobsByRunId(),
      },
    })

    await tick()

    expect(container.querySelector('[role="grid"]')).toBeNull()
    expect(container.querySelector('[role="gridcell"]')).toBeNull()

    // Verify existing list/listitem structure is preserved
    expect(container.querySelectorAll('[role="list"]').length).toBeGreaterThan(0)
    expect(container.querySelectorAll('[role="listitem"]').length).toBeGreaterThan(0)
  })
})
