import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'
import { expect, test } from './lib/fixtures'
import { makeRunEvent, sendWS, WS_MOCK_INIT_SCRIPT } from './lib/ws-mock'

const STATUS_FIXTURE: Array<{
  id: number
  status: 'Queued' | 'InProgress' | 'Completed'
  conclusion:
    | null
    | 'Success'
    | 'Failure'
    | 'Cancelled'
    | 'TimedOut'
    | 'ActionRequired'
    | 'StartupFailure'
    | 'Stale'
    | 'Neutral'
    | 'Skipped'
  srText: string
  expectedColor: string
}> = [
  { id: 1, status: 'Queued', conclusion: null, srText: 'Queued', expectedColor: 'var(--queued)' },
  {
    id: 2,
    status: 'InProgress',
    conclusion: null,
    srText: 'In Progress',
    expectedColor: 'var(--running)',
  },
  {
    id: 3,
    status: 'Completed',
    conclusion: 'Success',
    srText: 'Success',
    expectedColor: 'var(--success)',
  },
  {
    id: 4,
    status: 'Completed',
    conclusion: 'Failure',
    srText: 'Failure',
    expectedColor: 'var(--failed)',
  },
  {
    id: 5,
    status: 'Completed',
    conclusion: 'Cancelled',
    srText: 'Cancelled',
    expectedColor: 'var(--cancelled)',
  },
  {
    id: 6,
    status: 'Completed',
    conclusion: 'TimedOut',
    srText: 'Timed Out',
    expectedColor: 'var(--timed-out)',
  },
  {
    id: 7,
    status: 'Completed',
    conclusion: 'ActionRequired',
    srText: 'Action Required',
    expectedColor: 'var(--action-required)',
  },
  {
    id: 8,
    status: 'Completed',
    conclusion: 'StartupFailure',
    srText: 'Startup Failure',
    expectedColor: 'var(--failed)',
  },
  {
    id: 9,
    status: 'Completed',
    conclusion: 'Stale',
    srText: 'Stale',
    expectedColor: 'var(--neutral)',
  },
  {
    id: 10,
    status: 'Completed',
    conclusion: 'Neutral',
    srText: 'Neutral',
    expectedColor: 'var(--neutral)',
  },
  {
    id: 11,
    status: 'Completed',
    conclusion: 'Skipped',
    srText: 'Skipped',
    expectedColor: 'var(--neutral)',
  },
]

