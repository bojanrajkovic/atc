import { eventDispatcher } from '$lib/dispatcher'
import { connectionStore } from '$lib/stores/connection.svelte'
import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

export class ConnectionManager {
  private ws: WebSocket | null = null
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null
  private baseUrl: string
  private snapshotSeq: bigint = 0n
  private preConnectBuffer: SeqEvent[] = []
  private connected = false

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl
  }

  /** JSON reviver to convert numeric fields to bigint for known i64/u64 fields */
  private jsonReviver(key: string, value: unknown): unknown {
    if (['seq', 'id', 'runId', 'jobId'].includes(key) && (typeof value === 'number' || typeof value === 'string')) {
      try {
        return BigInt(value)
      } catch {
        return value
      }
    }
    return value
  }

  async connect(): Promise<void> {
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
        // Post-connect: dispatch normally
        eventDispatcher.dispatch(seqEvent)
      } else {
        // Buffer events during state fetch
        this.preConnectBuffer.push(seqEvent)
      }
    }

    this.ws.onclose = () => this.handleDisconnect()
    this.ws.onerror = () => {} // onclose fires after onerror

    // Step 2: Wait for WS to open
    await new Promise<void>((resolve, reject) => {
      if (!this.ws) return reject(new Error('No WebSocket'))
      this.ws.onopen = () => resolve()
      // If close fires before open, reject
      const originalOnclose = this.ws.onclose
      const ws = this.ws
      this.ws.onclose = (e) => {
        reject(new Error('WebSocket closed before open'))
        if (originalOnclose) originalOnclose.call(ws, e)
      }
    })

    // Step 3: Fetch state snapshot
    try {
      const res = await fetch(`${this.baseUrl}/v1/state`)
      if (!res.ok) throw new Error(`State fetch failed: ${res.status}`)
      const text = await res.text()
      const snapshot: StateSnapshot = JSON.parse(text, (key, value) => this.jsonReviver(key, value))

      // Step 4: Load snapshot into stores
      runStore.loadSnapshot(snapshot.runs, snapshot.jobs)
      runnerStore.loadPools(snapshot.poolStats)
      this.snapshotSeq = snapshot.seq

      // Step 5: Flush buffered events, discarding stale ones
      for (const buffered of this.preConnectBuffer) {
        if (buffered.seq >= this.snapshotSeq) {
          eventDispatcher.dispatch(buffered)
        }
      }
      this.preConnectBuffer = []
      this.connected = true

      // Step 6: Transition to connected
      connectionStore.status = 'connected'
      connectionStore.reconnectAttempt = 0
    } catch {
      // State fetch failed — close WS and let onclose trigger reconnect.
      // Nullify onclose first to prevent double-reconnect, then call
      // handleDisconnect once explicitly.
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
    connectionStore.status = 'reconnecting'

    // Exponential backoff: 1s, 2s, 4s, 8s, ..., capped at 30s
    const delay = Math.min(1000 * 2 ** connectionStore.reconnectAttempt, 30_000)
    connectionStore.reconnectAttempt++

    this.reconnectTimer = setTimeout(() => {
      this.connect()
    }, delay)
  }

  destroy(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer)
      this.reconnectTimer = null
    }
    if (this.ws) {
      this.ws.onclose = null // Prevent reconnect on intentional close
      this.ws.close()
      this.ws = null
    }
    connectionStore.status = 'disconnected'
    this.connected = false
  }
}
