import { render, screen } from '@testing-library/svelte'
import { describe, expect, it, vi } from 'vitest'
import type { JobStats } from '$lib/stores/runs.svelte'
import { createMockRun } from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

// Mock svelte/motion so prefersReducedMotion.current is true for all tests.
// This must be at file scope so vi.mock() hoisting ensures kanban-transitions.ts
// reads the mocked value when it is first imported. The animation tests below
// remain valid because they check DOM structure (card identity, cross-column
// movement), not timing — and all rely on a setTimeout(≥350ms) that is more
// than enough with DURATION_MOVE=0.
vi.mock('svelte/motion', () => ({
  prefersReducedMotion: { current: true },
}))

// These browser tests exercise animation/FLIP/crossfade behavior — they don't care
// about roving tabindex focus management. Stub getRovingContext with a static
// no-focus context so RunCard mounts without requiring a provider in the test tree.
vi.mock('$lib/components/roving/context', () => ({
  getRovingContext: () => ({
    focusedRunId: null,
    initialFocusRunId: null,
    currentFocusRunId: null,
    kanbanHasFocus: false,
    setFocus: () => {},
    setKanbanHasFocus: () => {},
    restoreFocusToInitial: () => {},
  }),
  setRovingContext: () => {},
  ROVING_CONTEXT_KEY: Symbol('RovingFocusContext'),
}))

const emptyStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

function statsMapFor(runs: readonly WorkflowRun[]): Map<bigint, JobStats> {
  const m = new Map<bigint, JobStats>()
  for (const r of runs) m.set(r.id, emptyStats)
  return m
}

