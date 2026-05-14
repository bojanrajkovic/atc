/**
 * ARIA live-region silence during snapshot replay and buffered-event drain.
 *
 * The ConnectionManager must NOT invoke liveRegion.observeFlush during the
 * snapshot-load + buffered-drain phase on connect or reconnect. The flush
 * callback is re-armed only AFTER the drain completes so that subsequent live
 * events produce announcements normally.
 *
 * Two full reconnect cycles are exercised to verify the deferral is re-armed
 * on every connect, not just the first.
 */
import { HttpResponse, http } from 'msw'
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  MockWebSocket,
  setupConnectionTestServer,
  snapshotToJSON,
} from '$lib/__tests__/connection-test-helpers'
import { liveRegion } from '$lib/aria/live-region.svelte'
import { ConnectionManager } from '$lib/connection'
import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

/** Build a minimal RunEvent SeqEvent (Requested action triggers an announcement). */
function makeRequestedRunEvent(runId: bigint, seq: bigint): SeqEvent {
  return {
    seq,
    event: {
      type: 'Run',
      data: {
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'ci',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'test commit',
        triggerEvent: 'push',
        displayTitle: `Run ${runId}`,
        htmlUrl: `https://github.com/org/repo/actions/runs/${runId}`,
        createdAt: new Date().toISOString(),
        runStartedAt: null,
        updatedAt: new Date().toISOString(),
        action: { type: 'Requested' },
      } as RunEventEnvelope,
    },
  }
}

/** Serialize a SeqEvent to JSON with bigint → string (matching the wire format). */
function serializeEvent(event: SeqEvent): string {
  return JSON.stringify(event, (_key, value) => {
    if (typeof value === 'bigint') return value.toString()
    return value
  })
}

/** Flush the microtask queue several times to let async chains settle. */
async function flushMicrotasks(n = 6): Promise<void> {
  for (let i = 0; i < n; i++) {
    await Promise.resolve()
  }
}

/**
 * Yield to the Node.js event loop so that pending I/O callbacks (MSW HTTP,
 * msw/node intercept) and macro-task microtask chains can drain.
 * Used when a `connect()` promise is not directly accessible.
 */
function yieldToEventLoop(ms = 20): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

