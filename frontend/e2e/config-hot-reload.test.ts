import type { Job } from '$lib/types/generated/Job'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E for hot-reload runner_pools (issue #172).
 *
 * Mocks a snapshot with one pool at running=3/capacity=10, then dispatches
 * a `ConfigUpdate` WireFrame raising capacity to 20. The CapacityBar must
 * re-render to `3/20` without a page reload. A subsequent
 * `ConfigReloadError` frame must surface the `ConfigReloadErrorBanner`
 * with the backend's reason text, dismissible via the close button, while
 * keeping the dashboard intact (issue #203).
 */

function makeRun(id: number): WorkflowRun {
  return {
    id: BigInt(id),
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'test commit',
    event: 'push',
    displayTitle: `Run ${id}`,
    status: 'InProgress',
    conclusion: null,
    htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${id}`,
    createdAt: '2026-04-17T09:59:00Z',
    runStartedAt: '2026-04-17T09:59:30Z',
    updatedAt: '2026-04-17T09:59:55Z',
    runAttempt: 1,
  }
}

function makeRunningJob(jobId: number, runId: number, labels: string[]): Job {
  return {
    id: BigInt(jobId),
    runId: BigInt(runId),
    name: `job-${jobId}`,
    status: 'InProgress',
    conclusion: null,
    labels,
    runner: {
      id: BigInt(jobId * 100),
      name: `runner-${jobId}`,
      groupName: 'self-hosted-pool',
    },
    steps: [],
    createdAt: '2026-04-17T09:59:00Z',
    startedAt: '2026-04-17T09:59:30Z',
    completedAt: null,
  }
}

function snapshot(running: number, capacity: number): StateSnapshot {
  const jobs: Job[] = []
  for (let i = 0; i < running; i++) {
    jobs.push(makeRunningJob(i + 1, 1, ['self-hosted', 'linux', 'x64']))
  }
  return {
    lastSeq: 1n,
    runs: [makeRun(1)],
    jobs,
    runnerPoolCapacities: [{ labels: ['linux', 'self-hosted', 'x64'], capacity }],
    displayTtlSeconds: 0,
  }
}

test.describe('Config hot reload', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  test('ConfigUpdate WireFrame re-renders CapacityBar without page reload', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(snapshot(3, 10), bigintReplacer),
      })
    })

    await page.goto('/')

    const meter = page.getByRole('meter', { name: /pool capacity/i })
    await expect(meter).toBeVisible()
    await expect(meter).toHaveAttribute('aria-valuemax', '10')
    await expect(page.getByText('3/10')).toBeVisible()

    // Send the ConfigUpdate WireFrame via the dispatcher bridge. The
    // dispatcher's outer-kind switch routes ConfigUpdate to
    // `runStore.applyConfigUpdate`, which atomically replaces the capacity
    // slice; Svelte reactivity re-renders CapacityBar.
    await sendWS(
      page,
      JSON.stringify({
        kind: 'ConfigUpdate',
        runnerPoolCapacities: [{ labels: ['linux', 'self-hosted', 'x64'], capacity: 20 }],
        displayTtlSeconds: 0,
      }),
    )

    await expect(meter).toHaveAttribute('aria-valuemax', '20')
    await expect(page.getByText('3/20')).toBeVisible()
  })

  test('ConfigReloadError WireFrame surfaces dismissible banner without breaking the dashboard', async ({
    page,
  }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(snapshot(3, 10), bigintReplacer),
      })
    })

    await page.goto('/')

    const meter = page.getByRole('meter', { name: /pool capacity/i })
    await expect(meter).toBeVisible()

    await sendWS(
      page,
      JSON.stringify({
        kind: 'ConfigReloadError',
        reason: 'capacity must be >= 1',
      }),
    )

    // The ConfigReloadErrorBanner appears with the backend's reason text.
    const banner = page.getByRole('status', { name: /config reload/i })
    await expect(banner).toBeVisible()
    await expect(banner).toContainText('capacity must be >= 1')

    // The dashboard remains intact behind the banner.
    await expect(meter).toBeVisible()
    await expect(page.getByText('3/10')).toBeVisible()

    // Clicking the Dismiss button hides the banner.
    await banner.getByRole('button', { name: /dismiss/i }).click()
    await expect(banner).toBeHidden()
  })
})
