import { render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'

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
  // Helper to create RunnerPoolStats with sensible defaults
  function makePool(opts: {
    groupName: string | null
    labels: string[]
    queued?: number
    running?: number
    total?: number | null
    isElastic?: boolean
  }): RunnerPoolStats {
    return {
      labels: opts.labels,
      groupName: opts.groupName,
      queued: opts.queued ?? 0,
      running: opts.running ?? 1,
      total: opts.total ?? null,
      isElastic: opts.isElastic ?? true,
    }
  }
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

  it('shows connected indicator when connection established', async () => {
    render(TopBar)

    // Set connection to connected and update timestamp
    connectionStore.status = 'connected'
    connectionStore.lastEventAt = Date.now()

    // Wait for reactivity
    await new Promise((r) => setTimeout(r, 50))

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

  it('AC3.1 — two pools sharing a groupName render with disambiguated labels', async () => {
    render(TopBar)
    runnerStore.loadPools([
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-latest'] }),
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-24.04'] }),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions · ubuntu-latest')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-24.04')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions')).toBeNull()
  })

  it('AC3.2 — single pool with non-null groupName renders without suffix', async () => {
    render(TopBar)
    runnerStore.loadPools([makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-latest'] })])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions ·')).toBeNull()
  })

  it('AC3.3 — pool with null groupName falls back to joined labels', async () => {
    render(TopBar)
    runnerStore.loadPools([makePool({ groupName: null, labels: ['self-hosted', 'linux'] })])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('self-hosted, linux')).toBeTruthy()
  })

  it('AC3.4 — three pools sharing a groupName each get the suffix', async () => {
    render(TopBar)
    runnerStore.loadPools([
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-latest'] }),
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-24.04'] }),
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-22.04'] }),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions · ubuntu-latest')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-24.04')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-22.04')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions')).toBeNull()
  })

  it('AC3.5 — mixed ambiguous and unambiguous pools', async () => {
    render(TopBar)
    runnerStore.loadPools([
      makePool({ groupName: 'GitHub Actions', labels: ['ubuntu-latest'] }),
      makePool({ groupName: 'self-hosted-linux-group', labels: ['self-hosted', 'x86_64'] }),
      makePool({ groupName: 'self-hosted-linux-group', labels: ['self-hosted', 'arm64'] }),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions')).toBeTruthy()
    expect(screen.getByText('self-hosted-linux-group · self-hosted, x86_64')).toBeTruthy()
    expect(screen.getByText('self-hosted-linux-group · self-hosted, arm64')).toBeTruthy()
  })
})
