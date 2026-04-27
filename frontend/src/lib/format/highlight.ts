/**
 * Wrap fuzzy-matched substrings in <mark> elements for visual highlighting.
 * Uses simple case-insensitive substring match (not full fuzzy match) — close
 * enough for visual hinting; `command-score` handles the authoritative scoring at the connected-component level.
 *
 * Returns HTML; consumers must use {@html} to render.
 */
export function highlightMatches(text: string, query: string): string {
  if (!query) return escapeHtml(text)
  const trimmed = query.trim()
  if (!trimmed) return escapeHtml(text)
  // Escape regex special chars in query
  const escapedQuery = trimmed.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const re = new RegExp(`(${escapedQuery})`, 'gi')
  const parts = escapeHtml(text).split(re)
  // In split results with capturing group, matches always sit at odd indices (1, 3, 5, ...)
  // non-matches at even indices (0, 2, 4, ...). No need to test with stateful regex.
  return parts.map((part, idx) => (idx % 2 === 1 ? `<mark>${part}</mark>` : part)).join('')
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}
