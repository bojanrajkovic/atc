import { liveRegion } from '$lib/aria/live-region.svelte'
import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { RunnerPoolCapacity } from '$lib/types/generated/RunnerPoolCapacity'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'
import type { WireFrame } from '$lib/types/generated/WireFrame'

/**
 * Type guard: does the parsed message already carry the outer `kind`
 * discriminator? A `false` return means the payload is the legacy
 * `CommittedEvent` shape (`{seq, event}`) that a pre-this-PR backend would
 * have produced — we normalize those into `WireFrame.Committed` to keep the
 * rolling-deploy window lossless. See the `onmessage` handler below for the
 * caller context.
 */
function isWireFrame(value: unknown): value is WireFrame {
  return typeof value === 'object' && value !== null && 'kind' in value
}

/**
 * Stop scheduling reconnect timers after this many consecutive failed attempts.
 * Cumulative wait under the exponential schedule (1s, 2s, 4s, 8s, 16s, 30s×5)
 * is ~3 minutes; the dashboard then sits in `disconnected` until the user
 * clicks the indicator to retry. Prevents tabs left open against a
 * permanently-gone backend from spinning the connect loop forever.
 */
export const MAX_RECONNECT_ATTEMPTS = 10

export class ConnectionManager {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private abortController: AbortController | null = null
  private baseUrl: string
  private snapshotLastSeq: bigint = 0n
  private preConnectBuffer: CommittedEvent[] = []
  /**
   * Holds the latest pre-snapshot ConfigUpdate, if any.
   *
   * `ConfigUpdate` frames carry the full operator-declared capacity list
   * (not a delta), so a pre-snapshot frame is meaningful only as "latest
   * wins" — buffering a list is pointless. The slot is drained against the
   * snapshot's capacities after `runStore.loadSnapshot` so the snapshot
   * fetch's brief race window with the watcher does not lose a hot-reload.
   * `null` means no pre-snapshot ConfigUpdate observed.
   */
  private pendingConfigUpdate: RunnerPoolCapacity[] | null = null
  private connected = false

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl
  }

  /** JSON reviver to convert numeric fields to bigint for known i64/u64 fields */
  private jsonReviver(key: string, value: unknown): unknown {
    if (
      ['seq', 'lastSeq', 'id', 'runId', 'jobId', 'number'].includes(key) &&
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
    this.pendingConfigUpdate = null

    // Step 1: Open WebSocket FIRST (WS-first protocol)
    const wsUrl = `${this.baseUrl.replace(/^http/, 'ws')}/v1/ws`
    this.ws = new WebSocket(wsUrl)

    this.ws.onmessage = (event) => {
      const parsed: unknown = JSON.parse(event.data, (key, value) => this.jsonReviver(key, value))
      // During a rolling deploy a new frontend bundle may briefly connect to
      // an old backend pod that still sends the legacy `{seq, event}` shape
      // (no outer `kind`). Normalize to a Committed WireFrame so neither the
      // connected-mode dispatcher nor the pre-snapshot switch drops events
      // during the rollout window. The dispatcher's connected-mode entry
      // point has the same shim; this normalization keeps the pre-snapshot
      // buffer in agreement.
      const frame: WireFrame = isWireFrame(parsed)
        ? parsed
        : { kind: 'Committed', ...(parsed as CommittedEvent) }
      connectionStore.recordEvent()

      if (this.connected) {
        eventDispatcher.dispatch(frame)
        return
      }

      // Pre-snapshot phase: route frames to the appropriate buffer.
      switch (frame.kind) {
        case 'Committed':
          this.preConnectBuffer.push({ seq: frame.seq, event: frame.event })
          break
        case 'ConfigUpdate':
          // Latest-wins: full capacity list each time, so overwriting
          // discards stale state without information loss.
          this.pendingConfigUpdate = frame.runnerPoolCapacities
          break
        case 'ConfigReloadError':
          // Pre-snapshot ConfigReloadError frames are informational only and
          // dropped — the next successful reload will repaint state via a
          // ConfigUpdate, and the snapshot rail already carries the current
          // server-side capacities. Surfacing the error pre-snapshot would
          // race with the loading indicator and confuse operators.
          break
        case 'ServerHello':
          // The version check is snapshot-independent — apply now. First
          // ServerHello in a session sets the reference; later mismatches
          // arm the deploy-detected banner.
          connectionStore.observeServerVersion(frame.version)
          break
        case 'GoingAway':
          // Transient, single-shot. Mark the flag so the indicator can
          // render its "Server restarting" tooltip during the gap.
          connectionStore.markGoingAway(frame.reason)
          break
        default: {
          // Unknown kind from a newer backend; ignore silently here (the
          // dispatcher's connected-mode default arm warns once per kind).
        }
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
      runStore.loadSnapshot(snapshot.runs, snapshot.jobs, snapshot.runnerPoolCapacities ?? [])
      this.snapshotLastSeq = snapshot.lastSeq

      // Step 5b: Drain any pre-snapshot ConfigUpdate that arrived between
      // snapshot generation and snapshot fetch. The ConfigUpdate carries the
      // full capacity list, so applying it on top of the snapshot's
      // capacities is a clean replace, not a merge. Apply BEFORE flushing
      // CommittedEvents so derived computations see the latest pool config
      // when the events trigger recomputation.
      if (this.pendingConfigUpdate !== null) {
        runStore.applyConfigUpdate(this.pendingConfigUpdate)
        this.pendingConfigUpdate = null
      }

      // Step 6: Flush buffered events, discarding stale ones
      // Detach any prior setOnFlush callback so buffered-replay events do not
      // produce announcements during reconnect drain.
      eventDispatcher.setOnFlush(null)
      for (const buffered of this.preConnectBuffer) {
        if (buffered.seq > this.snapshotLastSeq) {
          // Re-wrap as a Committed WireFrame so the dispatcher's outer-kind
          // switch routes through the buffered path the same way live frames
          // would.
          eventDispatcher.dispatch({
            kind: 'Committed',
            seq: buffered.seq,
            event: buffered.event,
          })
        }
      }
      this.preConnectBuffer = []
      eventDispatcher.flush()
      // Step 6b: Wire the live-region callback AFTER the buffered drain so only
      // subsequent live events produce announcements.
      eventDispatcher.setOnFlush((events) => liveRegion.observeFlush(events))
      this.connected = true

      // Step 7: Transition to connected. Reset GoingAway flags — the previous
      // close cycle (if any) is complete and we're up against a fresh backend.
      connectionStore.serverGoingAway = false
      connectionStore.goingAwayReason = null
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
    // Abort any in-flight connect cycle before scheduling the reconnect. If
    // the socket closes during `/v1/state` fetch (likely on every redeploy
    // because `GoingAway` arrives right before the close), the prior
    // `connect()` call would otherwise complete its fetch path and set
    // `status = 'connected'` against a dead socket. The fetch chain already
    // bails on `signal.aborted`. See issue #47 AC10.
    this.abortController?.abort()
    // Detach the live-region callback on disconnect so the next reconnect cycle
    // (snapshot + buffered-drain) runs silently until re-wired.
    eventDispatcher.setOnFlush(null)
    // Also cancel any in-flight burst — observeFlush may have opened a burst
    // whose 200ms debounce timer has not yet fired; without this, closeBurst()
    // would announce a stale summary while the app is reconnecting.
    liveRegion.cancelBurst()

    // Give up after the configured attempt cap. The indicator transitions to
    // `disconnected` and the user can re-arm the loop by clicking it (which
    // routes through `connectionStore.requestReconnect()` → manager.reconnect()).
    if (connectionStore.reconnectAttempt >= MAX_RECONNECT_ATTEMPTS) {
      connectionStore.status = 'disconnected'
      return
    }

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

    // Mirror the disconnect/reconnect cleanup: detach onFlush + cancel any
    // pending burst. Without this, an app teardown / HMR / test cleanup that
    // happens while a flush callback or 200ms burst timer is still pending
    // could announce stale state after the manager is gone.
    eventDispatcher.setOnFlush(null)
    liveRegion.cancelBurst()

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