describe('KanbanColumn (browser mode)', () => {
  describe('kanban-board.AC5.3: animate:flip is applied to cards', () => {
    it('keeps card DOM identity stable across array reorder', async () => {
      // Each test imports its own fresh copy of KanbanColumn
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      let runs: readonly WorkflowRun[] = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      const { container, rerender } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Get initial card DOM node reference (not just the selector)
      const card1Before = container.querySelector('[data-run-id="100"]') as HTMLElement
      const card2Before = container.querySelector('[data-run-id="200"]') as HTMLElement
      expect(card1Before).toBeTruthy()
      expect(card2Before).toBeTruthy()

      // Reorder the array (swap)
      runs = [createMockRun({ id: 200n }), createMockRun({ id: 100n })] as const
      await rerender({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs,
        jobStatsByRun: statsMapFor(runs),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      // Wait for animation/reactivity
      await new Promise((r) => setTimeout(r, 350))

      // Cards should still exist with same data-run-id values
      const card1After = container.querySelector('[data-run-id="100"]') as HTMLElement
      const card2After = container.querySelector('[data-run-id="200"]') as HTMLElement
      expect(card1After).toBeTruthy()
      expect(card2After).toBeTruthy()

      // Verify DOM identity is preserved (same node reference, not recreated)
      expect(card1After).toBe(card1Before)
      expect(card2After).toBe(card2Before)

      // Verify data-run-id attributes are stable (converted to string)
      expect(card1After?.getAttribute('data-run-id')).toBe('100')
      expect(card2After?.getAttribute('data-run-id')).toBe('200')
    })
  })

  describe('kanban-board.AC5.4: crossfade key matching between columns', () => {
    it('moves a card between two columns via crossfade matching', async () => {
      // Each test imports its own fresh copy of KanbanColumn
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      // Render two columns side-by-side with a card in column A
      let runs1: readonly WorkflowRun[] = [createMockRun({ id: 100n })]
      let runs2: readonly WorkflowRun[] = [createMockRun({ id: 200n })]

      const { container: container1, rerender: rerender1 } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: runs1,
          jobStatsByRun: statsMapFor(runs1),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Import again (same module) to render second column sharing the same crossfade
      const { default: KanbanColumnSecond } = await import('./KanbanColumn.svelte')
      const { container: container2, rerender: rerender2 } = render(KanbanColumnSecond, {
        props: {
          label: 'IN_PROGRESS',
          headingId: 'kanban-col-in-progress',
          runs: runs2,
          jobStatsByRun: statsMapFor(runs2),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Verify initial state: card 100 in column 1, card 200 in column 2
      expect(container1.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(container2.querySelector('[data-run-id="200"]')).toBeTruthy()

      // Move card 100 from column 1 to column 2 (simulating status change)
      runs1 = []
      runs2 = [createMockRun({ id: 200n }), createMockRun({ id: 100n })]

      await rerender1({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs: runs1,
        jobStatsByRun: statsMapFor(runs1),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      await rerender2({
        label: 'IN_PROGRESS',
        headingId: 'kanban-col-in-progress',
        runs: runs2,
        jobStatsByRun: statsMapFor(runs2),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      // Wait for crossfade animation to settle
      await new Promise((r) => setTimeout(r, 350))

      // Verify cross-column transition: card 100 no longer in column 1, now in column 2
      expect(container1.querySelector('[data-run-id="100"]')).toBeFalsy()
      expect(container2.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(container2.querySelector('[data-run-id="200"]')).toBeTruthy()
    })
  })

  describe('kanban-board.AC5.5: bigint as {#each} key', () => {
    it('handles bigint run IDs without errors', async () => {
      // Each test imports its own fresh copy of KanbanColumn
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      const run1 = createMockRun({ id: 1n })
      const run2 = createMockRun({ id: 2n })
      const run3 = createMockRun({ id: 3n })

      let runs: readonly WorkflowRun[] = [run1, run2, run3]
      const { container, rerender } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Verify cards are present
      let listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(3)

      // Reorder with bigint keys
      runs = [run3, run1, run2]
      await rerender({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs,
        jobStatsByRun: statsMapFor(runs),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      await new Promise((r) => setTimeout(r, 50))

      // Verify reorder succeeded and cards are still present
      listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(3)

      // Verify data-run-id attributes are stable and correct
      expect(container.querySelector('[data-run-id="1"]')).toBeTruthy()
      expect(container.querySelector('[data-run-id="2"]')).toBeTruthy()
      expect(container.querySelector('[data-run-id="3"]')).toBeTruthy()
    })
  })

  describe('kanban-board.AC5.6: burst (multiple runs in one update)', () => {
    it('moves multiple cards between columns in a single update', async () => {
      // Each test imports its own fresh copy of KanbanColumn
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      // Render two columns: column A has 100n and 200n, column B is empty
      let runsA: readonly WorkflowRun[] = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      let runsB: readonly WorkflowRun[] = []

      const { container: containerA, rerender: rerenderA } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: runsA,
          jobStatsByRun: statsMapFor(runsA),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Import again to render second column with same crossfade instance
      const { default: KanbanColumnB } = await import('./KanbanColumn.svelte')
      const { container: containerB, rerender: rerenderB } = render(KanbanColumnB, {
        props: {
          label: 'IN_PROGRESS',
          headingId: 'kanban-col-in-progress',
          runs: runsB,
          jobStatsByRun: statsMapFor(runsB),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Verify initial state: both cards in column A
      expect(containerA.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(containerA.querySelector('[data-run-id="200"]')).toBeTruthy()
      expect(containerB.querySelector('[data-run-id="100"]')).toBeFalsy()
      expect(containerB.querySelector('[data-run-id="200"]')).toBeFalsy()

      // Burst: move both cards to column B in a single update
      runsA = []
      runsB = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]

      await rerenderA({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs: runsA,
        jobStatsByRun: statsMapFor(runsA),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      await rerenderB({
        label: 'IN_PROGRESS',
        headingId: 'kanban-col-in-progress',
        runs: runsB,
        jobStatsByRun: statsMapFor(runsB),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      // Wait for crossfade animations to settle
      await new Promise((r) => setTimeout(r, 350))

      // Verify both cards moved to column B
      expect(containerA.querySelector('[data-run-id="100"]')).toBeFalsy()
      expect(containerA.querySelector('[data-run-id="200"]')).toBeFalsy()
      expect(containerB.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(containerB.querySelector('[data-run-id="200"]')).toBeTruthy()

      // Verify final count in destination column
      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })
  })

  describe('kanban-board.AC6.3 & AC6.4: Animations respect prefers-reduced-motion', () => {
    // The vi.mock('svelte/motion', ...) at the top of this file ensures
    // prefersReducedMotion.current === true when kanban-transitions.ts is
    // first imported, so DURATION_MOVE/ARRIVE/REMOVE are all 0.

    it('AC6.3: DURATION_MOVE is 0 under reduced motion (mock binds before module import)', async () => {
      // Import after the file-scope mock has taken effect. kanban-transitions.ts
      // reads prefersReducedMotion.current at module-top; the vi.mock hoist ensures
      // the mocked value is visible at that import time.
      const { DURATION_MOVE, DURATION_ARRIVE, DURATION_REMOVE } = await import(
        '$lib/animations/kanban-transitions'
      )

      // All duration constants must be 0 under reduced motion
      expect(DURATION_MOVE).toBe(0)
      expect(DURATION_ARRIVE).toBe(0)
      expect(DURATION_REMOVE).toBe(0)
    })

    it('AC6.3: cross-column movement completes without animation delay under reduced motion', async () => {
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      let runsA: readonly WorkflowRun[] = [createMockRun({ id: 100n })]
      let runsB: readonly WorkflowRun[] = []

      const { container: containerA, rerender: rerenderA } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: runsA,
          jobStatsByRun: statsMapFor(runsA),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Import again (same module) to render second column sharing the same crossfade
      const { default: KanbanColumnSecond } = await import('./KanbanColumn.svelte')
      const { container: containerB, rerender: rerenderB } = render(KanbanColumnSecond, {
        props: {
          label: 'IN_PROGRESS',
          headingId: 'kanban-col-in-progress',
          runs: runsB,
          jobStatsByRun: statsMapFor(runsB),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Verify initial state
      expect(containerA.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(containerB.querySelector('[data-run-id="100"]')).toBeFalsy()

      // Move card between columns
      runsA = []
      runsB = [createMockRun({ id: 100n })]

      await rerenderA({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs: runsA,
        jobStatsByRun: statsMapFor(runsA),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      await rerenderB({
        label: 'IN_PROGRESS',
        headingId: 'kanban-col-in-progress',
        runs: runsB,
        jobStatsByRun: statsMapFor(runsB),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      // With DURATION_MOVE=0, crossfade completes in a single frame. A short
      // settle is still needed for the Svelte reconciler to process transitions.
      await new Promise((r) => setTimeout(r, 50))

      // Verify cross-column transition completed: card removed from source, appears in destination
      expect(containerA.querySelector('[data-run-id="100"]')).toBeFalsy()
      expect(containerB.querySelector('[data-run-id="100"]')).toBeTruthy()
    })

    it('AC6.4: within-column reorder completes instantly under reduced motion', async () => {
      const { default: KanbanColumn } = await import('./KanbanColumn.svelte')

      let runs: readonly WorkflowRun[] = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]

      const { container, rerender } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      // Verify initial state
      expect(container.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(container.querySelector('[data-run-id="200"]')).toBeTruthy()

      // Reorder within the column
      runs = [createMockRun({ id: 200n }), createMockRun({ id: 100n })] as const

      await rerender({
        label: 'QUEUED',
        headingId: 'kanban-col-queued',
        runs,
        jobStatsByRun: statsMapFor(runs),
        activePoolFilter: null,
        jobsByRunId: new Map<bigint, readonly Job[]>(),
      })

      // With DURATION_MOVE=0 the FLIP animation is instantaneous. A short settle
      // allows the Svelte reconciler to finalize the DOM update.
      await new Promise((r) => setTimeout(r, 50))

      // Verify reorder completed - both cards still present in final positions
      expect(container.querySelector('[data-run-id="100"]')).toBeTruthy()
      expect(container.querySelector('[data-run-id="200"]')).toBeTruthy()
    })
  })
})
