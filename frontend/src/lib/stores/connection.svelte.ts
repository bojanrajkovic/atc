type ConnectionStatus =
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'disconnected'
  | 'unauthenticated'

export type AuthReason = 'auth_required' | 'stale_authorization'

const STALE_THRESHOLD_MS = 30_000
const STALE_CHECK_INTERVAL_MS = 5_000
const VERSION_MISMATCH_COUNTDOWN_MS = 30_000
const CONFIG_RELOAD_ERROR_AUTO_DISMISS_MS = 60_000

class ConnectionStore {
  status = $state<ConnectionStatus>('disconnected')
  lastEventAt = $state<number | null>(null)
  reconnectAttempt = $state(0)
  reconnectRequested = $state(0)

  // Set by ConnectionManager when a state fetch or WS rejection resolves to a
  // 401 (see connection.ts). Read by the login screen / re-auth UI (#463,
  // #464) to decide between the login screen and the popup-first staleness
  // flow. Cleared by retry().
  authReason = $state<AuthReason | null>(null)

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

  // ConfigReloadError admin alert state (issue #203). Single-slot, last-wins:
  // a fresh ConfigReloadError replaces the visible reason and restarts the
  // 60s wall-clock auto-dismiss timer. Manual dismiss is also supported via
  // dismissConfigReloadError(). The timer lives here on the store (rather
  // than as a component $effect) so tests can drive it without mounting
  // the Svelte component.
  configReloadError = $state<string | null>(null)
  private configReloadErrorTimeout: ReturnType<typeof setTimeout> | null = null

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

  /** ConnectionManager calls this on a 401 (direct or probed) instead of scheduling backoff. */
  enterUnauthenticated(reason: AuthReason): void {
    this.status = 'unauthenticated'
    this.authReason = reason
  }

  /**
   * Re-enters the normal connect path after re-auth. Reuses the existing
   * reconnectRequested signal — ConnectionManager.svelte's effect already
   * calls manager.reconnect() (fresh WS + state fetch) off that counter, so
   * no separate wiring is needed for the post-auth retry.
   */
  retry(): void {
    this.authReason = null
    this.requestReconnect()
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

  /**
   * Record a config-reload-error from the dispatcher (issue #203). Replaces
   * any visible reason, arms a 60-second wall-clock auto-dismiss timer, and
   * cancels any prior pending dismiss so the timer always reflects the most
   * recent error.
   */
  markConfigReloadError(reason: string): void {
    if (this.configReloadErrorTimeout !== null) {
      clearTimeout(this.configReloadErrorTimeout)
    }
    this.configReloadError = reason
    this.configReloadErrorTimeout = setTimeout(() => {
      this.configReloadError = null
      this.configReloadErrorTimeout = null
    }, CONFIG_RELOAD_ERROR_AUTO_DISMISS_MS)
  }

  /** Manual-close target for the ConfigReloadErrorBanner. */
  dismissConfigReloadError(): void {
    if (this.configReloadErrorTimeout !== null) {
      clearTimeout(this.configReloadErrorTimeout)
      this.configReloadErrorTimeout = null
    }
    this.configReloadError = null
  }

  destroy(): void {
    if (this.tickInterval !== null) {
      clearInterval(this.tickInterval)
      this.tickInterval = null
    }
    if (this.configReloadErrorTimeout !== null) {
      clearTimeout(this.configReloadErrorTimeout)
      this.configReloadErrorTimeout = null
    }
  }
}

export const connectionStore = new ConnectionStore()
