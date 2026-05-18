type ConnectionStatus = 'connecting' | 'connected' | 'reconnecting' | 'disconnected'

const STALE_THRESHOLD_MS = 30_000
const STALE_CHECK_INTERVAL_MS = 5_000
const VERSION_MISMATCH_COUNTDOWN_MS = 30_000

class ConnectionStore {
  status = $state<ConnectionStatus>('disconnected')
  lastEventAt = $state<number | null>(null)
  reconnectAttempt = $state(0)
  reconnectRequested = $state(0)

  // ServerHello session-reference state (issue #47). The first ServerHello
  // version observed in a tab session becomes the reference; any subsequent
  // ServerHello with a different version arms a 30-second countdown that
  // ends in window.location.reload(). In-memory only — a refresh wipes the
  // store, and the new bundle's first ServerHello matches the running
  // backend, so no banner.
  serverVersionReference = $state<string | null>(null)
  serverVersionMismatch = $state<string | null>(null)
  serverReloadAt = $state<number | null>(null)

  // GoingAway envelope state (issue #47). Set by markGoingAway() when the
  // backend emits a WireFrame::GoingAway just before Close(1001). Cleared on
  // the next successful 'connected' transition. Informational metadata that
  // lets the ConnectionIndicator render a tailored "Server restarting"
  // tooltip during the close + reconnect gap.
  serverGoingAway = $state(false)
  goingAwayReason = $state<string | null>(null)

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

  /**
   * Apply a ServerHello.version observation. Called by both the dispatcher
   * and the ConnectionManager's pre-snapshot switch. Session-reference rules:
   *
   * 1. First observation in a session sets `serverVersionReference` and is
   *    otherwise a no-op — no banner on first connect.
   * 2. Observing the reference version a second time (reconnect to the same
   *    backend pod) is a no-op.
   * 3. Observing a different version arms the banner with a 30-second
   *    countdown (`serverReloadAt`).
   * 4. Observing the same MISMATCHED version repeatedly does not rearm — a
   *    user clicking around during the countdown shouldn't reset it.
   * 5. Observing a NEW mismatched version (a third distinct version)
   *    refreshes `serverVersionMismatch` and resets the deadline.
   * 6. After a mismatch is armed, briefly reconnecting to the reference
   *    keeps the countdown running. We're already committed to refreshing.
   */
  observeServerVersion(version: string): void {
    if (this.serverVersionReference === null) {
      this.serverVersionReference = version
      return
    }
    if (version === this.serverVersionReference) {
      return
    }
    if (this.serverVersionMismatch !== version) {
      this.serverVersionMismatch = version
      this.serverReloadAt = Date.now() + VERSION_MISMATCH_COUNTDOWN_MS
    }
  }

  /** GoingAway envelope receipt. */
  markGoingAway(reason: string): void {
    this.serverGoingAway = true
    this.goingAwayReason = reason
  }

  /** Hard-reload the tab. Banner click target and the auto-reload at zero. */
  refreshNow(): void {
    window.location.reload()
  }

  destroy(): void {
    if (this.tickInterval !== null) {
      clearInterval(this.tickInterval)
      this.tickInterval = null
    }
  }
}

export const connectionStore = new ConnectionStore()
