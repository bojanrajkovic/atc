import type { Job } from '$lib/types/generated/Job'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { expect, test } from './lib/fixtures'
import { bigintReplacer, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

/**
 * E2E for the operator-declared runner-pool capacity feature (issue #16).
 *
 * Seeds `/v1/state` with a fixed `runs` + `jobs` set plus a populated
 * `runnerPoolCapacities` block. The frontend's `computePoolStats` merge
 * should populate `RunnerPoolStats.total`, `CapacityBar.svelte` should
 * appear, and the color band should reflect utilization (`--success`
 * <70%, `--running` 70–99%, `--failed` >=100%).
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
      groupId: 42n,
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
    accessibleReposCount: 0n,
  }
}

test.describe('Runner pool capacity', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  test('renders 3/10 with the success color when utilization is below 70%', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(snapshot(3, 10), bigintReplacer),
      })
    })

    await page.goto('/')

    const meter = page.getByRole('meter', { name: /pool capacity/i })
    await expect(meter).toBeVisible()
    await expect(meter).toHaveAttribute('aria-valuenow', '3')
    await expect(meter).toHaveAttribute('aria-valuemax', '10')

    const fill = meter.locator('div').first()
    await expect(fill).toHaveAttribute('style', /var\(--success\)/)

    // The runner bar shows the count text
    await expect(page.getByText('3/10')).toBeVisible()
  })

  test('renders amber color when utilization is between 70% and 99%', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(snapshot(8, 10), bigintReplacer),
      })
    })

    await page.goto('/')

    const meter = page.getByRole('meter', { name: /pool capacity/i })
    await expect(meter).toBeVisible()
    const fill = meter.locator('div').first()
    await expect(fill).toHaveAttribute('style', /var\(--running\)/)
    await expect(page.getByText('8/10')).toBeVisible()
  })

  test('renders failed color clamped at 100% width when running > capacity', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(snapshot(12, 10), bigintReplacer),
      })
    })

    await page.goto('/')

    const meter = page.getByRole('meter', { name: /pool capacity/i })
    await expect(meter).toBeVisible()
    const fill = meter.locator('div').first()
    await expect(fill).toHaveAttribute('style', /var\(--failed\)/)
    // Width is clamped to 100% even though running > capacity
    await expect(fill).toHaveAttribute('style', /width: 100%/)
  })

  test('pools without a declaration render no CapacityBar', async ({ page }) => {
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify(
          {
            lastSeq: 1n,
            runs: [makeRun(1)],
            jobs: [makeRunningJob(1, 1, ['ubuntu-latest'])],
            // Capacity declared for a different label set
            runnerPoolCapacities: [{ labels: ['self-hosted'], capacity: 5 }],
            accessibleReposCount: 0n,
          } satisfies StateSnapshot,
          bigintReplacer,
        ),
      })
    })

    await page.goto('/')

    // The job's pool ('ubuntu-latest') has no capacity declaration → no meter.
    await expect(page.getByText('self-hosted-pool')).toBeVisible()
    await expect(page.getByRole('meter', { name: /pool capacity/i })).not.toBeVisible()
  })
})
