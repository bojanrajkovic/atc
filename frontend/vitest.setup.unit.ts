import { vi } from 'vitest'

// Mock localStorage for jsdom tests.
// jsdom's localStorage is unreliable; uiStore reads it in its constructor,
// so the stub must be in place before any test module that imports RunCard
// (which transitively imports uiStore) evaluates.
const _mockLocalStorage = (() => {
  let store: Record<string, string> = {}
  return {
    getItem: (k: string) => store[k] ?? null,
    setItem: (k: string, v: string) => {
      store[k] = v
    },
    removeItem: (k: string) => {
      delete store[k]
    },
    clear: () => {
      store = {}
    },
  }
})()
vi.stubGlobal('localStorage', _mockLocalStorage)

// Mock window.matchMedia for jsdom tests
// This is needed because kanban-transitions.ts imports from svelte/motion,
// which calls window.matchMedia at module scope
vi.stubGlobal(
  'matchMedia',
  vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
)
