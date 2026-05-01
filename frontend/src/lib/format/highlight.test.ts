import { describe, expect, it } from 'vitest'
import { highlightMatches } from './highlight'

describe('highlightMatches', () => {
  it('returns escaped HTML when query is empty', () => {
    const result = highlightMatches('hello world', '')
    expect(result).toBe('hello world')
  })

  it('returns escaped HTML when query is only whitespace', () => {
    const result = highlightMatches('hello world', '   ')
    expect(result).toBe('hello world')
  })

  it('wraps matched substring in mark elements with case-insensitive match', () => {
    const result = highlightMatches('linux x86', 'lin')
    expect(result).toContain('<mark>lin</mark>')
    expect(result).toContain('ux x86')
  })

  it('wraps case-insensitive matches', () => {
    const result = highlightMatches('LINUX', 'lin')
    expect(result).toContain('<mark>')
    expect(result).toContain('</mark>')
  })

  it('handles multiple matches', () => {
    const result = highlightMatches('test test', 'test')
    const markCount = (result.match(/<mark>/g) || []).length
    expect(markCount).toBe(2)
  })

  it('escapes HTML special characters in text', () => {
    const result = highlightMatches('<script>alert("xss")</script>', 'alert')
    expect(result).not.toContain('<script>')
    expect(result).toContain('&lt;script&gt;')
    expect(result).toContain('&quot;')
    expect(result).toContain('<mark>alert</mark>')
  })

  it('escapes regex special characters in query', () => {
    const result = highlightMatches('test.example', '.')
    // Should match the literal dot, not any character
    expect(result).toContain('<mark>.</mark>')
  })

  it('returns empty string when text is empty', () => {
    const result = highlightMatches('', 'query')
    expect(result).toBe('')
  })

  it('handles multiple words with spaces', () => {
    const result = highlightMatches('linux self-hosted x86', 'self')
    expect(result).toContain('<mark>self</mark>')
    expect(result).toContain('linux')
    expect(result).toContain('-hosted x86')
  })
})
