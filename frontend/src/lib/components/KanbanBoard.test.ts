import { render, screen } from '@testing-library/svelte'
import { afterAll, beforeEach, describe, expect, it } from 'vitest'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import { uiStore } from '$lib/stores/ui.svelte'
import { createMockRunEvent } from '$lib/test-utils/factories'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'

import KanbanBoard from './KanbanBoard.svelte'

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
    const completedJob: JobEventEnvelope = {
      jobId: 1n,
      runId: 42n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'build',
      createdAt: '2026-04-17T09:58:00Z',
      startedAt: '2026-04-17T09:58:05Z',
      completedAt: '2026-04-17T09:59:00Z',
      action: {
        type: 'Completed',
        data: { conclusion: 'Success', runner: null, labels: [], steps: [] },
      },
    }
    const queuedJob1: JobEventEnvelope = {
      jobId: 2n,
      runId: 42n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'test',
      createdAt: '2026-04-17T09:58:00Z',
      startedAt: null,
      completedAt: null,
      action: { type: 'Queued', data: { labels: [], steps: [] } },
    }
    const queuedJob2: JobEventEnvelope = {
      jobId: 3n,
      runId: 42n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'deploy',
      createdAt: '2026-04-17T09:58:00Z',
      startedAt: null,
      completedAt: null,
      action: { type: 'Queued', data: { labels: [], steps: [] } },
    }

    runStore.applyJobEvent(completedJob)
    runStore.applyJobEvent(queuedJob1)
    runStore.applyJobEvent(queuedJob2)

    render(KanbanBoard)

    expect(screen.getByText('Jobs 1 of 3')).toBeTruthy()
  })
})
