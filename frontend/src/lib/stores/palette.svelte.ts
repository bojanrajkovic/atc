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
    // Restore persisted recentRunIds from sessionStorage
    if (typeof window !== 'undefined') {
      const saved = sessionStorage.getItem('atc.palette.recent')
      if (saved) {
        try {
          const parsed = JSON.parse(saved) as string[]
          this.recentRunIds = parsed.map((s) => BigInt(s))
        } catch {
          // Malformed or invalid JSON; ignore and start fresh
          this.recentRunIds = []
        }
      }
    }

    // Persist recentRunIds to sessionStorage whenever it changes
    $effect.root(() => {
      $effect(() => {
        const serialized = JSON.stringify(this.recentRunIds.map(String))
        sessionStorage.setItem('atc.palette.recent', serialized)
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
