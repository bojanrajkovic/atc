import { describe, expect, it } from 'vitest'
import { formatUrlForRunId, parseRunIdFromUrl } from './url-state'

describe('url-state/parseRunIdFromUrl', () => {
  it('parses a valid positive run id', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=42')).toBe(42n)
  })

  it('returns null when the run param is missing', () => {
    expect(parseRunIdFromUrl('https://example.com/')).toBeNull()
  })

  it('returns null for an empty run param', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=')).toBeNull()
  })

  it('returns null for a non-numeric run param', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=abc')).toBeNull()
  })

  it('returns null for a negative run param', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=-5')).toBeNull()
  })

  it('returns null for a leading-plus run param', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=%2B5')).toBeNull()
  })

  it('returns null for scientific notation', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=1e10')).toBeNull()
  })

  it('returns null for decimal values', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=1.5')).toBeNull()
  })

  it('returns null for whitespace-padded values', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=%2042')).toBeNull()
  })

  it('returns the first value when multiple run params are present', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=1&run=2')).toBe(1n)
  })

  it('preserves precision for very large bigint values', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=18446744073709551615')).toBe(
      18446744073709551615n,
    )
  })

  it('ignores other query params and returns the run value', () => {
    expect(parseRunIdFromUrl('https://example.com/?foo=bar&run=99')).toBe(99n)
  })

  it('returns the run value across a hash fragment', () => {
    expect(parseRunIdFromUrl('https://example.com/?run=7#section')).toBe(7n)
  })

  it('returns null for a completely malformed URL', () => {
    expect(parseRunIdFromUrl('not a url')).toBeNull()
  })
})

describe('url-state/formatUrlForRunId', () => {
  it('sets the run param when given a bigint', () => {
    expect(formatUrlForRunId(42n, 'https://example.com/')).toBe('/?run=42')
  })

  it('deletes the run param when given null', () => {
    expect(formatUrlForRunId(null, 'https://example.com/?run=42')).toBe('/')
  })

  it('preserves a non-root pathname', () => {
    expect(formatUrlForRunId(42n, 'https://example.com/foo/bar')).toBe('/foo/bar?run=42')
  })

  it('preserves the hash fragment when setting the run param', () => {
    expect(formatUrlForRunId(42n, 'https://example.com/#section')).toBe('/?run=42#section')
  })

  it('preserves the hash fragment when deleting the run param', () => {
    expect(formatUrlForRunId(null, 'https://example.com/?run=42#section')).toBe('/#section')
  })

  it('preserves other query params when setting the run param', () => {
    expect(formatUrlForRunId(42n, 'https://example.com/?foo=bar')).toBe('/?foo=bar&run=42')
  })

  it('preserves other query params when deleting the run param', () => {
    expect(formatUrlForRunId(null, 'https://example.com/?foo=bar&run=42')).toBe('/?foo=bar')
  })

  it('overwrites an existing run param', () => {
    expect(formatUrlForRunId(99n, 'https://example.com/?run=42')).toBe('/?run=99')
  })

  it('returns a relative URL (no protocol, no host)', () => {
    const result = formatUrlForRunId(42n, 'https://example.com/foo')
    expect(result).not.toMatch(/^https?:/)
    expect(result).not.toContain('example.com')
  })

  it('emits a large bigint without truncation', () => {
    expect(formatUrlForRunId(18446744073709551615n, 'https://example.com/')).toBe(
      '/?run=18446744073709551615',
    )
  })

  it('preserves the pathname and hash when deleting a run param that is not present', () => {
    expect(formatUrlForRunId(null, 'https://example.com/foo#bar')).toBe('/foo#bar')
  })
})
