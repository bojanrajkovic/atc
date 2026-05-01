type ConnectionStatus = 'connecting' | 'connected' | 'reconnecting' | 'disconnected'

const STALE_THRESHOLD_MS = 30_000
const STALE_CHECK_INTERVAL_MS = 5_000

class ConnectionStore {
  status = $state<ConnectionStatus>('disconnected')
  lastEventAt = $state<number | null>(null)
  reconnectAttempt = $state(0)
  reconnectRequested = $state(0)

  // Reactive tick counter — incremented by a setInterval to trigger
  // periodic re-evaluation of isStale. $derived only re-evaluates when
  // its tracked $state dependencies change; Date.now() alone is not
  // reactive, so without this tick the staleness check would freeze
  // at its initial computation.
  private tick = $state(0)
  private tickInterval: ReturnType<typeof setInterval> | null = null

  isStale = $derived(
    // Reference this.tick so $derived re-evaluates when tick changes
    this.tick >= 0 &&
      this.status === 'connected' &&
      this.lastEventAt !== null &&
      Date.now() - this.lastEventAt > STALE_THRESHOLD_MS,
  )

  constructor() {
    this.tickInterval = setInterval(() => {
      this.tick++
    }, STALE_CHECK_INTERVAL_MS)
  }

  recordEvent(): void {
    this.lastEventAt = Date.now()
  }

  requestReconnect(): void {
    this.reconnectRequested += 1
  }

  destroy(): void {
    if (this.tickInterval !== null) {
      clearInterval(this.tickInterval)
      this.tickInterval = null
    }
  }
}

export const connectionStore = new ConnectionStore()
