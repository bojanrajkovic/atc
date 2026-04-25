import type { Page } from '@playwright/test'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'

/**
 * JS-level WebSocket mock for E2E tests.
 *
 * Playwright's `routeWebSocket` intercepts at the CDP layer, but
 * `WebSocketRoute.send()` does not reliably deliver messages to the page
 * in Vite dev-server environments (known Playwright bug:
 * microsoft/playwright#34280, #33370). Instead, we mock at the JS level:
 *
 * - Intercept `new WebSocket('/v1/ws')` and fire `onopen` via microtask
 * - Passthrough all other URLs (including Vite HMR) to the real WebSocket
 * - State transitions use window.__stores bridge (set in main.ts) instead
 *
 * This tests: JSON parsing with bigint reviver → store mutation →
 * Svelte reactivity → DOM rendering. The WS transport and EventDispatcher
 * RAF path are NOT exercised (covered by browser-mode tests instead).
 */
export const WS_MOCK_INIT_SCRIPT = `
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

})();
`

/** Helper: build a SeqEvent JSON payload for a Run event (wire format). */
export function makeRunEvent(
  seq: number,
  fields: {
    runId: number
    displayTitle: string
    createdAt: string
    runStartedAt: string | null
    updatedAt: string
    action: Record<string, unknown>
  },
): string {
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

/** Helper: build a SeqEvent JSON payload for a Job event with optional pool stats sidecar. */
export function makeJobSeqEvent(
  seq: number,
  opts: {
    jobData: JobEventEnvelope
    poolStatsAfter: RunnerPoolStats[] | null
  },
): string {
  return JSON.stringify(
    {
      seq,
      event: {
        type: 'Job',
        data: opts.jobData,
      },
      poolStatsAfter: opts.poolStatsAfter,
    },
    (_key, value) => (typeof value === 'bigint' ? value.toString() : value),
  )
}

/** Inject a websocket event into the app's stores via the dev-mode global bridge.
 *  Tests the JSON parsing (bigint reviver) → store mutation → Svelte reactivity → DOM pipeline.
 *  Note: Playwright's routeWebSocket.send() has a known delivery issue in this
 *  Vite dev-server environment, so we access the stores directly via window.__stores. */
export async function sendWS(page: Page, msg: string): Promise<void> {
  const result = await page.evaluate((data) => {
    // biome-ignore lint/suspicious/noExplicitAny: dev-mode global bridge intentionally untyped
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
      return JSON.stringify({
        result: 'dispatched',
        queued: stores.runStore.queuedRuns.length,
        inProgress: stores.runStore.inProgressRuns.length,
        completed: stores.runStore.completedRuns.length,
      })
    }

    if (seqEvent.event.type === 'Job') {
      stores.runStore.applyJobEvent(seqEvent.event.data)
      if (seqEvent.poolStatsAfter != null) {
        stores.runnerStore.loadPools(seqEvent.poolStatsAfter)
      }
      return JSON.stringify({
        result: 'dispatched',
        pools: stores.runnerStore?.pools?.length ?? 0,
      })
    }

    return 'unknown event type'
  }, msg)
  const parsed = JSON.parse(result)
  if (parsed.result !== 'dispatched') throw new Error(`WS send failed: ${result}`)
}
