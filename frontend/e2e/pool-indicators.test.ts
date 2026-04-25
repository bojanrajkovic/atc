import { expect, test } from '@playwright/test'
import type { JobEventEnvelope } from '../src/lib/types/generated/JobEventEnvelope'
import type { RunnerPoolStats } from '../src/lib/types/generated/RunnerPoolStats'
import { makeJobSeqEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

test.describe('Pool indicators update live', () => {
  test.beforeEach(async ({ page }) => {
    // Inject the mock WebSocket so ConnectionManager can succeed
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  test('TopBar pool indicators update as Job events flow', async ({ page }) => {
    // Navigate to app, wait for initial render with snapshot
    await page.goto('/')

    // Wait for the connection to establish and initial state to populate
    await page.waitForTimeout(500)

    // Dispatch a Queued Job event with poolStatsAfter showing 1 queued runner
    const queuedPool: RunnerPoolStats = {
      labels: ['ubuntu-latest'],
      queued: 1,
      running: 0,
      groupName: 'GitHub Actions',
      isElastic: true,
      total: null,
    }

    const queuedJobEnvelope: JobEventEnvelope = {
      jobId: 1n,
      runId: 100n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'Test Job',
      createdAt: new Date().toISOString(),
      startedAt: null,
      completedAt: null,
      action: {
        type: 'Queued',
        data: {
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    }

    await sendWS(
      page,
      makeJobSeqEvent(10, {
        jobData: queuedJobEnvelope,
        poolStatsAfter: [queuedPool],
      }),
    )

    // Assert TopBar pool indicator shows the pool with queued count
    const poolIndicator = page.getByTestId('runner-pool-GitHub Actions')
    await expect(poolIndicator).toBeVisible()
    await expect(poolIndicator).toContainText(/\+1 queued/i)

    // Dispatch InProgress event with poolStatsAfter showing 0 queued, 1 running
    const inProgressPool: RunnerPoolStats = {
      labels: ['ubuntu-latest'],
      queued: 0,
      running: 1,
      groupName: 'GitHub Actions',
      isElastic: true,
      total: null,
    }

    const inProgressJobEnvelope: JobEventEnvelope = {
      jobId: 1n,
      runId: 100n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'Test Job',
      createdAt: new Date().toISOString(),
      startedAt: new Date().toISOString(),
      completedAt: null,
      action: {
        type: 'InProgress',
        data: {
          runner: null,
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    }

    await sendWS(
      page,
      makeJobSeqEvent(11, {
        jobData: inProgressJobEnvelope,
        poolStatsAfter: [inProgressPool],
      }),
    )

    // Assert TopBar pool indicator updates: queued badge gone, running count shows
    await expect(poolIndicator).toContainText(/1/) // Should show running count
    // Ensure queued badge is gone
    const queuedBadge = poolIndicator.getByText(/queued/)
    await expect(queuedBadge).not.toBeVisible()

    // Dispatch Completed event with poolStatsAfter = [] (pool removed)
    const completedJobEnvelope: JobEventEnvelope = {
      jobId: 1n,
      runId: 100n,
      org: 'test-org',
      repo: 'test-repo',
      name: 'Test Job',
      createdAt: new Date().toISOString(),
      startedAt: new Date().toISOString(),
      completedAt: new Date().toISOString(),
      action: {
        type: 'Completed',
        data: {
          conclusion: 'Success',
          runner: null,
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    }

    await sendWS(
      page,
      makeJobSeqEvent(12, {
        jobData: completedJobEnvelope,
        poolStatsAfter: [],
      }),
    )

    // Assert the pool indicator is gone
    await expect(poolIndicator).not.toBeVisible()
  })
})
