import type { JobEventEnvelope } from '../src/lib/types/generated/JobEventEnvelope'
import { expect, test } from './lib/fixtures'
import { makeJobCommittedEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

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

    // Step 1: Dispatch a Queued Job event (no runner assigned yet)
    // Pool is derived from the job: labels.join(', ') = 'ubuntu-latest' since groupName is null
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

    await sendWS(page, makeJobCommittedEvent(10, { jobData: queuedJobEnvelope }))

    // Pool indicator shows 'ubuntu-latest' (no groupName yet — derived from labels)
    const queuedPoolIndicator = page.getByTestId('runner-pool-ubuntu-latest')
    await expect(queuedPoolIndicator).toBeVisible()
    await expect(queuedPoolIndicator).toContainText(/\+1 queued/i)

    // Step 2: Dispatch InProgress event with runner that has groupName='GitHub Actions'
    // Pool display name switches to groupName once the runner is assigned
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
          runner: {
            id: 1n,
            name: 'runner-1',
            groupId: 0n,
            groupName: 'GitHub Actions',
          },
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    }

    await sendWS(page, makeJobCommittedEvent(11, { jobData: inProgressJobEnvelope }))

    // Pool indicator now shows 'GitHub Actions' (groupName takes over as display name)
    const runningPoolIndicator = page.getByTestId('runner-pool-GitHub Actions')
    await expect(runningPoolIndicator).toBeVisible()
    // queued badge gone, running count shows
    await expect(runningPoolIndicator).toContainText(/1/)
    const queuedBadge = runningPoolIndicator.getByText(/queued/)
    await expect(queuedBadge).not.toBeVisible()

    // Step 3: Dispatch Completed event — pool should disappear entirely
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
          runner: {
            id: 1n,
            name: 'runner-1',
            groupId: 0n,
            groupName: 'GitHub Actions',
          },
          labels: ['ubuntu-latest'],
          steps: [],
        },
      },
    }

    await sendWS(page, makeJobCommittedEvent(12, { jobData: completedJobEnvelope }))

    // Pool indicator is gone — no active jobs remain
    await expect(runningPoolIndicator).not.toBeVisible()
  })
})
