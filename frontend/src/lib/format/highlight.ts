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
  return parts.map((part) => (re.test(part) ? `<mark>${part}</mark>` : part)).join('')
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}
