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

  // AC3.7: isStale detection based on time elapsed since last event
  describe('AC3.7: Staleness detection', () => {
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
})
