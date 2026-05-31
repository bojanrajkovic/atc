import { render, screen } from '@testing-library/svelte'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'

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
  // Helper to create an InProgress Job with a runner (for pool derivation)
  function makeInProgressJob(
    id: bigint,
    runId: bigint,
    labels: string[],
    runner: RunnerInfo | null,
  ): Job {
    return {
      id,
      runId,
      name: `job-${id}`,
      status: 'InProgress',
      conclusion: null,
      labels,
      runner,
      steps: [],
      createdAt: new Date().toISOString(),
      startedAt: new Date().toISOString(),
      completedAt: null,
      runAttempt: 1,
    }
  }

  // Helper to create a runner with given groupName
  function makeRunner(groupName: string | null): RunnerInfo {
    return {
      id: 1n,
      name: 'runner-1',
      groupName,
    }
  }

  let TopBar: typeof import('./TopBar.svelte').default
  let connectionStore: typeof import('$lib/stores/connection.svelte')['connectionStore']
  let runStore: typeof import('$lib/stores/runs.svelte')['runStore']

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.resetModules()
    const connModule = await import('$lib/stores/connection.svelte')
    const runModule = await import('$lib/stores/runs.svelte')
    const topBarModule = await import('./TopBar.svelte')
    connectionStore = connModule.connectionStore
    runStore = runModule.runStore
    TopBar = topBarModule.default

    runStore.jobsByRun.clear()
  })

  afterEach(() => {
    mockLocalStorage.clear()
    runStore.jobsByRun.clear()
  })

  it('renders Logo text', () => {
    render(TopBar)

    const logo = screen.getByLabelText(/ATC — Actions Traffic Control/i)
    expect(logo).toBeTruthy()
  })

  it('renders ConnectionIndicator with disconnected state by default', () => {
    render(TopBar)

    // Default connectionStore.status is 'disconnected'. TopBar wires a
    // requestReconnect callback so the indicator renders as a clickable
    // button (per issue #24) rather than the inert role="status" span.
    const indicator = screen.getByRole('button', { name: /disconnected/i })
    expect(indicator).toBeTruthy()
  })

  it('renders Settings button', () => {
    render(TopBar)

    const settingsButton = screen.getByRole('button', { name: /settings/i })
    expect(settingsButton).toBeTruthy()
  })

  it('renders RunnerBar with empty pools by default', () => {
    render(TopBar)

    // Default runnerStore.pools is empty (no jobs), so the empty-state copy appears.
    const noPools = screen.getByText('No active runners')
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

  it('renders runner pools when jobs are present', async () => {
    render(TopBar)

    // Seed jobs to derive pools from
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['linux'], makeRunner(null)),
      makeInProgressJob(2n, 1n, ['linux'], makeRunner(null)),
      makeInProgressJob(3n, 1n, ['linux'], makeRunner(null)),
      makeInProgressJob(4n, 1n, ['windows', 'large'], makeRunner('windows-group')),
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

  it('two pools sharing a groupName render with disambiguated labels', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['ubuntu-latest'], makeRunner('GitHub Actions')),
      makeInProgressJob(2n, 1n, ['ubuntu-24.04'], makeRunner('GitHub Actions')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions · ubuntu-latest')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-24.04')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions')).toBeNull()
  })

  it('single pool with non-null groupName renders without suffix', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['ubuntu-latest'], makeRunner('GitHub Actions')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions ·')).toBeNull()
  })

  it('pool with null groupName falls back to joined labels', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['self-hosted', 'linux'], makeRunner(null)),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('linux, self-hosted')).toBeTruthy()
  })

  it('three pools sharing a groupName each get the suffix', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['ubuntu-latest'], makeRunner('GitHub Actions')),
      makeInProgressJob(2n, 1n, ['ubuntu-24.04'], makeRunner('GitHub Actions')),
      makeInProgressJob(3n, 1n, ['ubuntu-22.04'], makeRunner('GitHub Actions')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions · ubuntu-latest')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-24.04')).toBeTruthy()
    expect(screen.getByText('GitHub Actions · ubuntu-22.04')).toBeTruthy()
    expect(screen.queryByText('GitHub Actions')).toBeNull()
  })

  it('chip label falls back to labels when groupName is "Default" (issue #143)', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['self-hosted', 'linux', 'amd64'], makeRunner('Default')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('amd64, linux, self-hosted')).toBeTruthy()
    expect(screen.queryByText('Default')).toBeNull()
  })

  it('"Default" group is treated as null when disambiguating shared groupName', async () => {
    // Two pools both named "Default" should NOT get the disambiguation suffix —
    // the count loop excludes "Default" so both fall through to the labels-only
    // branch, matching the rule "Default is treated as null".
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['self-hosted', 'linux'], makeRunner('Default')),
      makeInProgressJob(2n, 1n, ['self-hosted', 'macos'], makeRunner('Default')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('linux, self-hosted')).toBeTruthy()
    expect(screen.getByText('macos, self-hosted')).toBeTruthy()
    expect(screen.queryByText(/^Default ·/)).toBeNull()
  })

  it('mixed ambiguous and unambiguous pools', async () => {
    render(TopBar)
    runStore.jobsByRun.set(1n, [
      makeInProgressJob(1n, 1n, ['ubuntu-latest'], makeRunner('GitHub Actions')),
      makeInProgressJob(2n, 1n, ['self-hosted', 'x86_64'], makeRunner('self-hosted-linux-group')),
      makeInProgressJob(3n, 1n, ['self-hosted', 'arm64'], makeRunner('self-hosted-linux-group')),
    ])
    await new Promise((r) => setTimeout(r, 50))

    expect(screen.getByText('GitHub Actions')).toBeTruthy()
    expect(screen.getByText('self-hosted-linux-group · self-hosted, x86_64')).toBeTruthy()
    expect(screen.getByText('self-hosted-linux-group · arm64, self-hosted')).toBeTruthy()
  })

  describe('GoingAway tooltip (issue #47)', () => {
    it('shows "Server restarting" tooltip when connectionStore.serverGoingAway is true and we are reconnecting', async () => {
      render(TopBar)

      // Simulate the going-away envelope arriving, which the dispatcher would
      // route via connectionStore.markGoingAway(). Then the WS would close and
      // ConnectionManager would transition status to 'reconnecting'.
      connectionStore.markGoingAway('server shutdown')
      connectionStore.status = 'reconnecting'

      await new Promise((r) => setTimeout(r, 50))

      // The indicator's aria-label / tooltip text should reflect the restart
      // rather than the generic "Reconnecting..." wording.
      const indicator = screen.getByRole('status', { name: /server restarting/i })
      expect(indicator).toBeTruthy()
    })

    it('clears the going-away framing once status reaches connected', async () => {
      render(TopBar)

      connectionStore.markGoingAway('server shutdown')
      connectionStore.status = 'reconnecting'
      await new Promise((r) => setTimeout(r, 50))
      expect(screen.queryByRole('status', { name: /server restarting/i })).toBeTruthy()

      // ConnectionManager's success path resets serverGoingAway / goingAwayReason
      // alongside the status flip to 'connected'.
      connectionStore.serverGoingAway = false
      connectionStore.goingAwayReason = null
      connectionStore.status = 'connected'
      connectionStore.lastEventAt = Date.now()
      await new Promise((r) => setTimeout(r, 50))

      expect(screen.queryByRole('status', { name: /server restarting/i })).toBeNull()
      expect(screen.getByRole('status', { name: /connected/i })).toBeTruthy()
    })
  })
})
