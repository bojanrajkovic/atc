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

  it('effect invokes manager.reconnect when reconnectRequested counter changes', async () => {
    const connModule = await import('$lib/connection')
    const reconnectSpy = vi.spyOn(connModule.ConnectionManager.prototype, 'reconnect')

    const { default: ConnectionManager } = await import('./ConnectionManager.svelte')
    render(ConnectionManager)

    // Initial state: no reconnects should have been called yet
    const initialCallCount = reconnectSpy.mock.calls.length
    expect(initialCallCount).toBe(0)

    // Request a reconnect
    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 100))

    // The effect should have called manager.reconnect()
    expect(reconnectSpy.mock.calls.length).toBe(initialCallCount + 1)

    // Request another reconnect
    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 100))

    // The effect should have called manager.reconnect() again
    expect(reconnectSpy.mock.calls.length).toBe(initialCallCount + 2)

    reconnectSpy.mockRestore()
  })

  it('effect correctly increments reconnect call count on multiple requests', async () => {
    const connModule = await import('$lib/connection')
    const reconnectSpy = vi.spyOn(connModule.ConnectionManager.prototype, 'reconnect')

    const { default: ConnectionManager } = await import('./ConnectionManager.svelte')
    render(ConnectionManager)

    const initial = connectionStore.reconnectRequested
    const initialCallCount = reconnectSpy.mock.calls.length

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 1)
    expect(reconnectSpy.mock.calls.length).toBe(initialCallCount + 1)

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 2)
    expect(reconnectSpy.mock.calls.length).toBe(initialCallCount + 2)

    connectionStore.requestReconnect()
    await new Promise((resolve) => setTimeout(resolve, 30))
    expect(connectionStore.reconnectRequested).toBe(initial + 3)
    expect(reconnectSpy.mock.calls.length).toBe(initialCallCount + 3)

    reconnectSpy.mockRestore()
  })
})
