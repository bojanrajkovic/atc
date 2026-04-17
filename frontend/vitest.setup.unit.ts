import { vi } from 'vitest'

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
