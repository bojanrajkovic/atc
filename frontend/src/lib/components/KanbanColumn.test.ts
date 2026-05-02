import { render, screen } from '@testing-library/svelte'
import { afterAll, describe, expect, it } from 'vitest'
import { poolKey } from '$lib/filters/pool'
import type { JobStats } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockJob, createMockRun } from '$lib/test-utils/factories'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import KanbanColumnHarness from './test-utils/KanbanColumn.test-harness.svelte'

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
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const section = screen.getByRole('region')
      expect(section.getAttribute('aria-labelledby')).toBe('kanban-col-queued')
    })

    it('renders heading with correct id', () => {
      const run = createMockRun()
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const heading = screen.getByRole('heading', { level: 2 })
      expect(heading.id).toBe('kanban-col-queued')
    })

    it('renders heading text in uppercase', () => {
      const run = createMockRun()
      render(KanbanColumnHarness, {
        props: {
          label: 'InProgress',
          headingId: 'kanban-col-in-progress',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const heading = screen.getByRole('heading', { level: 2 })
      expect(heading.textContent).toContain('INPROGRESS')
    })

    it('renders role=list container', () => {
      const run = createMockRun()
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const list = screen.getByRole('list')
      expect(list).toBeTruthy()
    })

    it('renders cards as role=listitem', () => {
      const runs = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })
  })

  describe('Card count and rendering', () => {
    it('renders 2 runs with 2 listitems', () => {
      const runs = [createMockRun({ id: 100n }), createMockRun({ id: 200n })]
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
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
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(3)
    })

    it('renders 0 runs with empty role=list container', () => {
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [],
          jobStatsByRun: new Map<bigint, JobStats>(),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
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
      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const cards = container.querySelectorAll('article[data-run-id]')
      expect(cards).toHaveLength(2)
    })

    it('converts bigint run.id to string in data-run-id attribute', () => {
      const run = createMockRun({ id: 456n })
      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
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
      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
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
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const count = screen.getByText('3')
      expect(count).toBeTruthy()
    })

    it('displays zero count when no runs', () => {
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [],
          jobStatsByRun: new Map<bigint, JobStats>(),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
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
      render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          runs: [run],
          headingId: 'q',
          jobStatsByRun,
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
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
        render(KanbanColumnHarness, {
          props: {
            label: 'QUEUED',
            runs: [run],
            headingId: 'q',
            jobStatsByRun,
            activePoolFilter: null,
            jobsByRunId: new Map<bigint, readonly Job[]>(),
          },
        }),
      ).toThrow(/total-map invariant/)
    })
  })

  describe('AC5.1: pool filter applied to column', () => {
    it('renders only the matching run when activePoolFilter is set', () => {
      const runA = createMockRun({ id: 10n })
      const runB = createMockRun({ id: 20n })
      const runs = [runA, runB]

      // runA has a job matching linux|x86, runB has a job matching windows only
      const jobA = createMockJob({ runId: 10n, id: 100n, labels: ['linux', 'x86', 'self-hosted'] })
      const jobB = createMockJob({ runId: 20n, id: 200n, labels: ['windows'] })

      const jobsByRunId = new Map<bigint, readonly Job[]>([
        [10n, [jobA]],
        [20n, [jobB]],
      ])

      const filter = poolKey(['linux', 'x86'])

      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: filter,
          jobsByRunId,
        },
      })

      // Only runA (id=10) should render
      expect(container.querySelector('article[data-run-id="10"]')).toBeTruthy()
      expect(container.querySelector('article[data-run-id="20"]')).toBeFalsy()
      // Column count badge should reflect the filtered count (1, not 2)
      expect(screen.getByText('1')).toBeTruthy()
    })
  })

  describe('AC5.5: null filter passthrough', () => {
    it('renders all runs unchanged when activePoolFilter is null', () => {
      const runA = createMockRun({ id: 30n })
      const runB = createMockRun({ id: 40n })
      const runs = [runA, runB]

      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      expect(container.querySelector('article[data-run-id="30"]')).toBeTruthy()
      expect(container.querySelector('article[data-run-id="40"]')).toBeTruthy()
      expect(screen.getByText('2')).toBeTruthy()
    })
  })

  describe('frontend-1-0-polish.AC4.1: scroll container has atc-scrollbar class', () => {
    it('the role=list scroll container has class atc-scrollbar applied', () => {
      const run = createMockRun({ id: 1n })
      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
          jobStatsByRun: statsMapFor([run]),
          activePoolFilter: null,
          jobsByRunId: new Map<bigint, readonly Job[]>(),
        },
      })

      const scrollContainer = container.querySelector('[role="list"]')
      expect(scrollContainer).toBeTruthy()
      expect(scrollContainer?.classList.contains('atc-scrollbar')).toBe(true)
    })
  })

  describe('AC5.6: empty result when filter matches no jobs', () => {
    it('renders zero cards and does not crash when filter matches no jobs', () => {
      const runA = createMockRun({ id: 50n })
      const runB = createMockRun({ id: 60n })
      const runs = [runA, runB]

      const jobA = createMockJob({ runId: 50n, id: 500n, labels: ['linux'] })
      const jobB = createMockJob({ runId: 60n, id: 600n, labels: ['windows'] })

      const jobsByRunId = new Map<bigint, readonly Job[]>([
        [50n, [jobA]],
        [60n, [jobB]],
      ])

      // Filter with labels that no job has
      const filter = poolKey(['nonexistent-label'])

      const { container } = render(KanbanColumnHarness, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs,
          jobStatsByRun: statsMapFor(runs),
          activePoolFilter: filter,
          jobsByRunId,
        },
      })

      // No cards should render
      expect(container.querySelectorAll('article[data-run-id]')).toHaveLength(0)
      // Column count badge should be 0
      expect(screen.getByText('0')).toBeTruthy()
      // The list container should still be present (no crash)
      expect(screen.getByRole('list')).toBeTruthy()
    })
  })
})
