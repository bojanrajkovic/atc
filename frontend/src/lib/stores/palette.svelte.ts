/**
 * PaletteStore — high-frequency typing state for the command palette.
 *
 * Lives here (not in UIStore) because:
 * - paletteQuery changes on every keystroke; frequent mutations don't belong
 *   alongside low-frequency UI settings (theme, mode, density).
 * - recentRunIds benefits from sessionStorage (not localStorage) — transient
 *   per session, supporting user's search workflow without polluting long-term
 *   UI preferences.
 *
 * Persistence: recentRunIds is stored in sessionStorage under the key
 * "atc.palette.recent" (dot-separated for intentional namespace separation from
 * UIStore's localStorage keys like "atc-theme", "atc-mode", "atc-density").
 *
 * LRU semantics: only recency-tracking; full frecency (weighted by time and
 * frequency) is a backlog issue, not v1 scope.
 */
export class PaletteStore {
  paletteOpen = $state(false)
  paletteQuery = $state('')
  recentRunIds = $state<bigint[]>([])
  subMenu = $state<'theme' | null>(null)

  constructor() {
    // Restore persisted recentRunIds from sessionStorage. Storage access can
    // throw `SecurityError` (Safari private mode, sandboxed iframe, cookies
    // disabled) or `QuotaExceededError`; treat persistence as best-effort so
    // app boot never fails on a storage quirk.
    if (typeof window !== 'undefined') {
      try {
        const saved = sessionStorage.getItem('atc.palette.recent')
        if (saved) {
          const parsed = JSON.parse(saved) as string[]
          this.recentRunIds = parsed.map((s) => BigInt(s))
        }
      } catch {
        // SecurityError on getItem, malformed JSON, or invalid BigInt — ignore
        this.recentRunIds = []
      }
    }

    // Persist recentRunIds to sessionStorage whenever it changes (best-effort)
    $effect.root(() => {
      $effect(() => {
        if (typeof window === 'undefined') return
        try {
          const serialized = JSON.stringify(this.recentRunIds.map(String))
          sessionStorage.setItem('atc.palette.recent', serialized)
        } catch {
          // SecurityError or QuotaExceededError — silently drop the write
        }
      })
    })
  }

  open(): void {
    this.paletteOpen = true
    // Preserve paletteQuery — do NOT clear it
  }

  close(): void {
    this.paletteOpen = false
    this.subMenu = null
  }

  toggle(): void {
    if (this.paletteOpen) {
      this.close()
    } else {
      this.open()
    }
  }

  setQuery(q: string): void {
    this.paletteQuery = q
  }

  recordRunVisit(id: bigint): void {
    // LRU: filter out existing entry, unshift new id, slice to first 10
    this.recentRunIds = [id, ...this.recentRunIds.filter((existing) => existing !== id)].slice(
      0,
      10,
    )
  }

  enterSubmenu(name: 'theme'): void {
    this.subMenu = name
  }

  exitSubmenu(): void {
    this.subMenu = null
  }
}

export const paletteStore = new PaletteStore()
