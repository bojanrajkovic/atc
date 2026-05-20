/**
 * Pure helpers for the `?run=<id>` deep-link surface. No DOM access — the
 * caller passes `window.location.href` (or any URL string) in, and the
 * formatter returns a relative URL string (`pathname + search + hash`) that
 * `history.pushState` / `replaceState` accept directly. The relative shape is
 * the canonical form across this module so the loop guard in App.svelte can
 * compare formatter output against the current relative URL exactly; mixing
 * absolute and relative shapes silently produces duplicate history entries.
 */

const RUN_ID_PATTERN = /^[0-9]+$/

export function parseRunIdFromUrl(url: string): bigint | null {
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return null
  }
  const raw = parsed.searchParams.get('run')
  if (raw === null || raw === '') return null
  if (!RUN_ID_PATTERN.test(raw)) return null
  try {
    return BigInt(raw)
  } catch {
    return null
  }
}

export function formatUrlForRunId(runId: bigint | null, currentUrl: string): string {
  const parsed = new URL(currentUrl)
  if (runId === null) {
    parsed.searchParams.delete('run')
  } else {
    parsed.searchParams.set('run', runId.toString())
  }
  return `${parsed.pathname}${parsed.search}${parsed.hash}`
}
