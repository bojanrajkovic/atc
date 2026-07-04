import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { connectionStore } from './connection.svelte'

describe('ConnectionStore', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })

  afterEach(() => {
    connectionStore.destroy()
    vi.useRealTimers()
  })

  describe('Staleness detection', () => {
    it('should not be stale when disconnected even with old lastEventAt', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'disconnected'
      connectionStore.lastEventAt = now - 40_000 // 40 seconds ago

      // Advance time to trigger tick interval
      vi.advanceTimersByTime(5_000)

      // isStale should be false because status is not 'connected'
      expect(connectionStore.isStale).toBe(false)
    })

    it('should not be stale when connected but no event received yet', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = null

      // Advance time to trigger tick interval
      vi.advanceTimersByTime(5_000)

      // isStale should be false because lastEventAt is null
      expect(connectionStore.isStale).toBe(false)
    })

    it('should not be stale when connected and event is recent', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = now - 10_000 // 10 seconds ago

      // Advance time to trigger tick interval
      vi.advanceTimersByTime(5_000)

      // isStale should be false because only 10 seconds have passed
      expect(connectionStore.isStale).toBe(false)
    })

    it('should be stale when connected and >30 seconds without event', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = now - 31_000 // 31 seconds ago

      // Advance time to trigger tick interval (forces re-evaluation)
      vi.advanceTimersByTime(5_000)

      // isStale should be true because >30 seconds have passed
      expect(connectionStore.isStale).toBe(true)
    })

    it('should become not stale when recordEvent is called', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = now - 31_000 // 31 seconds ago

      // Verify it's stale
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(true)

      // Record a new event at current time
      connectionStore.recordEvent()

      // Immediately check (before next tick), isStale should reflect new time
      expect(connectionStore.isStale).toBe(false)
    })

    it('should transition from not stale to stale as time passes', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.recordEvent()

      // Advance time to trigger tick interval (5s)
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(false)

      // Advance more time to just before stale threshold (25 seconds total)
      vi.advanceTimersByTime(20_000)
      expect(connectionStore.isStale).toBe(false)

      // Advance past threshold (31 seconds total from event)
      vi.advanceTimersByTime(6_000)
      expect(connectionStore.isStale).toBe(true)
    })

    it('should only be stale when all three conditions are met', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      // Test: connected=true, lastEventAt≠null, >30s elapsed
      connectionStore.status = 'connected'
      connectionStore.lastEventAt = now - 31_000

      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(true)

      // Change status to reconnecting (not connected)
      connectionStore.status = 'reconnecting'
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(false)

      // Back to connected
      connectionStore.status = 'connected'
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(true)

      // Set lastEventAt to null
      connectionStore.lastEventAt = null
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(false)

      // Set lastEventAt to recent time (within last 30s)
      connectionStore.lastEventAt = Date.now()
      vi.advanceTimersByTime(5_000)
      expect(connectionStore.isStale).toBe(false)
    })

    it('should correctly distinguish between 30s and 31s elapsed', () => {
      const baseTime = 1000
      vi.setSystemTime(baseTime)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = baseTime - 30_000 // Event at baseTime - 30000

      // Don't advance time yet - check at exact 30 seconds
      expect(connectionStore.isStale).toBe(false)

      // Now move time forward to make it 31 seconds
      vi.advanceTimersByTime(1)
      expect(connectionStore.isStale).toBe(true)
    })

    it('should handle reconnectAttempt field independently of staleness', () => {
      const now = Date.now()
      vi.setSystemTime(now)

      connectionStore.status = 'connected'
      connectionStore.lastEventAt = now - 31_000
      connectionStore.reconnectAttempt = 5

      vi.advanceTimersByTime(5_000)

      expect(connectionStore.isStale).toBe(true)
      expect(connectionStore.reconnectAttempt).toBe(5) // Not affected by stale calculation
    })
  })

  // Additional behavior tests
  describe('recordEvent behavior', () => {
    it('should update lastEventAt to current time', () => {
      const time1 = 1000
      vi.setSystemTime(time1)

      connectionStore.recordEvent()
      expect(connectionStore.lastEventAt).toBe(time1)

      vi.setSystemTime(time1 + 5000)
      connectionStore.recordEvent()
      expect(connectionStore.lastEventAt).toBe(time1 + 5000)
    })
  })

  describe('destroy behavior', () => {
    it('should not error when destroy is called multiple times', () => {
      // Verify calling destroy multiple times doesn't error
      expect(() => {
        connectionStore.destroy()
        connectionStore.destroy()
      }).not.toThrow()
    })
  })

  describe('initialization', () => {
    it('should initialize with default values', () => {
      // Note: connectionStore is a singleton, so we test the defaults
      // through fresh state after clear (not applicable for this store)
      expect(connectionStore.status).toBeDefined()
      expect(
        connectionStore.lastEventAt === null || typeof connectionStore.lastEventAt === 'number',
      ).toBe(true)
      expect(typeof connectionStore.reconnectAttempt).toBe('number')
    })
  })

  describe('requestReconnect behavior', () => {
    it('should initialize reconnectRequested at 0', () => {
      expect(connectionStore.reconnectRequested).toBe(0)
    })

    it('should increment reconnectRequested when requestReconnect is called', () => {
      const initial = connectionStore.reconnectRequested
      connectionStore.requestReconnect()
      expect(connectionStore.reconnectRequested).toBe(initial + 1)
    })

    it('should maintain monotonic counter across multiple calls', () => {
      const initial = connectionStore.reconnectRequested
      connectionStore.requestReconnect()
      expect(connectionStore.reconnectRequested).toBe(initial + 1)
      connectionStore.requestReconnect()
      expect(connectionStore.reconnectRequested).toBe(initial + 2)
      connectionStore.requestReconnect()
      expect(connectionStore.reconnectRequested).toBe(initial + 3)
    })
  })

  describe('enterUnauthenticated / retry', () => {
    afterEach(() => {
      connectionStore.status = 'disconnected'
      connectionStore.authReason = null
    })

    it('enterUnauthenticated sets status and reason', () => {
      connectionStore.enterUnauthenticated('auth_required')
      expect(connectionStore.status).toBe('unauthenticated')
      expect(connectionStore.authReason).toBe('auth_required')
    })

    it('retry clears the reason and signals a reconnect', () => {
      connectionStore.enterUnauthenticated('stale_authorization')
      const initial = connectionStore.reconnectRequested

      connectionStore.retry()

      expect(connectionStore.authReason).toBe(null)
      expect(connectionStore.reconnectRequested).toBe(initial + 1)
    })
  })

  describe('observeServerVersion — session-reference handshake (issue #47)', () => {
    beforeEach(() => {
      // Reset issue #47 fields between tests.
      connectionStore.serverVersionReference = null
      connectionStore.serverVersionMismatch = null
      connectionStore.serverReloadAt = null
    })

    it('first observation in a session sets `serverVersionReference` and does not arm the banner', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.observeServerVersion('v1.0.0')

      expect(connectionStore.serverVersionReference).toBe('v1.0.0')
      expect(connectionStore.serverVersionMismatch).toBeNull()
      expect(connectionStore.serverReloadAt).toBeNull()
    })

    it('observing the reference version a second time is a no-op (reconnect to same backend)', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.observeServerVersion('v1.0.0')
      vi.advanceTimersByTime(10_000)
      connectionStore.observeServerVersion('v1.0.0')

      expect(connectionStore.serverVersionReference).toBe('v1.0.0')
      expect(connectionStore.serverVersionMismatch).toBeNull()
      expect(connectionStore.serverReloadAt).toBeNull()
    })

    it('a different version arms `serverReloadAt ≈ now + 30_000`', () => {
      const t = 1_000_000
      vi.setSystemTime(t)
      connectionStore.observeServerVersion('v1.0.0')
      connectionStore.observeServerVersion('v1.1.0')

      expect(connectionStore.serverVersionReference).toBe('v1.0.0')
      expect(connectionStore.serverVersionMismatch).toBe('v1.1.0')
      expect(connectionStore.serverReloadAt).toBe(t + 30_000)
    })

    it('observing the SAME mismatched version twice does not rearm the countdown', () => {
      const t = 1_000_000
      vi.setSystemTime(t)
      connectionStore.observeServerVersion('v1.0.0')
      connectionStore.observeServerVersion('v1.1.0')
      const firstDeadline = connectionStore.serverReloadAt

      vi.advanceTimersByTime(5_000)
      connectionStore.observeServerVersion('v1.1.0')
      expect(connectionStore.serverReloadAt).toBe(firstDeadline)
    })

    it('observing a NEW mismatched version updates `serverVersionMismatch` and resets `serverReloadAt`', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.observeServerVersion('v1.0.0')
      connectionStore.observeServerVersion('v1.1.0')
      const firstDeadline = connectionStore.serverReloadAt

      vi.advanceTimersByTime(5_000)
      connectionStore.observeServerVersion('v1.2.0')

      expect(connectionStore.serverVersionMismatch).toBe('v1.2.0')
      expect(connectionStore.serverReloadAt).not.toBe(firstDeadline)
      expect(connectionStore.serverReloadAt).toBe(Date.now() + 30_000)
    })

    it('reconnecting to the reference version after a mismatch keeps the countdown armed', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.observeServerVersion('v1.0.0')
      connectionStore.observeServerVersion('v1.1.0')
      const deadlineBefore = connectionStore.serverReloadAt

      // Brief reconnect to the reference pod during a rolling deploy.
      connectionStore.observeServerVersion('v1.0.0')

      expect(connectionStore.serverVersionMismatch).toBe('v1.1.0')
      expect(connectionStore.serverReloadAt).toBe(deadlineBefore)
    })
  })

  describe('markGoingAway — GoingAway envelope (issue #47)', () => {
    beforeEach(() => {
      connectionStore.serverGoingAway = false
      connectionStore.goingAwayReason = null
    })

    it('sets `serverGoingAway` and `goingAwayReason`', () => {
      connectionStore.markGoingAway('server shutdown')
      expect(connectionStore.serverGoingAway).toBe(true)
      expect(connectionStore.goingAwayReason).toBe('server shutdown')
    })
  })

  describe('refreshNow — auto-reload (issue #47)', () => {
    it('invokes window.location.reload', () => {
      const reload = vi.fn()
      const originalLocation = window.location
      Object.defineProperty(window, 'location', {
        configurable: true,
        value: { ...originalLocation, reload },
      })
      try {
        connectionStore.refreshNow()
        expect(reload).toHaveBeenCalledOnce()
      } finally {
        Object.defineProperty(window, 'location', {
          configurable: true,
          value: originalLocation,
        })
      }
    })
  })

  describe('markConfigReloadError — admin alert banner (issue #203)', () => {
    beforeEach(() => {
      connectionStore.dismissConfigReloadError()
    })

    afterEach(() => {
      connectionStore.dismissConfigReloadError()
    })

    it('sets configReloadError to the reason string', () => {
      connectionStore.markConfigReloadError('missing key X')
      expect(connectionStore.configReloadError).toBe('missing key X')
    })

    it('replaces the visible reason on a second call (single-slot, last-wins)', () => {
      connectionStore.markConfigReloadError('first reason')
      expect(connectionStore.configReloadError).toBe('first reason')
      connectionStore.markConfigReloadError('second reason')
      expect(connectionStore.configReloadError).toBe('second reason')
    })

    it('auto-dismisses 60 seconds after markConfigReloadError', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.markConfigReloadError('boom')
      expect(connectionStore.configReloadError).toBe('boom')

      // Advance just before the boundary — still visible.
      vi.advanceTimersByTime(59_999)
      expect(connectionStore.configReloadError).toBe('boom')

      // Cross the 60s boundary — cleared.
      vi.advanceTimersByTime(1)
      expect(connectionStore.configReloadError).toBeNull()
    })

    it('a second markConfigReloadError mid-display restarts the 60s timer', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.markConfigReloadError('first')

      // 30 seconds in, a fresh error arrives.
      vi.advanceTimersByTime(30_000)
      connectionStore.markConfigReloadError('second')
      expect(connectionStore.configReloadError).toBe('second')

      // 30 seconds after the second mark — still visible (would have
      // expired at the 60s mark from the first call if the timer hadn't
      // reset).
      vi.advanceTimersByTime(30_000)
      expect(connectionStore.configReloadError).toBe('second')

      // 30 more seconds — total 60s after the second mark — cleared.
      vi.advanceTimersByTime(30_000)
      expect(connectionStore.configReloadError).toBeNull()
    })

    it('dismissConfigReloadError clears state and any pending timer', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.markConfigReloadError('boom')
      expect(connectionStore.configReloadError).toBe('boom')

      connectionStore.dismissConfigReloadError()
      expect(connectionStore.configReloadError).toBeNull()

      // Even after advancing past 60s, no spurious clear from a still-armed
      // timeout fires (no-op on already-null state, but the regression we
      // guard is "timer cleared so it can't trip in the future and reset
      // a subsequent error").
      vi.advanceTimersByTime(60_000)
      expect(connectionStore.configReloadError).toBeNull()

      // Confirm: after dismiss, a fresh markConfigReloadError + advance
      // still behaves correctly.
      connectionStore.markConfigReloadError('fresh')
      expect(connectionStore.configReloadError).toBe('fresh')
      vi.advanceTimersByTime(60_000)
      expect(connectionStore.configReloadError).toBeNull()
    })

    it('destroy() clears the auto-dismiss timer (state is not changed by destroy)', () => {
      vi.setSystemTime(1_000_000)
      connectionStore.markConfigReloadError('boom')
      expect(connectionStore.configReloadError).toBe('boom')

      connectionStore.destroy()

      // destroy() clears the pending timer but does not null out state.
      // Advancing past the 60s deadline must NOT clear configReloadError —
      // that would prove the timer is still armed.
      vi.advanceTimersByTime(120_000)
      expect(connectionStore.configReloadError).toBe('boom')
    })
  })
})
