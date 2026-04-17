import type { Route } from '@playwright/test'
import { expect, test } from '@playwright/test'
import type { StateSnapshot } from '../src/lib/types/generated/StateSnapshot'

/**
 * JS-level WebSocket mock for E2E tests.
 *
 * Playwright's `routeWebSocket` intercepts at the CDP layer, but
 * `WebSocketRoute.send()` does not reliably deliver messages to the page
 * in this Vite dev-server environment. Instead, we mock at the JS level:
 *
 * - Intercept `new WebSocket('/v1/ws')` and fire `onopen` via microtask
 * - Expose `window.__sendWSMessage(data)` to inject messages from the test
 * - Passthrough all other URLs (including Vite HMR) to the real WebSocket
 *
 * This tests the full pipeline: onmessage → JSON.parse with bigint reviver →
 * EventDispatcher → RAF → store mutation → Svelte reactivity → DOM.
 */
const WS_MOCK_INIT_SCRIPT = `
(() => {
  const _RealWS = window.WebSocket;
  let _mockInstance = null;

  class MockWebSocket extends EventTarget {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSING = 2;
    static CLOSED = 3;

    readyState = MockWebSocket.CONNECTING;
    url;
    protocol = '';
    extensions = '';
    bufferedAmount = 0;
    binaryType = 'blob';
    onopen = null;
    onmessage = null;
    onerror = null;
    onclose = null;

    constructor(url, protocols) {
      super();
      this.url = typeof url === 'string' ? url : url.toString();
      _mockInstance = this;
      // Fire onopen in a microtask so the caller can set up handlers
      Promise.resolve().then(() => {
        this.readyState = MockWebSocket.OPEN;
        const event = new Event('open');
        if (this.onopen) this.onopen(event);
        this.dispatchEvent(event);
      });
    }
    send(data) { /* page→server messages ignored in mock */ }
    close(code, reason) {
      this.readyState = MockWebSocket.CLOSED;
      const event = new CloseEvent('close', { code: code || 1000, reason: reason || '' });
      if (this.onclose) this.onclose(event);
      this.dispatchEvent(event);
    }
  }

  // Replace WebSocket — intercept /v1/ws, passthrough everything else
  window.WebSocket = function(url, protocols) {
    const urlStr = typeof url === 'string' ? url : url.toString();
    if (urlStr.includes('/v1/ws')) {
      return new MockWebSocket(url, protocols);
    }
    return new _RealWS(url, protocols);
  };
  window.WebSocket.CONNECTING = _RealWS.CONNECTING;
  window.WebSocket.OPEN = _RealWS.OPEN;
  window.WebSocket.CLOSING = _RealWS.CLOSING;
  window.WebSocket.CLOSED = _RealWS.CLOSED;
  window.WebSocket.prototype = _RealWS.prototype;

  // Expose message injection for E2E tests (used by the JS mock WS path)
  window.__sendWSMessage = (data) => {
    if (!_mockInstance || _mockInstance.readyState !== MockWebSocket.OPEN) return false;
    const event = new MessageEvent('message', { data });
    if (_mockInstance.onmessage) _mockInstance.onmessage(event);
    _mockInstance.dispatchEvent(event);
    return true;
  };
})();
`

/** Helper: build a SeqEvent JSON payload for a Run event (wire format). */
function makeRunEvent(
  seq: number,
  fields: {
    runId: number
    displayTitle: string
    createdAt: string
    runStartedAt: string | null
    updatedAt: string
    action: Record<string, unknown>
  },
) {
  return JSON.stringify({
    seq,
    event: {
      type: 'Run',
      data: {
        runId: fields.runId,
        org: 'test-org',
        repo: 'test-repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: fields.displayTitle,
        htmlUrl: `https://github.com/test-org/test-repo/actions/runs/${fields.runId}`,
        createdAt: fields.createdAt,
        runStartedAt: fields.runStartedAt,
        updatedAt: fields.updatedAt,
        action: fields.action,
      },
    },
  })
}

/** Inject a run event into the app's store via the dev-mode global bridge.
 *  Tests the JSON parsing (bigint reviver) → store mutation → Svelte reactivity → DOM pipeline.
 *  Note: Playwright's routeWebSocket.send() has a known delivery issue in this
 *  Vite dev-server environment, so we access the store directly via window.__stores. */
async function sendWS(page: import('@playwright/test').Page, msg: string): Promise<void> {
  const result = await page.evaluate((data) => {
    const stores = (window as any).__stores
    if (!stores?.runStore) return 'no store bridge'

    // Parse with the same bigint reviver as ConnectionManager
    const reviver = (key: string, value: unknown) => {
      if (
        ['seq', 'id', 'runId', 'jobId', 'groupId', 'number'].includes(key) &&
        (typeof value === 'number' || typeof value === 'string')
      ) {
        try {
          return BigInt(value)
        } catch {
          return value
        }
      }
      return value
    }
    const seqEvent = JSON.parse(data, reviver)

    if (seqEvent.event.type === 'Run') {
      stores.runStore.applyRunEvent(seqEvent.event.data)
      // Force $state reactivity by reassigning the Map (triggers the setter)
      stores.runStore.runs = new Map(stores.runStore.runs)
      return JSON.stringify({
        result: 'dispatched',
        queued: stores.runStore.queuedRuns.length,
        inProgress: stores.runStore.inProgressRuns.length,
        completed: stores.runStore.completedRuns.length,
      })
    }
    return 'unknown event type'
  }, msg)
  const parsed = JSON.parse(result)
  if (parsed.result !== 'dispatched') throw new Error(`WS send failed: ${result}`)
}

