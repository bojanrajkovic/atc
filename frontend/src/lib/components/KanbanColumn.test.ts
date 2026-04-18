import { render, screen } from '@testing-library/svelte'
import { afterAll, describe, expect, it } from 'vitest'
import type { JobStats } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRun } from '$lib/test-utils/factories'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import KanbanColumn from './KanbanColumn.svelte'

const emptyStats: JobStats = { completed: 0, total: 0, runnerSummary: null }

function statsMapFor(runs: readonly WorkflowRun[]): Map<bigint, JobStats> {
  const m = new Map<bigint, JobStats>()
  for (const r of runs) m.set(r.id, emptyStats)
  return m
}

describe('KanbanColumn', () => {
  // KanbanColumn → RunCard → uiStore chain starts a 1s setInterval at import
  // time; stop it at file end so the timer does not outlive this suite.
  afterAll(() => {
    uiStore.destroy()
  })

  describe('kanban-board.AC1.2: ARIA structure', () => {
    it('renders section with aria-labelledby referencing heading id', () => {
      const run = createMockRun()
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
        },
      })

      const section = screen.getByRole('region')
      expect(section.getAttribute('aria-labelledby')).toBe('kanban-col-queued')
    })

    it('renders heading with correct id', () => {
      const run = createMockRun()
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
        },
      })

      const heading = screen.getByRole('heading', { level: 2 })
      expect(heading.id).toBe('kanban-col-queued')
    })

    it('renders heading text in uppercase', () => {
      const run = createMockRun()
      render(KanbanColumn, {
        props: {
          label: 'InProgress',
          headingId: 'kanban-col-in-progress',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
        },
      })

      const heading = screen.getByRole('heading', { level: 2 })
      expect(heading.textContent).toContain('INPROGRESS')
    })

    it('renders role=list container', () => {
      const run = createMockRun()
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
        },
      })

      const list = screen.getByRole('list')
      expect(list).toBeTruthy()
    })

    it('renders cards as role=listitem', () => {
      const runs = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })
  })

  describe('Card count and rendering', () => {
    it('renders 2 runs with 2 listitems', () => {
      const runs = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })

    it('renders 3 runs with 3 listitems', () => {
      const runs = [
        createMockRun({ id: 100n }),
        createMockRun({ id: 200n }),
        createMockRun({ id: 300n }),
      ]
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(3)
    })

    it('renders 0 runs with empty role=list container', () => {
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [],
          jobStatsByRun: new Map<bigint, JobStats>(),
        },
      })

      const list = screen.getByRole('list')
      expect(list).toBeTruthy()

      const listItems = screen.queryAllByRole('listitem')
      expect(listItems).toHaveLength(0)
    })
  })

  describe('Data attributes for test targeting', () => {
    it('sets data-run-id on each RunCard root article', () => {
      const runs = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      const { container } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      const cards = container.querySelectorAll('article[data-run-id]')
      expect(cards).toHaveLength(2)
    })

    it('converts bigint run.id to string in data-run-id attribute', () => {
      const run = createMockRun({ id: 456n })
      const { container } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
        },
      })

      const card = container.querySelector('article[data-run-id="456"]')
      expect(card).toBeTruthy()
    })

    it('maintains stable data-run-id values across multiple runs', () => {
      const runs = [
        createMockRun({ id: 111n }),
        createMockRun({ id: 222n }),
        createMockRun({ id: 333n }),
      ]
      const { container } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      expect(container.querySelector('article[data-run-id="111"]')).toBeTruthy()
      expect(container.querySelector('article[data-run-id="222"]')).toBeTruthy()
      expect(container.querySelector('article[data-run-id="333"]')).toBeTruthy()
    })
  })

  describe('Column metadata', () => {
    it('displays correct count in column header', () => {
      const runs = [createMockRun({ id: 1n }), createMockRun({ id: 2n }), createMockRun({ id: 3n })]
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
        },
      })

      const count = screen.getByText('3')
      expect(count).toBeTruthy()
    })

    it('displays zero count when no runs', () => {
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [],
          jobStatsByRun: new Map<bigint, JobStats>(),
        },
      })

      const count = screen.getByText('0')
      expect(count).toBeTruthy()
    })
  })

  describe('jobStatsByRun threading', () => {
    it('passes jobStats from jobStatsByRun to each rendered RunCard', () => {
      const run = createMockRun({ id: 7n })
      const jobStatsByRun = new Map<bigint, JobStats>([
        [7n, { completed: 2, total: 5, runnerSummary: 'runner-a' }],
      ])
      render(KanbanColumn, {
        props: { label: 'QUEUED', runs: [run], headingId: 'q', jobStatsByRun },
      })

      // ProgressBar renders "Jobs 2 of 5" via JobStats.completed / total.
      expect(screen.getByText('Jobs 2 of 5')).toBeTruthy()
      // RunnerLabel renders summary (prefixed with ⊞ glyph).
      expect(screen.getByText(/runner-a/)).toBeTruthy()
    })

    it('throws a total-map invariant error if a run has no JobStats entry', () => {
      const run = createMockRun({ id: 9n })
      // jobStatsByRun does NOT contain id 9n — invariant violation.
      const jobStatsByRun = new Map<bigint, JobStats>()
      expect(() =>
        render(KanbanColumn, {
          props: { label: 'QUEUED', runs: [run], headingId: 'q', jobStatsByRun },
        }),
      ).toThrow(/total-map invariant/)
    })
  })
})
