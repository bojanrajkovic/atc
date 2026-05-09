import { describe, expect, it } from 'vitest'
import { formatDuration } from './duration'

describe('format/duration', () => {
  describe('static spec with 1-second diff', () => {
    it('returns 0:01 for 1-second diff', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 1000 })
      expect(result).toBe('0:01')
    })

    it('returns 1:00 for 60-second diff', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 60000 })
      expect(result).toBe('1:00')
    })

    it('returns 2:14 for 134-second diff', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 134000 })
      expect(result).toBe('2:14')
    })

    it('returns 9:59 for 599-second diff', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 599000 })
      expect(result).toBe('9:59')
    })
  })

  describe('live spec with 61-second diff', () => {
    it('returns 1:01 for live spec with 61-second diff', () => {
      const result = formatDuration({ kind: 'live', startMs: 0, nowMs: 61000 })
      expect(result).toBe('1:01')
    })

    it('live and static produce identical output for identical diffs', () => {
      const diff = 61000
      const staticResult = formatDuration({ kind: 'static', startMs: 0, endMs: diff })
      const liveResult = formatDuration({ kind: 'live', startMs: 0, nowMs: diff })
      expect(staticResult).toBe(liveResult)
      expect(liveResult).toBe('1:01')
    })
  })

  describe('switchover at 3600000 ms (1 hour)', () => {
    it('returns 59:59 for 3599999 ms (last MM:SS output)', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 3599999 })
      expect(result).toBe('59:59')
    })

    it('returns 1:00:00 for 3600000 ms (first H:MM:SS output)', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 3600000 })
      expect(result).toBe('1:00:00')
    })

    it('returns 1:01:01 for 3661000 ms', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 3661000 })
      expect(result).toBe('1:01:01')
    })

    it('returns 27:00:00 for 97200000 ms (27 hours)', () => {
      const result = formatDuration({ kind: 'static', startMs: 0, endMs: 97200000 })
      expect(result).toBe('27:00:00')
    })
  })

  describe('negative diff edge case', () => {
    it('returns 0:00 for static spec with endMs < startMs', () => {
      const result = formatDuration({ kind: 'static', startMs: 1000, endMs: 0 })
      expect(result).toBe('0:00')
    })

    it('returns 0:00 for live spec with nowMs < startMs', () => {
      const result = formatDuration({ kind: 'live', startMs: 1000, nowMs: 0 })
      expect(result).toBe('0:00')
    })

    it('output does not contain NaN substring', () => {
      const result = formatDuration({ kind: 'static', startMs: 5000, endMs: 0 })
      expect(result).not.toContain('NaN')
      expect(result).toBe('0:00')
    })
  })

  describe('character-count stability within format zones', () => {
    describe('MM:SS one-digit-minute sub-range', () => {
      it('0:00 and 9:59 have equal length (both 4 characters)', () => {
        const min = formatDuration({ kind: 'static', startMs: 0, endMs: 0 })
        const max = formatDuration({ kind: 'static', startMs: 0, endMs: 599000 })
        expect(min).toBe('0:00')
        expect(max).toBe('9:59')
        expect(min.length).toBe(4)
        expect(max.length).toBe(4)
        expect(min.length).toBe(max.length)
      })
    })

    describe('MM:SS two-digit-minute sub-range', () => {
      it('10:00 and 59:59 have equal length (both 5 characters)', () => {
        const min = formatDuration({ kind: 'static', startMs: 0, endMs: 600000 })
        const max = formatDuration({ kind: 'static', startMs: 0, endMs: 3599000 })
        expect(min).toBe('10:00')
        expect(max).toBe('59:59')
        expect(min.length).toBe(5)
        expect(max.length).toBe(5)
        expect(min.length).toBe(max.length)
      })
    })

    describe('H:MM:SS one-digit-hour sub-range', () => {
      it('1:01:01 and 9:59:59 have equal length (both 7 characters)', () => {
        const min = formatDuration({ kind: 'static', startMs: 0, endMs: 3661000 })
        const max = formatDuration({ kind: 'static', startMs: 0, endMs: 35999000 })
        expect(min).toBe('1:01:01')
        expect(max).toBe('9:59:59')
        expect(min.length).toBe(7)
        expect(max.length).toBe(7)
        expect(min.length).toBe(max.length)
      })
    })
  })
})