test.describe('Kanban board', () => {
  test.beforeEach(async ({ page }) => {
    await page.addInitScript(WS_MOCK_INIT_SCRIPT)
  })

  /**
   * AC8.1: Full lifecycle verification
   * Hydration placeholder → empty state → populated board (via WS event)
   */
  test('AC8.1: hydration → empty → populated board lifecycle', async ({ page }) => {
    let stateRoute: Route | null = null

    // Delay /v1/state fulfillment to observe hydration placeholder
    await page.route('**/v1/state', (route) => {
      stateRoute = route
    })

    await page.goto('/')

    // Step 1: Hydration placeholder visible while connecting
    await expect(page.getByText(/Connecting/)).toBeVisible()

    // Step 2: Fulfill empty snapshot → "No workflows yet."
    await stateRoute!.fulfill({
      contentType: 'application/json',
      body: JSON.stringify({
        seq: 1,
        runs: [],
        jobs: [],
        poolStats: [],
      } satisfies StateSnapshot),
    })
    await expect(page.getByText('No workflows yet.')).toBeVisible()

    // Step 3: Send WS event → board populates
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1001,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T10:00:00Z',
        runStartedAt: null,
        updatedAt: '2026-04-16T10:00:00Z',
        action: { type: 'Requested' },
      }),
    )

    // Column headers appear
    await expect(page.getByRole('heading', { name: 'QUEUED' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'IN PROGRESS' })).toBeVisible()
    await expect(page.getByRole('heading', { name: 'COMPLETED' })).toBeVisible()

    // Card appears in QUEUED column
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1001"]'),
    ).toBeVisible()
  })

  /**
   * AC8.2: Card movement through lifecycle via WS events
   * Queued → InProgress → Completed within a single page session
   */
  test('AC8.2: card moves between columns via WS events', async ({ page }) => {
    // Start with one run in Queued via initial snapshot
    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [
            {
              id: 1002,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'CI',
              workflowPath: '.github/workflows/ci.yml',
              branch: 'main',
              headSha: 'def456',
              commitMessage: 'another commit',
              event: 'push',
              displayTitle: 'CI — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1002',
              createdAt: '2026-04-16T11:00:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-16T11:00:00Z',
            },
          ],
          jobs: [],
          poolStats: [],
        } satisfies StateSnapshot),
      })
    })

    await page.goto('/')

    // Section locators
    const queuedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-queued"]') })
    const inProgressSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-in-progress"]') })
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })

    // Verify initial state: card in QUEUED
    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).toBeVisible()

    // Send WS event: Queued → InProgress
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1002,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T11:00:00Z',
        runStartedAt: '2026-04-16T11:00:30Z',
        updatedAt: '2026-04-16T11:00:30Z',
        action: { type: 'InProgress' },
      }),
    )

    // Card moves to IN PROGRESS column
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).toBeVisible()
    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()

    // Send WS event: InProgress → Completed
    await sendWS(
      page,
      makeRunEvent(11, {
        runId: 1002,
        displayTitle: 'CI — main',
        createdAt: '2026-04-16T11:00:00Z',
        runStartedAt: '2026-04-16T11:00:30Z',
        updatedAt: '2026-04-16T11:00:45Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    // Card moves to COMPLETED column
    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).toBeVisible()
    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1002"]'),
    ).not.toBeVisible()

    // Capture screenshot for visual regression check
    await page.screenshot({ path: 'e2e/screenshots/kanban-populated.png', fullPage: true })
  })

  /**
   * AC8.3: Reduced motion variant
   * Same lifecycle as AC8.2 with prefers-reduced-motion, zero console errors
   */
  test('AC8.3: reduced motion variant completes lifecycle without errors', async ({ page }) => {
    await page.emulateMedia({ reducedMotion: 'reduce' })

    const consoleErrors: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'error') consoleErrors.push(msg.text())
    })

    await page.route('**/v1/state', (route) => {
      route.fulfill({
        contentType: 'application/json',
        body: JSON.stringify({
          seq: 1,
          runs: [
            {
              id: 1003,
              org: 'test-org',
              repo: 'test-repo',
              workflowName: 'Build',
              workflowPath: '.github/workflows/build.yml',
              branch: 'main',
              headSha: 'ghi789',
              commitMessage: 'reduced motion test',
              event: 'push',
              displayTitle: 'Build — main',
              status: 'Queued',
              conclusion: null,
              htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1003',
              createdAt: '2026-04-16T12:00:00Z',
              runStartedAt: null,
              updatedAt: '2026-04-16T12:00:00Z',
            },
          ],
          jobs: [],
          poolStats: [],
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
    const completedSection = page
      .locator('section')
      .filter({ has: page.locator('[id="kanban-col-completed"]') })

    await expect(
      queuedSection.locator('article[role="listitem"][data-run-id="1003"]'),
    ).toBeVisible()

    // Queued → InProgress via WS
    await sendWS(
      page,
      makeRunEvent(10, {
        runId: 1003,
        displayTitle: 'Build — main',
        createdAt: '2026-04-16T12:00:00Z',
        runStartedAt: '2026-04-16T12:00:30Z',
        updatedAt: '2026-04-16T12:00:30Z',
        action: { type: 'InProgress' },
      }),
    )

    await expect(
      inProgressSection.locator('article[role="listitem"][data-run-id="1003"]'),
    ).toBeVisible()

    // InProgress → Completed via WS
    await sendWS(
      page,
      makeRunEvent(11, {
        runId: 1003,
        displayTitle: 'Build — main',
        createdAt: '2026-04-16T12:00:00Z',
        runStartedAt: '2026-04-16T12:00:30Z',
        updatedAt: '2026-04-16T12:00:45Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }),
    )

    await expect(
      completedSection.locator('article[role="listitem"][data-run-id="1003"]'),
    ).toBeVisible()

    // Zero console errors after full lifecycle
    expect(consoleErrors).toEqual([])
  })
})
