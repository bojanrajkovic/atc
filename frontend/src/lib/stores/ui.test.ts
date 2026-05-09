import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Mock localStorage since jsdom doesn't properly support it
const mockLocalStorage = (() => {
  let store: Record<string, string> = {}

  return {
    getItem: (key: string) => store[key] ?? null,
    setItem: (key: string, value: string) => {
      store[key] = value
    },
    removeItem: (key: string) => {
      delete store[key]
    },
    clear: () => {
      store = {}
    },
  }
})()

vi.stubGlobal('localStorage', mockLocalStorage)

describe('UIStore', () => {
  // UIStore is a module-level singleton with $effect.root().
  // We use vi.resetModules() to get a fresh singleton for each test.
  let uiStore: typeof import('./ui.svelte')['uiStore']

  beforeEach(async () => {
    // Clear localStorage before test
    mockLocalStorage.clear()
    // Reset all modules to get a fresh singleton
    vi.resetModules()
    // Dynamically import the store module to get a fresh instance
    const module = await import('./ui.svelte')
    uiStore = module.uiStore
  })

  afterEach(() => {
    uiStore.destroy() // singleton now owns a setInterval; destroy it per-test
    // to avoid real-timer leaks.
    mockLocalStorage.clear()
  })

  // AC3.8: UIStore persists theme and mode to localStorage and restores on initialization
  describe('AC3.8: localStorage persistence and DOM attribute setting', () => {
    it('should initialize with default theme (radar) and mode (dark)', () => {
      expect(uiStore.theme).toBe('radar')
      expect(uiStore.mode).toBe('dark')
    })

    it('should restore theme from localStorage on initialization', async () => {
      // Set localStorage before creating the store
      mockLocalStorage.setItem('atc-theme', 'violet')

      // Reset modules and reimport to test initialization
      vi.resetModules()
      const { uiStore: newStore } = await import('./ui.svelte')

      expect(newStore.theme).toBe('violet')
    })

    it('should restore mode from localStorage on initialization', async () => {
      // Set localStorage before creating the store
      mockLocalStorage.setItem('atc-mode', 'light')

      // Reset modules and reimport to test initialization
      vi.resetModules()
      const { uiStore: newStore } = await import('./ui.svelte')

      expect(newStore.mode).toBe('light')
    })

    it('should restore both theme and mode from localStorage on initialization', async () => {
      // Set both values
      mockLocalStorage.setItem('atc-theme', 'pink')
      mockLocalStorage.setItem('atc-mode', 'light')

      // Reset modules and reimport to test initialization
      vi.resetModules()
      const { uiStore: newStore } = await import('./ui.svelte')

      expect(newStore.theme).toBe('pink')
      expect(newStore.mode).toBe('light')
    })

    it('should persist theme to localStorage when changed', async () => {
      uiStore.theme = 'pink'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(mockLocalStorage.getItem('atc-theme')).toBe('pink')
    })

    it('should persist mode to localStorage when changed', async () => {
      uiStore.mode = 'light'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(mockLocalStorage.getItem('atc-mode')).toBe('light')
    })

    it('should set data-theme attribute on documentElement when theme changes', async () => {
      uiStore.theme = 'warm'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-theme')).toBe('warm')
    })

    it('should set data-mode="light" attribute when mode is light', async () => {
      uiStore.mode = 'light'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-mode')).toBe('light')
    })

    it('should remove data-mode attribute when mode is dark', async () => {
      // First set to light
      uiStore.mode = 'light'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-mode')).toBe('light')

      // Then change to dark
      uiStore.mode = 'dark'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-mode')).toBeNull()
    })

    it('should initialize with no data-mode attribute (dark is default)', async () => {
      // Reset modules to get fresh initialization
      vi.resetModules()
      const { uiStore: newStore } = await import('./ui.svelte')

      // Dark mode is default, so no data-mode attribute should be set
      expect(newStore.mode).toBe('dark')
      expect(document.documentElement.getAttribute('data-mode')).toBeNull()
    })

    it('should update DOM attributes when persisted values are restored', async () => {
      mockLocalStorage.setItem('atc-theme', 'radar')
      mockLocalStorage.setItem('atc-mode', 'light')

      vi.resetModules()
      await import('./ui.svelte')

      expect(document.documentElement.getAttribute('data-theme')).toBe('radar')
      expect(document.documentElement.getAttribute('data-mode')).toBe('light')
    })

    it('should handle multiple consecutive theme changes', async () => {
      uiStore.theme = 'warm'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(mockLocalStorage.getItem('atc-theme')).toBe('warm')
      expect(document.documentElement.getAttribute('data-theme')).toBe('warm')

      uiStore.theme = 'violet'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(mockLocalStorage.getItem('atc-theme')).toBe('violet')
      expect(document.documentElement.getAttribute('data-theme')).toBe('violet')

      uiStore.theme = 'pink'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(mockLocalStorage.getItem('atc-theme')).toBe('pink')
      expect(document.documentElement.getAttribute('data-theme')).toBe('pink')
    })

    it('should handle toggling between light and dark mode', async () => {
      expect(uiStore.mode).toBe('dark')
      expect(document.documentElement.getAttribute('data-mode')).toBeNull()

      uiStore.mode = 'light'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-mode')).toBe('light')
      expect(mockLocalStorage.getItem('atc-mode')).toBe('light')

      uiStore.mode = 'dark'
      // Allow effects to fire
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-mode')).toBeNull()
      expect(mockLocalStorage.getItem('atc-mode')).toBe('dark')
    })

    it('should not affect density field', () => {
      expect(uiStore.density).toBe('comfortable')

      uiStore.theme = 'warm'
      uiStore.mode = 'light'

      expect(uiStore.density).toBe('comfortable')
    })

    it('should sync density to DOM and localStorage', async () => {
      expect(uiStore.density).toBe('comfortable')
      expect(document.documentElement.getAttribute('data-density')).toBeNull()

      uiStore.density = 'compact'
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-density')).toBe('compact')
      expect(mockLocalStorage.getItem('atc-density')).toBe('compact')

      uiStore.density = 'comfortable'
      await new Promise((r) => setTimeout(r, 0))
      expect(document.documentElement.getAttribute('data-density')).toBeNull()
      expect(mockLocalStorage.getItem('atc-density')).toBe('comfortable')
    })

    it('should restore density from localStorage', async () => {
      mockLocalStorage.setItem('atc-density', 'compact')
      vi.resetModules()
      const module = await import('$lib/stores/ui.svelte')
      expect(module.uiStore.density).toBe('compact')
    })

    it('should not affect selectedRunId field', () => {
      expect(uiStore.selectedRunId).toBeNull()

      uiStore.theme = 'warm'
      uiStore.selectedRunId = 42n

      expect(uiStore.selectedRunId).toBe(42n)
    })
  })
})
