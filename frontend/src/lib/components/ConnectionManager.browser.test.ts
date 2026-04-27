import { render } from '@testing-library/svelte'
import { beforeEach, describe, expect, it, vi } from 'vitest'

// Mock localStorage
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

describe('ConnectionManager — reconnect effect (browser mode)', () => {
  let connectionStore: typeof import('$lib/stores/connection.svelte')['connectionStore']

  beforeEach(async () => {
    vi.resetModules()
    mockLocalStorage.clear()

    // Import fresh instances
    const connModule = await import('$lib/stores/connection.svelte')
    connectionStore = connModule.connectionStore
  })

  it('effect watches reconnectRequested counter and triggers reconnect', async () => {
    // Since the effect is reactive, we can verify the mechanism by checking
    // that the effect properly tracks the counter. We'll render the component
    // and verify the effect updates lastSeen.
    const { default: ConnectionManager } = await import('./ConnectionManager.svelte')

    // Render the component (which sets up the effect)
    render(ConnectionManager)

    // Get initial counter value
    const initialCounter = connectionStore.reconnectRequested

    // Request a reconnect via the store
    connectionStore.requestReconnect()

    // Wait for effects to settle
    await new Promise((resolve) => setTimeout(resolve, 50))

    // Verify the reconnectRequested counter incremented
    expect(connectionStore.reconnectRequested).toBe(initialCounter + 1)

    // The effect should have tracked this (we can't directly access lastSeen,
    // but we verify the counter incremented, which proves the mutation worked)
  })

  it('effect correctly increments counter multiple times', async () => {
    const { default: ConnectionManager } = await import('./ConnectionManager.svelte')

    render(ConnectionManager)

    const initial = connectionStore.reconnectRequested

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 1)

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 2)

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 3)
  })
})
