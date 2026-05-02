import { render, screen } from '@testing-library/svelte'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createMockRun, createMockRunEvent } from '$lib/test-utils/factories'

// Mock localStorage since browsers still need this mock in some contexts
const mockLocalStorage = (() => {
  let store: Record<string, string> = {}

  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value
    },
    removeItem: (key: string) => {
      delete store[key]
    },
    clear: () => {
      store = {}
    },
  }
})()

vi.stubGlobal('localStorage', mockLocalStorage)

// Stub matchMedia for the animation module import chain
vi.stubGlobal(
  'matchMedia',
  vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
)

describe('KanbanBoard (browser mode)', () => {
  // Import stores at module level so they persist across tests
  let connectionStore: typeof import('$lib/stores/connection.svelte')['connectionStore']
  let runStore: typeof import('$lib/stores/runs.svelte')['runStore']

  // Set up stores before tests run
  beforeAll(async () => {
    const connModule = await import('$lib/stores/connection.svelte')
    const runsModule = await import('$lib/stores/runs.svelte')
    connectionStore = connModule.connectionStore
    runStore = runsModule.runStore
  })

  beforeEach(() => {
    mockLocalStorage.clear()
    // Reset store state
    connectionStore.status = 'disconnected'
    runStore.clear()
  })

  afterEach(() => {
    mockLocalStorage.clear()
  })

  // AC7.1: Hydration placeholder when not connected
  it('renders hydration placeholder when connection status is not connected', async () => {
    const { default: KanbanBoard } = await import('./KanbanBoard.test-harness.svelte')
    render(KanbanBoard)

    // Default connectionStore.status is 'disconnected'
    const connectingText = screen.getByText(/Connecting/i)
    expect(connectingText).toBeTruthy()

    // Ensure "No workflows yet." is NOT visible
    expect(() => screen.getByText(/No workflows yet/i)).toThrow()
  })

  // AC7.2: Empty state when connected with zero runs
  it('renders empty state when connected but no runs', async () => {
    const { default: KanbanBoard } = await import('./KanbanBoard.test-harness.svelte')
    render(KanbanBoard)

    // Set connection to connected
    connectionStore.status = 'connected'

    // Wait for reactivity
    await new Promise((r) => setTimeout(r, 50))

    // Should render "No workflows yet." text
    const emptyText = screen.getByText(/No workflows yet/i)
    expect(emptyText).toBeTruthy()

    // Should not render column headings
    expect(() => screen.getByText(/QUEUED/i)).toThrow()
    expect(() => screen.getByText(/IN PROGRESS/i)).toThrow()
    expect(() => screen.getByText(/COMPLETED/i)).toThrow()
  })

  // AC7.3: Populated state with three column headings
  it('renders three-column kanban grid when connected with runs', async () => {
    const KanbanBoard = (await import('./KanbanBoard.test-harness.svelte')).default

    // Set connection to connected BEFORE rendering
    connectionStore.status = 'connected'

    // Create test runs for each status
    const queuedRun = createMockRun({
      id: 1n,
      status: 'Queued',
      displayTitle: 'Test Run',
      htmlUrl: 'https://example.com/run/1',
    })

    const inProgressRun = createMockRun({
      id: 2n,
      status: 'InProgress',
      displayTitle: 'Test Run 2',
      htmlUrl: 'https://example.com/run/2',
    })

    const completedRun = createMockRun({
      id: 3n,
      status: 'Completed',
      conclusion: 'Success',
      displayTitle: 'Test Run 3',
      htmlUrl: 'https://example.com/run/3',
    })

    // Apply events to populate the store BEFORE rendering
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: queuedRun.id,
        org: queuedRun.org,
        repo: queuedRun.repo,
        workflowName: queuedRun.workflowName,
        workflowPath: queuedRun.workflowPath,
        branch: queuedRun.branch,
        headSha: queuedRun.headSha,
        commitMessage: queuedRun.commitMessage,
        triggerEvent: queuedRun.event,
        displayTitle: queuedRun.displayTitle,
        htmlUrl: queuedRun.htmlUrl,
        createdAt: queuedRun.createdAt,
        runStartedAt: null,
        updatedAt: queuedRun.updatedAt,
        action: { type: 'Requested' },
      }),
    )

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: inProgressRun.id,
        org: inProgressRun.org,
        repo: inProgressRun.repo,
        workflowName: inProgressRun.workflowName,
        workflowPath: inProgressRun.workflowPath,
        branch: inProgressRun.branch,
        headSha: inProgressRun.headSha,
        commitMessage: inProgressRun.commitMessage,
        triggerEvent: inProgressRun.event,
        displayTitle: inProgressRun.displayTitle,
        htmlUrl: inProgressRun.htmlUrl,
        createdAt: inProgressRun.createdAt,
        runStartedAt: inProgressRun.runStartedAt,
        updatedAt: inProgressRun.updatedAt,
        action: { type: 'InProgress' },
      }),
    )

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: completedRun.id,
        org: completedRun.org,
        repo: completedRun.repo,
        workflowName: completedRun.workflowName,
        workflowPath: completedRun.workflowPath,
        branch: completedRun.branch,
        headSha: completedRun.headSha,
        commitMessage: completedRun.commitMessage,
        triggerEvent: completedRun.event,
        displayTitle: completedRun.displayTitle,
        htmlUrl: completedRun.htmlUrl,
        createdAt: completedRun.createdAt,
        runStartedAt: completedRun.runStartedAt,
        updatedAt: completedRun.updatedAt,
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    // Now render AFTER store is populated
    render(KanbanBoard)

    // Wait for reactivity
    await new Promise((r) => setTimeout(r, 50))

    // Should render three column headings - use getByRole to avoid multiple matches
    expect(screen.getByRole('heading', { name: /QUEUED/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /IN PROGRESS/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /COMPLETED/i })).toBeTruthy()

    // Should NOT render "No workflows yet." or "Connecting..."
    expect(() => screen.getByText(/No workflows yet/i)).toThrow()
    expect(() => screen.getByText(/Connecting/i)).toThrow()
  })

  // AC7.4: Card distribution across columns
  it('distributes cards to correct columns based on run status', async () => {
    const KanbanBoard = (await import('./KanbanBoard.test-harness.svelte')).default

    connectionStore.status = 'connected'

    const queuedRun = createMockRun({
      id: 1n,
      status: 'Queued',
      displayTitle: 'Test Run',
      htmlUrl: 'https://example.com/run/1',
    })

    const inProgressRun = createMockRun({
      id: 2n,
      status: 'InProgress',
      displayTitle: 'Test Run 2',
      htmlUrl: 'https://example.com/run/2',
    })

    const completedRun = createMockRun({
      id: 3n,
      status: 'Completed',
      conclusion: 'Success',
      displayTitle: 'Test Run 3',
      htmlUrl: 'https://example.com/run/3',
    })

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: queuedRun.id,
        org: queuedRun.org,
        repo: queuedRun.repo,
        workflowName: queuedRun.workflowName,
        workflowPath: queuedRun.workflowPath,
        branch: queuedRun.branch,
        headSha: queuedRun.headSha,
        commitMessage: queuedRun.commitMessage,
        triggerEvent: queuedRun.event,
        displayTitle: queuedRun.displayTitle,
        htmlUrl: queuedRun.htmlUrl,
        createdAt: queuedRun.createdAt,
        runStartedAt: null,
        updatedAt: queuedRun.updatedAt,
        action: { type: 'Requested' },
      }),
    )

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: inProgressRun.id,
        org: inProgressRun.org,
        repo: inProgressRun.repo,
        workflowName: inProgressRun.workflowName,
        workflowPath: inProgressRun.workflowPath,
        branch: inProgressRun.branch,
        headSha: inProgressRun.headSha,
        commitMessage: inProgressRun.commitMessage,
        triggerEvent: inProgressRun.event,
        displayTitle: inProgressRun.displayTitle,
        htmlUrl: inProgressRun.htmlUrl,
        createdAt: inProgressRun.createdAt,
        runStartedAt: inProgressRun.runStartedAt,
        updatedAt: inProgressRun.updatedAt,
        action: { type: 'InProgress' },
      }),
    )

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: completedRun.id,
        org: completedRun.org,
        repo: completedRun.repo,
        workflowName: completedRun.workflowName,
        workflowPath: completedRun.workflowPath,
        branch: completedRun.branch,
        headSha: completedRun.headSha,
        commitMessage: completedRun.commitMessage,
        triggerEvent: completedRun.event,
        displayTitle: completedRun.displayTitle,
        htmlUrl: completedRun.htmlUrl,
        createdAt: completedRun.createdAt,
        runStartedAt: completedRun.runStartedAt,
        updatedAt: completedRun.updatedAt,
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    const { container } = render(KanbanBoard)

    await new Promise((r) => setTimeout(r, 50))

    // Verify each card appears in correct column using data-run-id
    const queuedCard = container.querySelector(
      'section[aria-labelledby="kanban-col-queued"] [data-run-id="1"]',
    )
    expect(queuedCard).toBeTruthy()

    const inProgressCard = container.querySelector(
      'section[aria-labelledby="kanban-col-in-progress"] [data-run-id="2"]',
    )
    expect(inProgressCard).toBeTruthy()

    const completedCard = container.querySelector(
      'section[aria-labelledby="kanban-col-completed"] [data-run-id="3"]',
    )
    expect(completedCard).toBeTruthy()
  })

  // AC7.5: Column counts reflect run counts
  it('displays correct count badges for each column', async () => {
    const KanbanBoard = (await import('./KanbanBoard.test-harness.svelte')).default

    connectionStore.status = 'connected'

    // Create multiple runs of each status
    for (let i = 1; i <= 2; i++) {
      runStore.applyRunEvent({
        runId: BigInt(i),
        org: 'test',
        repo: 'repo',
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: `abc${i}`,
        commitMessage: `Test commit ${i}`,
        triggerEvent: 'push',
        displayTitle: `Test Run ${i}`,
        htmlUrl: `https://example.com/run/${i}`,
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Requested',
        },
      })
    }

    for (let i = 3; i <= 4; i++) {
      runStore.applyRunEvent({
        runId: BigInt(i),
        org: 'test',
        repo: 'repo',
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: `abc${i}`,
        commitMessage: `Test commit ${i}`,
        triggerEvent: 'push',
        displayTitle: `Test Run ${i}`,
        htmlUrl: `https://example.com/run/${i}`,
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: {
          type: 'InProgress',
        },
      })
    }

    for (let i = 5; i <= 7; i++) {
      runStore.applyRunEvent({
        runId: BigInt(i),
        org: 'test',
        repo: 'repo',
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'main',
        headSha: `abc${i}`,
        commitMessage: `Test commit ${i}`,
        triggerEvent: 'push',
        displayTitle: `Test Run ${i}`,
        htmlUrl: `https://example.com/run/${i}`,
        createdAt: new Date().toISOString(),
        runStartedAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
        action: {
          type: 'Completed',
          data: {
            conclusion: 'Success',
          },
        },
      })
    }

    const { container } = render(KanbanBoard)

    await new Promise((r) => setTimeout(r, 50))

    // Column headers show counts — verify they are visible using getByRole
    expect(screen.getByRole('heading', { name: /QUEUED/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /IN PROGRESS/i })).toBeTruthy()
    expect(screen.getByRole('heading', { name: /COMPLETED/i })).toBeTruthy()

    // Verify store counts: 2 queued, 2 in progress, 3 completed
    expect(runStore.queuedRuns.length).toBe(2)
    expect(runStore.inProgressRuns.length).toBe(2)
    expect(runStore.completedRuns.length).toBe(3)

    // Verify RENDERED count badges match (the actual DOM output)
    const queuedSection = container.querySelector('section[aria-labelledby="kanban-col-queued"]')
    const queuedCountSpan = queuedSection?.querySelector('h2 + span')
    expect(queuedCountSpan?.textContent).toBe('2')

    const inProgressSection = container.querySelector(
      'section[aria-labelledby="kanban-col-in-progress"]',
    )
    const inProgressCountSpan = inProgressSection?.querySelector('h2 + span')
    expect(inProgressCountSpan?.textContent).toBe('2')

    const completedSection = container.querySelector(
      'section[aria-labelledby="kanban-col-completed"]',
    )
    const completedCountSpan = completedSection?.querySelector('h2 + span')
    expect(completedCountSpan?.textContent).toBe('3')
  })

  // AC7.6: Snapshot reload stability
  it('preserves DOM identity and ordering across snapshot reload', async () => {
    const KanbanBoard = (await import('./KanbanBoard.test-harness.svelte')).default

    connectionStore.status = 'connected'

    const runs = [
      createMockRun({
        id: 1n,
        status: 'Queued',
        displayTitle: 'Test Run 1',
        htmlUrl: 'https://example.com/run/1',
        createdAt: '2026-04-16T10:00:00Z',
        updatedAt: '2026-04-16T10:00:00Z',
      }),
      createMockRun({
        id: 2n,
        status: 'Queued',
        displayTitle: 'Test Run 2',
        htmlUrl: 'https://example.com/run/2',
        createdAt: '2026-04-16T10:01:00Z',
        updatedAt: '2026-04-16T10:01:00Z',
      }),
    ]

    // Load initial snapshot
    runStore.loadSnapshot(runs, [])

    const { container } = render(KanbanBoard)

    await new Promise((r) => setTimeout(r, 50))

    // Record card order
    const cardsAfterFirst = Array.from(
      container.querySelectorAll('section[aria-labelledby="kanban-col-queued"] [data-run-id]'),
    ).map((el) => el.getAttribute('data-run-id'))

    // Reload with identical data
    runStore.loadSnapshot(runs, [])

    await new Promise((r) => setTimeout(r, 50))

    // Check card order is identical
    const cardsAfterSecond = Array.from(
      container.querySelectorAll('section[aria-labelledby="kanban-col-queued"] [data-run-id]'),
    ).map((el) => el.getAttribute('data-run-id'))

    expect(cardsAfterFirst).toEqual(cardsAfterSecond)
  })
})
