/**
 * Pure helpers for the `?run=<id>` deep-link surface. No DOM access — the
 * caller passes a URL string (absolute or relative), and the formatter
 * returns a relative URL string (`pathname + search + hash`) that
 * `history.pushState` / `replaceState` accept directly. The relative shape is
 * the canonical form across this module so the loop guard in App.svelte can
 * compare formatter output against the current relative URL exactly; mixing
 * absolute and relative shapes silently produces duplicate history entries.
 *
 * Both helpers accept relative inputs (parsed against a synthetic base) so
 * round-tripping through `formatUrlForRunId` and back through
 * `parseRunIdFromUrl` is lossless.
 */

const RUN_ID_PATTERN = /^[0-9]+$/

// Synthetic base used to parse relative URLs. The host is unused because the
// formatter only emits pathname + search + hash, but URL requires *some* base
// for relative inputs. Absolute inputs ignore this base entirely.
const SYNTHETIC_BASE = 'http://_/'

export function parseRunIdFromUrl(url: string): bigint | null {
  let parsed: URL
  try {
    parsed = new URL(url, SYNTHETIC_BASE)
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
  const parsed = new URL(currentUrl, SYNTHETIC_BASE)
  if (runId === null) {
    parsed.searchParams.delete('run')
  } else {
    parsed.searchParams.set('run', runId.toString())
  }
  return `${parsed.pathname}${parsed.search}${parsed.hash}`
}
