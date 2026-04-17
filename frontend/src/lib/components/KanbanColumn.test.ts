import { render, screen } from '@testing-library/svelte'
import { describe, expect, it } from 'vitest'
import type { RunId } from '$lib/types/generated/RunId'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

import KanbanColumn from './KanbanColumn.svelte'

// Test helper to create mock WorkflowRun objects
function createMockRun(overrides: Partial<WorkflowRun> = {}): WorkflowRun {
  const baseId: RunId = 123n
  return {
    id: baseId,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'Test Workflow',
    workflowPath: '.github/workflows/test.yml',
    branch: 'main',
    headSha: 'abc123def456',
    commitMessage: 'Test commit',
    event: 'push',
    displayTitle: 'Test Run',
    status: 'Queued',
    conclusion: null,
    htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/123',
    createdAt: '2024-01-01T00:00:00Z',
    runStartedAt: null,
    updatedAt: '2024-01-01T00:00:00Z',
    ...overrides,
  }
}

describe('KanbanColumn', () => {
  describe('kanban-board.AC1.2: ARIA structure', () => {
    it('renders section with aria-labelledby referencing heading id', () => {
      const run = createMockRun()
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run],
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
        },
      })

      const list = screen.getByRole('list')
      expect(list).toBeTruthy()
    })

    it('renders cards as role=listitem', () => {
      const run1 = createMockRun({ id: 100n })
      const run2 = createMockRun({ id: 200n })
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2],
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })
  })

  describe('Card count and rendering', () => {
    it('renders 2 runs with 2 listitems', () => {
      const run1 = createMockRun({ id: 100n })
      const run2 = createMockRun({ id: 200n })
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2],
        },
      })

      const listItems = screen.getAllByRole('listitem')
      expect(listItems).toHaveLength(2)
    })

    it('renders 3 runs with 3 listitems', () => {
      const run1 = createMockRun({ id: 100n })
      const run2 = createMockRun({ id: 200n })
      const run3 = createMockRun({ id: 300n })
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2, run3],
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
        },
      })

      const list = screen.getByRole('list')
      expect(list).toBeTruthy()

      const listItems = screen.queryAllByRole('listitem')
      expect(listItems).toHaveLength(0)
    })
  })

  describe('Data attributes for test targeting', () => {
    it('sets data-run-id on each card wrapper', () => {
      const run1 = createMockRun({ id: 100n })
      const run2 = createMockRun({ id: 200n })
      const { container } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2],
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
        },
      })

      const card = container.querySelector('article[data-run-id="456"]')
      expect(card).toBeTruthy()
    })

    it('maintains stable data-run-id values across multiple runs', () => {
      const run1 = createMockRun({ id: 111n })
      const run2 = createMockRun({ id: 222n })
      const run3 = createMockRun({ id: 333n })
      const { container } = render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2, run3],
        },
      })

      const card1 = container.querySelector('article[data-run-id="111"]')
      const card2 = container.querySelector('article[data-run-id="222"]')
      const card3 = container.querySelector('article[data-run-id="333"]')

      expect(card1).toBeTruthy()
      expect(card2).toBeTruthy()
      expect(card3).toBeTruthy()
    })
  })

  describe('Column metadata', () => {
    it('displays correct count in column header', () => {
      const run1 = createMockRun({ id: 1n })
      const run2 = createMockRun({ id: 2n })
      const run3 = createMockRun({ id: 3n })
      render(KanbanColumn, {
        props: {
          label: 'QUEUED',
          headingId: 'kanban-col-queued',
          runs: [run1, run2, run3],
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
        },
      })

      const count = screen.getByText('0')
      expect(count).toBeTruthy()
    })
  })
})
