import { liveRegion } from '$lib/aria/live-region.svelte'
import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

export class ConnectionManager {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private abortController: AbortController | null = null
  private baseUrl: string
  private snapshotSeq: bigint = 0n
  private preConnectBuffer: SeqEvent[] = []
  private connected = false

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl
  }

  /** JSON reviver to convert numeric fields to bigint for known i64/u64 fields */
  private jsonReviver(key: string, value: unknown): unknown {
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

  async connect(): Promise<void> {
    // Abort any prior in-flight connect
    this.abortController?.abort()
    this.abortController = new AbortController()
    const { signal } = this.abortController

    connectionStore.status = 'connecting'
    this.connected = false
    this.preConnectBuffer = []

    // Step 1: Open WebSocket FIRST (WS-first protocol)
    const wsUrl = `${this.baseUrl.replace(/^http/, 'ws')}/v1/ws`
    this.ws = new WebSocket(wsUrl)

    this.ws.onmessage = (event) => {
      const seqEvent: SeqEvent = JSON.parse(event.data, (key, value) =>
        this.jsonReviver(key, value),
      )
      connectionStore.recordEvent()

      if (this.connected) {
        eventDispatcher.dispatch(seqEvent)
      } else {
        this.preConnectBuffer.push(seqEvent)
      }
    }

    this.ws.onclose = () => this.handleDisconnect()
    this.ws.onerror = () => {} // onclose fires after onerror

    // Step 2: Wait for WS to open
    //
    // The abort listener is the third settle path alongside onopen/onclose.
    // It exists for the manual reconnect case: reconnect() nulls this.ws.onclose
    // before close()-ing the socket, so the rejector below cannot fire, and a
    // real browser will not fire onopen on a closed socket either. Without the
    // abort listener the Promise stranded, leaking the async frame.
    await new Promise<void>((resolve, reject) => {
      if (!this.ws) return reject(new Error('No WebSocket'))
      if (signal.aborted) return reject(new DOMException('Aborted', 'AbortError'))

      const onAbort = () => reject(new DOMException('Aborted', 'AbortError'))
      signal.addEventListener('abort', onAbort, { once: true })

      this.ws.onopen = () => resolve()
      const originalOnclose = this.ws.onclose
      const ws = this.ws
      this.ws.onclose = (e) => {
        reject(new Error('WebSocket closed before open'))
        if (originalOnclose) originalOnclose.call(ws, e)
      }
    })

    // Bail if aborted (destroy() called during WS open wait)
    if (signal.aborted) return

    // Step 3: Fetch state snapshot
    try {
      const res = await fetch(`${this.baseUrl}/v1/state`, { signal })
      if (!res.ok) throw new Error(`State fetch failed: ${res.status}`)
      const text = await res.text()
      const snapshot: StateSnapshot = JSON.parse(text, (key, value) => this.jsonReviver(key, value))

      // Bail if aborted (destroy() called during fetch)
      if (signal.aborted) return

      // Step 4: Drain any stale dispatcher events from prior connection
      eventDispatcher.clear()

      // Step 5: Load snapshot into stores
      runStore.loadSnapshot(snapshot.runs, snapshot.jobs)
      runnerStore.loadPools(snapshot.poolStats)
      this.snapshotSeq = snapshot.seq

      // Step 6: Flush buffered events, discarding stale ones
      // Detach any prior setOnFlush callback so buffered-replay events do not
      // produce announcements (AC6.7: reconnect silence during buffered drain).
      eventDispatcher.setOnFlush(null)
      for (const buffered of this.preConnectBuffer) {
        if (buffered.seq >= this.snapshotSeq) {
          eventDispatcher.dispatch(buffered)
        }
      }
      this.preConnectBuffer = []
      eventDispatcher.flush()
      // Step 6b: Wire the live-region callback AFTER the buffered drain so only
      // subsequent live events produce announcements (AC6.7 deferred wiring).
      eventDispatcher.setOnFlush((events) => liveRegion.observeFlush(events))
      this.connected = true

      // Step 7: Transition to connected
      connectionStore.status = 'connected'
      connectionStore.reconnectAttempt = 0
    } catch {
      // Ignore abort errors — destroy() was called intentionally
      if (signal.aborted) return

      // State fetch failed — close WS and trigger reconnect
      if (this.ws) {
        this.ws.onclose = null
        this.ws.close()
        this.ws = null
      }
      this.handleDisconnect()
    }
  }

  private handleDisconnect(): void {
    this.connected = false
    this.ws = null
    // Detach the live-region callback on disconnect so the next reconnect cycle
    // (snapshot + buffered-drain) runs silently until re-wired (AC6.7).
    eventDispatcher.setOnFlush(null)
    // Also cancel any in-flight burst — observeFlush may have opened a burst
    // whose 200ms debounce timer has not yet fired; without this, closeBurst()
    // would announce a stale summary while the app is reconnecting.
    liveRegion.cancelBurst()
    connectionStore.status = 'reconnecting'

    // Exponential backoff: 1s, 2s, 4s, 8s, ..., capped at 30s
    const delay = Math.min(1000 * 2 ** connectionStore.reconnectAttempt, 30_000)
    connectionStore.reconnectAttempt++

    this.reconnectTimer = setTimeout(() => {
      this.connect().catch(() => {})
    }, delay)
  }

  /**
   * Cancel any pending reconnect timer, reset the backoff counter, close the
   * current WebSocket, and begin a fresh connect cycle. Used by the
   * "Reconnect" command in the command palette via the
   * connectionStore.reconnectRequested trigger.
   */
  reconnect(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    connectionStore.reconnectAttempt = 0
    // Detach onFlush + cancel any pending burst BEFORE closing the WS. We null
    // ws.onclose to skip handleDisconnect (which is on a different path), so
    // without this any RAF batch queued by the prior connection could still
    // flush during the new connect cycle's snapshot-fetch window and announce
    // stale updates through the still-attached onFlush callback.
    eventDispatcher.setOnFlush(null)
    liveRegion.cancelBurst()
    if (this.ws) {
      this.ws.onclose = null
      this.ws.close()
      this.ws = null
    }
    this.connect().catch(() => {})
  }

  destroy(): void {
    // Abort any in-flight connect (fetch, WS open wait)
    this.abortController?.abort()
    this.abortController = null

    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.ws) {
      this.ws.onclose = null
      this.ws.close()
      this.ws = null
    }
    connectionStore.status = 'disconnected'
    this.connected = false
  }
}
