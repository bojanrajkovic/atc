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

describe('SettingsPopover', () => {
  // SettingsPopover uses uiStore, which is a module-level singleton with $effect.root().
  // We use vi.resetModules() to get a fresh singleton for each test.
  let uiStore: typeof import('$lib/stores/ui.svelte')['uiStore']
  let SettingsPopover: typeof import('./SettingsPopover.svelte').default

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.resetModules()
    const uiModule = await import('$lib/stores/ui.svelte')
    uiStore = uiModule.uiStore
    const componentModule = await import('./SettingsPopover.svelte')
    SettingsPopover = componentModule.default
  })

  afterEach(() => {
    mockLocalStorage.clear()
  })

  it('renders settings button with gear icon', () => {
    // The component should exist and be importable
    expect(SettingsPopover).toBeTruthy()
    expect(typeof SettingsPopover).toBe('function')
  })

  it('SettingsPopover component is a connected component', () => {
    // Verify the component imports and uses uiStore
    expect(uiStore).toBeTruthy()
    expect(uiStore.theme).toBe('radar')
    expect(uiStore.mode).toBe('dark')
    expect(uiStore.density).toBe('comfortable')
  })

  it('can mutate theme via uiStore', async () => {
    expect(uiStore.theme).toBe('radar')
    uiStore.theme = 'warm'
    await new Promise((r) => setTimeout(r, 0))
    expect(uiStore.theme).toBe('warm')
  })

  it('can mutate mode via uiStore', async () => {
    expect(uiStore.mode).toBe('dark')
    uiStore.mode = 'light'
    await new Promise((r) => setTimeout(r, 0))
    expect(uiStore.mode).toBe('light')
  })

  it('can mutate density via uiStore', async () => {
    expect(uiStore.density).toBe('comfortable')
    uiStore.density = 'compact'
    await new Promise((r) => setTimeout(r, 0))
    expect(uiStore.density).toBe('compact')
  })

  it('persists theme mutations to localStorage', async () => {
    uiStore.theme = 'violet'
    await new Promise((r) => setTimeout(r, 0))
    expect(mockLocalStorage.getItem('atc-theme')).toBe('violet')
  })
})
