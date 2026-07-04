import { render, screen } from '@testing-library/svelte'
import { tick } from 'svelte'
import { afterAll, beforeEach, describe, expect, it } from 'vitest'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'

import KanbanBoardHarness from './test-utils/KanbanBoard.test-harness.svelte'

function queuedJob(jobId: bigint, runId: bigint, name: string): JobEventEnvelope {
  return {
    jobId,
    runId,
    org: 'test-org',
    repo: 'test-repo',
    name,
    createdAt: '2026-04-17T09:58:00Z',
    startedAt: null,
    completedAt: null,
    runAttempt: 1,
    action: { type: 'Queued', data: { labels: [], steps: [] } },
  }
}

function completedJob(
  jobId: bigint,
  runId: bigint,
  name: string,
  conclusion: 'Success' | 'Failure' = 'Success',
): JobEventEnvelope {
  return {
    jobId,
    runId,
    org: 'test-org',
    repo: 'test-repo',
    name,
    createdAt: '2026-04-17T09:58:00Z',
    startedAt: '2026-04-17T09:58:05Z',
    completedAt: '2026-04-17T09:59:00Z',
    runAttempt: 1,
    action: {
      type: 'Completed',
      data: { conclusion, runner: null, labels: [], steps: [] },
    },
  }
}

/**
 * Integration backstop for the RunStore -> KanbanBoard -> KanbanColumn -> RunCard
 * data flow. Component-specific AC landings live in RunCard.test.ts /
 * KanbanColumn.test.ts; this file catches wiring regressions that the isolated
 * component tests would miss (the run renders, its JobStats arrive at the card,
 * and the ProgressBar reflects the current (completed, total) derived from
 * runStore.jobStatsByRun).
 */
describe('KanbanBoard — jobStatsByRun integration', () => {
  beforeEach(() => {
    runStore.clear()
    connectionStore.status = 'connected'
  })

  // KanbanBoard → KanbanColumn → RunCard → uiStore chain starts a 1s
  // setInterval at import time; stop it at file end so the timer does not
  // outlive this suite.
  afterAll(() => {
    uiStore.destroy()
  })

  it('renders ProgressBar "Jobs N of M" reflecting jobStatsByRun for a threaded run', () => {
    // Place one InProgress run.
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 42n,
        action: { type: 'InProgress' },
        runStartedAt: '2026-04-17T09:58:00Z',
      }),
    )

    // Three jobs on the same run: 1 Completed, 2 Queued. JobStats should
    // resolve to { completed: 1, total: 3 } via runStore.jobStatsByRun.
    runStore.applyJobEvent(completedJob(1n, 42n, 'build'))
    runStore.applyJobEvent(queuedJob(2n, 42n, 'test'))
    runStore.applyJobEvent(queuedJob(3n, 42n, 'deploy'))

    render(KanbanBoardHarness)

    expect(screen.getByText('Jobs 1 of 3')).toBeTruthy()
  })

  it('subscriber re-renders when applyJobEvent fires mid-lifecycle', async () => {
    // Place the run + initial jobs, then MOUNT the subscriber (KanbanBoard).
    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 7n,
        action: { type: 'InProgress' },
        runStartedAt: '2026-04-17T09:58:00Z',
      }),
    )
    runStore.applyJobEvent(queuedJob(1n, 7n, 'build'))
    runStore.applyJobEvent(queuedJob(2n, 7n, 'test'))

    render(KanbanBoardHarness)

    // Initial state: 0 of 2 jobs complete.
    expect(screen.getByText('Jobs 0 of 2')).toBeTruthy()

    runStore.applyJobEvent(completedJob(1n, 7n, 'build'))
    await tick()

    expect(screen.getByText('Jobs 1 of 2')).toBeTruthy()

    runStore.applyJobEvent(completedJob(2n, 7n, 'test'))
    await tick()

    expect(screen.getByText('Jobs 2 of 2')).toBeTruthy()
  })

  it('subscriber re-renders when applyRunEvent adds a new run mid-lifecycle', async () => {
    render(KanbanBoardHarness)

    // Connected + zero runs → EmptyState component with default caption.
    expect(screen.getByText('Watching for runs.')).toBeTruthy()

    runStore.applyRunEvent(
      createMockRunEvent({
        runId: 99n,
        action: { type: 'Requested' },
        displayTitle: 'New CI run',
      }),
    )
    await tick()

    expect(screen.getByText('New CI run')).toBeTruthy()
    // An empty run (no jobs) gets { completed: 0, total: 0 } via the
    // total-map invariant — proves jobStatsByRun also re-derived.
    expect(screen.getByText('Jobs 0 of 0')).toBeTruthy()
  })
})

describe('KanbanBoard — zero-repos empty state', () => {
  beforeEach(() => {
    runStore.clear()
    connectionStore.status = 'connected'
    connectionStore.identity = null
  })

  afterAll(() => {
    uiStore.destroy()
  })

  it('shows the default empty state when identity has not been fetched (mode=none)', () => {
    render(KanbanBoardHarness)
    expect(screen.getByText('Watching for runs.')).toBeTruthy()
  })

  it('shows the default empty state when the user has visible repos', () => {
    connectionStore.identity = {
      login: 'octocat',
      repoCount: 3,
      reposRefreshedAt: '2026-07-04T00:00:00Z',
      stale: false,
    }
    render(KanbanBoardHarness)
    expect(screen.getByText('Watching for runs.')).toBeTruthy()
  })

  it('shows the zero-repos message when authenticated with no visible repos', () => {
    connectionStore.identity = {
      login: 'octocat',
      repoCount: 0,
      reposRefreshedAt: '2026-07-04T00:00:00Z',
      stale: false,
    }
    render(KanbanBoardHarness)
    expect(screen.getByText(/no monitored repositories are visible to you/i)).toBeTruthy()
  })
})
