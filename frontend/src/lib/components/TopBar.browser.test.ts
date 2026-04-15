import { render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Mock localStorage since browsers still need this mock in some contexts
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

describe('TopBar (browser mode)', () => {
  let TopBar: typeof import('./TopBar.svelte').default
  let connectionStore: typeof import('$lib/stores/connection.svelte')['connectionStore']
  let runnerStore: typeof import('$lib/stores/runners.svelte')['runnerStore']

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.resetModules()
    const connModule = await import('$lib/stores/connection.svelte')
    const runnerModule = await import('$lib/stores/runners.svelte')
    const topBarModule = await import('./TopBar.svelte')
    connectionStore = connModule.connectionStore
    runnerStore = runnerModule.runnerStore
    TopBar = topBarModule.default
  })

  afterEach(() => {
    mockLocalStorage.clear()
  })

  it('renders Logo text', () => {
    render(TopBar)

    const logo = screen.getByLabelText(/ATC — Actions Traffic Control/i)
    expect(logo).toBeTruthy()
  })

  it('renders ConnectionIndicator with disconnected state by default', () => {
    render(TopBar)

    // Default connectionStore.status is 'disconnected'
    const indicator = screen.getByRole('status', { name: /disconnected/i })
    expect(indicator).toBeTruthy()
  })

  it('renders Settings button', () => {
    render(TopBar)

    const settingsButton = screen.getByRole('button', { name: /settings/i })
    expect(settingsButton).toBeTruthy()
  })

  it('renders RunnerBar with empty pools by default', () => {
    render(TopBar)

    // Default runnerStore.pools is empty, so "No pools" text appears
    const noPools = screen.getByText('No pools')
    expect(noPools).toBeTruthy()
  })

  it('shows connected indicator when connection established', () => {
    render(TopBar)

    // Set connection to connected and update timestamp
    connectionStore.status = 'connected'
    connectionStore.lastEventAt = Date.now()

    // Should render indicator with "Connected" status
    const indicator = screen.getByRole('status', { name: /connected/i })
    expect(indicator).toBeTruthy()
  })

  it('renders runner pools when loaded', async () => {
    render(TopBar)

    // Load pools into runnerStore
    runnerStore.loadPools([
      {
        labels: ['linux'],
        groupName: null,
        queued: 0,
        running: 3,
        isElastic: false,
        total: 10,
      },
      {
        labels: ['windows', 'large'],
        groupName: 'windows-group',
        queued: 2,
        running: 1,
        isElastic: false,
        total: null,
      },
    ])

    // Wait for reactivity
    await new Promise((r) => setTimeout(r, 50))

    // Should see both pool labels
    const linuxLabel = screen.getByText('linux')
    expect(linuxLabel).toBeTruthy()

    const windowsLabel = screen.getByText('windows-group')
    expect(windowsLabel).toBeTruthy()
  })

  it('renders separator dividers', () => {
    render(TopBar)

    // Separators should be rendered with separator role
    const separators = screen.getAllByRole('separator')
    expect(separators.length).toBeGreaterThanOrEqual(2)
  })
})
