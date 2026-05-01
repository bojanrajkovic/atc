import { describe, expect, it } from 'vitest'
import { formatTimestamp } from './timestamp'

describe('formatTimestamp', () => {
  it('formats a known ISO string to en-US medium date + short time', () => {
    // 2026-04-26T14:32:00Z in UTC; result depends on system locale/timezone.
    // Use a UTC reference and match the known output for the en-US formatter.
    // Note: Intl.DateTimeFormat without a timezone option uses the runtime's local
    // timezone — pinning a fixed UTC timestamp gives us a known UTC wall-clock value
    // but the *displayed* time depends on TZ offset. To make the test
    // deterministic, format the same date ourselves and assert equality.
    const iso = '2026-04-26T14:32:00Z'
    const expected = new Intl.DateTimeFormat('en-US', {
      dateStyle: 'medium',
      timeStyle: 'short',
    }).format(new Date(iso))

    expect(formatTimestamp(iso)).toBe(expected)
  })

  it('midnight edge case does not throw and returns a non-empty string', () => {
    const iso = '2026-01-01T00:00:00Z'
    let result: string | undefined
    expect(() => {
      result = formatTimestamp(iso)
    }).not.toThrow()
    expect(typeof result).toBe('string')
    expect(result!.length).toBeGreaterThan(0)
  })
})