describe('ARIA live-region silence during snapshot replay and buffered drain', () => {
  const baseUrl = 'http://localhost:3000'
  const server = setupConnectionTestServer()

  beforeAll(() => {
    server.listen()
    Object.defineProperty(globalThis, 'WebSocket', {
      value: MockWebSocket,
      writable: true,
      configurable: true,
    })
  })

  beforeEach(() => {
    MockWebSocket.clearAll()
  })

  afterEach(() => {
    server.resetHandlers()
    runStore.clear()
    connectionStore.status = 'disconnected'
    connectionStore.reconnectAttempt = 0
    connectionStore.lastEventAt = null
    MockWebSocket.clearAll()
    vi.restoreAllMocks()
  })

  afterAll(() => {
    server.close()
  })

  it('suppresses observeFlush during buffered drain on both first and second connect cycles, fires normally for live events', async () => {
    // Snapshot lastSeq = 5; buffered events use seq > 5 so they survive the filter.
    const snapshotWithSeq: StateSnapshot = {
      lastSeq: 5n,
      runs: [],
      jobs: [],
      runnerPoolCapacities: [],
    }

    // --- Spy on liveRegion.observeFlush ---
    const observeFlushSpy = vi.spyOn(liveRegion, 'observeFlush')

    // =========================================================
    // CYCLE 1: connect → snapshot → buffered drain → live event
    // =========================================================

    // Use a deferred fetch to create the window where the buffered event arrives.
    let resolveStateFetch1: (() => void) | undefined
    const statePromise1 = new Promise<void>((resolve) => {
      resolveStateFetch1 = resolve
    })

    server.use(
      http.get('http://localhost:*/v1/state', async () => {
        await statePromise1
        return HttpResponse.json(snapshotToJSON(snapshotWithSeq))
      }),
    )

    const manager = new ConnectionManager(baseUrl)
    const connectPromise1 = manager.connect()

    // Allow MockWebSocket to fire onopen (microtask queue).
    await flushMicrotasks()

    // Send a buffered event (arrives before state fetch resolves, seq >= snapshot.seq=5).
    const ws1 = MockWebSocket.getLastInstance()
    expect(ws1).toBeDefined()
    ws1!.receiveMessage(serializeEvent(makeRequestedRunEvent(101n, 10n)))

    // Let the receiveMessage microtask settle.
    await flushMicrotasks()

    // Resolve the state fetch — connection.ts drains the buffer with setOnFlush(null).
    resolveStateFetch1!()
    await connectPromise1

    // The drain called eventDispatcher.flush() with setOnFlush=null.
    // observeFlush must NOT have been called yet.
    expect(observeFlushSpy).not.toHaveBeenCalled()

    // Send a normal live event post-connect and flush the dispatcher manually.
    // setOnFlush is now wired to liveRegion.observeFlush, so this MUST fire it.
    const ws1Live = MockWebSocket.getLastInstance()!
    ws1Live.receiveMessage(serializeEvent(makeRequestedRunEvent(102n, 20n)))
    await flushMicrotasks()
    eventDispatcher.flush()

    expect(observeFlushSpy).toHaveBeenCalledTimes(1)

    // =========================================================
    // CYCLE 2: reconnect → snapshot → buffered drain → live event
    //
    // manager.reconnect() cancels the backoff timer, resets the reconnect
    // counter, closes the current WS, and calls connect() synchronously.
    // This exercises the full re-arming path:
    //   handleDisconnect sets setOnFlush(null) on close,
    //   then the next connect() sets setOnFlush(null) again before drain,
    //   and wires it back to liveRegion.observeFlush only after drain.
    // =========================================================

    let resolveStateFetch2: (() => void) | undefined
    const statePromise2 = new Promise<void>((resolve) => {
      resolveStateFetch2 = resolve
    })

    server.use(
      http.get('http://localhost:*/v1/state', async () => {
        await statePromise2
        return HttpResponse.json(snapshotToJSON(snapshotWithSeq))
      }),
    )

    manager.reconnect()

    // Allow the new MockWebSocket to fire onopen.
    await flushMicrotasks()

    // Send a buffered event on the fresh WS connection before state fetch resolves.
    const ws2 = MockWebSocket.getLastInstance()
    expect(ws2).toBeDefined()
    expect(ws2).not.toBe(ws1Live) // Must be a fresh WS instance.
    ws2!.receiveMessage(serializeEvent(makeRequestedRunEvent(201n, 30n)))
    await flushMicrotasks()

    // Resolve the second state fetch and let the async connect() chain finish.
    // The MSW intercept + fetch response involves Node.js I/O callbacks, not just
    // microtasks, so we yield to the event loop after resolving the deferred fetch.
    resolveStateFetch2!()
    await yieldToEventLoop()

    // observeFlush must NOT have been called since the first live-event flush.
    expect(observeFlushSpy).toHaveBeenCalledTimes(1)

    // Send a live event after the second drain — must fire observeFlush.
    const ws2Live = MockWebSocket.getLastInstance()!
    ws2Live.receiveMessage(serializeEvent(makeRequestedRunEvent(202n, 40n)))
    await flushMicrotasks()
    eventDispatcher.flush()

    expect(observeFlushSpy).toHaveBeenCalledTimes(2)

    manager.destroy()
  })

  // -------------------------------------------------------------------------
  // Codex P2: cancel pending live-region burst on disconnect/reconnect.
  //
  // Without this, a 200ms burst-debounce timer scheduled by observeFlush right
  // before the WS dropped (or right before reconnect() nulled ws.onclose) would
  // still fire closeBurst() during the new connect cycle and announce a stale
  // summary while the app is reconnecting. AND in the reconnect() path,
  // ws.onclose=null skips handleDisconnect, so any orphan RAF batch queued by
  // the prior connection could still flush through the still-attached onFlush
  // callback during the snapshot-fetch window.
  // -------------------------------------------------------------------------

  it('handleDisconnect detaches onFlush AND cancels pending live-region burst', async () => {
    const cancelBurstSpy = vi.spyOn(liveRegion, 'cancelBurst')
    const setOnFlushSpy = vi.spyOn(eventDispatcher, 'setOnFlush')

    server.use(
      http.get('http://localhost:*/v1/state', () =>
        HttpResponse.json(
          snapshotToJSON({ lastSeq: 5n, runs: [], jobs: [], runnerPoolCapacities: [] }),
        ),
      ),
    )

    const manager = new ConnectionManager(baseUrl)
    await manager.connect()
    cancelBurstSpy.mockClear()
    setOnFlushSpy.mockClear()

    // Trigger the natural-disconnect path: simulate the WS dropping by calling
    // close() on the live socket. handleDisconnect runs synchronously inside
    // ws.onclose, so we don't need to await anything async here.
    const ws = MockWebSocket.getLastInstance()!
    ws.close()
    await flushMicrotasks()

    expect(setOnFlushSpy).toHaveBeenCalledWith(null)
    expect(cancelBurstSpy).toHaveBeenCalledTimes(1)

    manager.destroy()
  })

  it('destroy() detaches onFlush AND cancels pending live-region burst', async () => {
    const cancelBurstSpy = vi.spyOn(liveRegion, 'cancelBurst')
    const setOnFlushSpy = vi.spyOn(eventDispatcher, 'setOnFlush')

    server.use(
      http.get('http://localhost:*/v1/state', () =>
        HttpResponse.json(
          snapshotToJSON({ lastSeq: 5n, runs: [], jobs: [], runnerPoolCapacities: [] }),
        ),
      ),
    )

    const manager = new ConnectionManager(baseUrl)
    await manager.connect()
    cancelBurstSpy.mockClear()
    setOnFlushSpy.mockClear()

    // destroy() nulls ws.onclose (skipping handleDisconnect) so cleanup must
    // be explicit, mirroring reconnect(). HMR / app teardown / test cleanup
    // during in-flight events would otherwise leak stale announcements.
    manager.destroy()

    expect(setOnFlushSpy).toHaveBeenCalledWith(null)
    expect(cancelBurstSpy).toHaveBeenCalledTimes(1)
  })

  it('reconnect() detaches onFlush AND cancels pending burst BEFORE closing the WS', async () => {
    const cancelBurstSpy = vi.spyOn(liveRegion, 'cancelBurst')
    const setOnFlushSpy = vi.spyOn(eventDispatcher, 'setOnFlush')

    server.use(
      http.get('http://localhost:*/v1/state', () =>
        HttpResponse.json(
          snapshotToJSON({ lastSeq: 5n, runs: [], jobs: [], runnerPoolCapacities: [] }),
        ),
      ),
    )

    const manager = new ConnectionManager(baseUrl)
    await manager.connect()

    // Capture the live WS so we can verify our cleanup ran BEFORE it was closed.
    const wsBeforeReconnect = MockWebSocket.getLastInstance()!
    cancelBurstSpy.mockClear()
    setOnFlushSpy.mockClear()

    // Stub close() to capture relative invocation order vs cancelBurst/setOnFlush.
    const callLog: string[] = []
    cancelBurstSpy.mockImplementation(() => {
      callLog.push('cancelBurst')
    })
    setOnFlushSpy.mockImplementation((cb) => {
      callLog.push(`setOnFlush(${cb === null ? 'null' : 'callback'})`)
    })
    const realClose = wsBeforeReconnect.close.bind(wsBeforeReconnect)
    vi.spyOn(wsBeforeReconnect, 'close').mockImplementation(() => {
      callLog.push('ws.close')
      realClose()
    })

    manager.reconnect()
    await flushMicrotasks()

    // Both cleanup hooks ran AT reconnect()-start (positions 0 and 1), BEFORE
    // ws.close (position 2). Subsequent setOnFlush calls inside the new
    // connect() cycle are fine — what matters is the prior connection's
    // callback is detached BEFORE its events can flush.
    const cancelIdx = callLog.indexOf('cancelBurst')
    const detachIdx = callLog.indexOf('setOnFlush(null)')
    const closeIdx = callLog.indexOf('ws.close')
    expect(cancelIdx).toBeGreaterThanOrEqual(0)
    expect(detachIdx).toBeGreaterThanOrEqual(0)
    expect(closeIdx).toBeGreaterThan(cancelIdx)
    expect(closeIdx).toBeGreaterThan(detachIdx)

    manager.destroy()
  })
})