function makeWorkflowRun(f: (typeof STATUS_FIXTURE)[number]): StateSnapshot['runs'][number] {
  return {
    id: f.id as unknown as bigint, // reviver on page side converts numeric -> bigint
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'test',
    event: 'push',
    displayTitle: `CI — run ${f.id}`,
    status: f.status,
    conclusion: f.conclusion,
    htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${f.id}`,
    createdAt: '2026-04-17T09:59:00Z',
    runStartedAt: f.status === 'Queued' ? null : '2026-04-17T09:59:30Z',
    updatedAt: '2026-04-17T10:00:00Z',
  }
}

test.describe('run-cards', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  test('renders cards with correct --status-color and glyph for every status', async ({ page }) => {
    await page.clock.install({ time: new Date('2026-04-17T10:00:00Z').getTime() })

    await page.route('**/v1/state', async (route) => {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          lastSeq: 1 as unknown as bigint,
          runs: STATUS_FIXTURE.map(makeWorkflowRun),
          jobs: [],
          runnerPoolCapacities: [],
          displayTtlSeconds: 0,
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    for (const { id, status, srText, expectedColor } of STATUS_FIXTURE) {
      const card = page.locator(`article[data-run-id="${id}"]`)
      await expect(card).toHaveAttribute('data-status', status)
      await expect(card.getByText(srText, { exact: false })).toHaveCount(1)
      const style = await card.getAttribute('style')
      expect(style).toContain(expectedColor)
    }

    // Visual regression capture.
    await page.screenshot({
      path: 'e2e/screenshots/run-cards-populated.png',
      fullPage: true,
    })
  })

  test('Queued → InProgress transition sets data-status="InProgress"', async ({ page }) => {
    await page.clock.install({ time: new Date('2026-04-17T10:00:00Z').getTime() })

    await page.route('**/v1/state', async (route) => {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          lastSeq: 1 as unknown as bigint,
          runs: [
            {
              id: 42 as unknown as bigint,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/42',
              createdAt: '2026-04-17T09:59:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-17T09:59:00Z',
            },
          ],
          jobs: [],
          runnerPoolCapacities: [],
          displayTtlSeconds: 0,
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })

    const queuedCard = queuedSection.locator('article[data-run-id="42"]')
    await expect(queuedCard).toHaveAttribute('data-status', 'Queued')

    await sendWS(
      page,
      makeRunEvent(2, {
        runId: 42,
        displayTitle: 'CI — main',
        runStartedAt: '2026-04-17T10:00:00Z',
        createdAt: '2026-04-17T09:59:00Z',
        updatedAt: '2026-04-17T10:00:00Z',
        action: { type: 'InProgress' },
      }),
    )

    // Scope to the IN PROGRESS column to avoid transient crossfade ambiguity
    // where both old and new DOM nodes briefly coexist.
    const inProgressCard = inProgressSection.locator('article[data-run-id="42"]')
    await expect(inProgressCard).toHaveAttribute('data-status', 'InProgress')
  })

  test('density toggle hides/restores secondary card content', async ({ page }) => {
    await page.clock.install({ time: new Date('2026-04-17T10:00:00Z').getTime() })

    await page.route('**/v1/state', async (route) => {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          lastSeq: 1 as unknown as bigint,
          runs: [
            {
              id: 1 as unknown as bigint,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'InProgress',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
              createdAt: '2026-04-17T09:59:00Z',
              runStartedAt: '2026-04-17T09:59:30Z',
              updatedAt: '2026-04-17T10:00:00Z',
            },
          ],
          jobs: [
            {
              id: 100 as unknown as bigint,
              runId: 1 as unknown as bigint,
              name: 'build',
              status: 'InProgress',
              conclusion: null,
              runner: {
                id: 1 as unknown as bigint,
                name: 'gh-hosted-1',
                groupName: null,
              },
              labels: ['ubuntu-latest'],
              steps: [],
              createdAt: '2026-04-17T09:59:00Z',
              startedAt: '2026-04-17T09:59:30Z',
              completedAt: null,
            },
          ],
          runnerPoolCapacities: [],
          displayTtlSeconds: 0,
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    const card = page.locator('article[data-run-id="1"]')
    const meta = card.locator('.run-card-meta')
    const progress = card.locator('.run-card-progress')
    const runner = card.locator('.run-card-runner')

    // Default (comfortable): all three visible.
    await expect(meta).toBeVisible()
    await expect(progress).toBeVisible()
    await expect(runner).toBeVisible()

    // Open SettingsPopover, click density toggle.
    await page.getByRole('button', { name: /settings/i }).click()
    await page.locator('button[aria-label="Toggle compact density"]').click()

    // Compact: all three hidden via [data-density="compact"] rules in app.css.
    await expect(meta).toBeHidden()
    await expect(progress).toBeHidden()
    await expect(runner).toBeHidden()

    // Toggle again → restored.
    await page.locator('button[aria-label="Toggle compact density"]').click()
    await expect(meta).toBeVisible()
    await expect(progress).toBeVisible()
    await expect(runner).toBeVisible()
  })

  test('InProgress card duration updates after page.clock.fastForward(1000)', async ({ page }) => {
    const start = new Date('2026-04-17T10:00:00Z').getTime()
    await page.clock.install({ time: start })

    await page.route('**/v1/state', async (route) => {
      await route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          lastSeq: 1 as unknown as bigint,
          runs: [
            {
              id: 1 as unknown as bigint,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'abc123',
              commitMessage: 'test',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'InProgress',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
              createdAt: '2026-04-17T09:59:00Z',
              runStartedAt: '2026-04-17T09:59:55Z',
              updatedAt: '2026-04-17T10:00:00Z',
            },
          ],
          jobs: [],
          runnerPoolCapacities: [],
          displayTtlSeconds: 0,
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    const card = page.locator('article[data-run-id="1"]')
    const durationSpan = card.locator('.run-card-duration')

    // Wait for initial render so the first duration text is stable.
    await expect(durationSpan).toHaveText(/^\d+:\d{2}$/)
    const before = (await durationSpan.textContent())?.trim()

    // Advance the virtual wall-clock by 1 second. uiStore's setInterval
    // callback fires on the virtual clock, updating nowMs, which reactively
    // recomputes the InProgress card's duration text.
    await page.clock.fastForward(1000)

    await expect(durationSpan).not.toHaveText(before ?? '', { timeout: 2000 })
    const after = (await durationSpan.textContent())?.trim()

    expect(before).not.toBe(after)
    expect(before).toMatch(/^\d+:\d{2}$/)
    expect(after).toMatch(/^\d+:\d{2}$/)
  })
})
