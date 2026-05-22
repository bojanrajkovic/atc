import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// uiStore reads/writes localStorage in its $effect.root for theme persistence.
// Stub before importing the runStore module (which transitively imports uiStore).
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

describe('RunStore — display-TTL filter', () => {
  // Both modules are singletons; load them once per test under fake timers so
  // uiStore's setInterval lands on the virtual queue and runStore reactivity
  // tracks the same uiStore instance the filter does.
  let runStore: typeof import('./runs.svelte')['runStore']
  let uiStore: typeof import('./ui.svelte')['uiStore']
  let createMockRun: typeof import('$lib/test-utils/factories')['createMockRun']
  let createMockJob: typeof import('$lib/test-utils/factories')['createMockJob']

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-17T10:00:00Z'))
    vi.resetModules()
    uiStore = (await import('./ui.svelte')).uiStore
    runStore = (await import('./runs.svelte')).runStore
    createMockRun = (await import('$lib/test-utils/factories')).createMockRun
    createMockJob = (await import('$lib/test-utils/factories')).createMockJob
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
    uiStore.destroy()
    vi.useRealTimers()
    mockLocalStorage.clear()
  })

  it('keeps a completed run visible while its age is under the TTL', () => {
    const completed = createMockRun({
      id: 100n,
      status: 'Completed',
      conclusion: 'Success',
      // 30 minutes ago, well within the 1h TTL.
      completedAt: new Date(uiStore.nowMs - 30 * 60 * 1000).toISOString(),
      updatedAt: '2026-04-17T09:30:00Z',
    })
    runStore.loadSnapshot([completed], [], [], 3600)
    expect(runStore.completedRuns.map((r) => r.id)).toEqual([100n])
  })

  it('reactively hides a completed run when nowMs advances past the TTL', () => {
    const completed = createMockRun({
      id: 101n,
      status: 'Completed',
      conclusion: 'Success',
      // 30 minutes ago.
      completedAt: new Date(uiStore.nowMs - 30 * 60 * 1000).toISOString(),
      updatedAt: '2026-04-17T09:30:00Z',
    })
    runStore.loadSnapshot([completed], [], [], 3600)
    expect(runStore.completedRuns.length).toBe(1)

    // Advance past the 1h threshold: the row should drop without an event
    // arriving. The uiStore ticker (setInterval, 1s) feeds nowMs.
    vi.advanceTimersByTime(45 * 60 * 1000) // 45 minutes — total age now 75 min
    expect(runStore.completedRuns.length).toBe(0)
  })

  it('displayTtlSeconds=0 disables filtering entirely', () => {
    const ancient = createMockRun({
      id: 102n,
      status: 'Completed',
      conclusion: 'Success',
      // 99 hours ago — would be filtered under any positive TTL.
      completedAt: new Date(uiStore.nowMs - 99 * 60 * 60 * 1000).toISOString(),
      updatedAt: '2026-04-13T07:00:00Z',
    })
    runStore.loadSnapshot([ancient], [], [], 0)
    expect(runStore.completedRuns.map((r) => r.id)).toEqual([102n])
  })

  it('keeps a completed row with null completedAt visible (mixed-version snapshot)', () => {
    // A completed row from a pre-feature replica may arrive with
    // completedAt: null (or undefined if the field is missing entirely).
    // Both shapes must keep the row visible — the predicate treats either
    // as "no cutoff applies yet".
    const nullCompletedAt = createMockRun({
      id: 103n,
      status: 'Completed',
      conclusion: 'Success',
      updatedAt: '2026-04-13T07:00:00Z',
    })
    runStore.loadSnapshot([nullCompletedAt], [], [], 3600)
    expect(runStore.completedRuns.map((r) => r.id)).toEqual([103n])
  })

  it('does not filter queued or in-progress runs by age', () => {
    // Active runs must remain visible regardless of when they were created.
    const queued = createMockRun({
      id: 200n,
      status: 'Queued',
      // Older than the TTL but not Completed — must stay.
      createdAt: new Date(uiStore.nowMs - 5 * 60 * 60 * 1000).toISOString(),
      updatedAt: new Date(uiStore.nowMs - 5 * 60 * 60 * 1000).toISOString(),
    })
    const inProgress = createMockRun({
      id: 201n,
      status: 'InProgress',
      createdAt: new Date(uiStore.nowMs - 4 * 60 * 60 * 1000).toISOString(),
      runStartedAt: new Date(uiStore.nowMs - 4 * 60 * 60 * 1000).toISOString(),
      updatedAt: new Date(uiStore.nowMs - 4 * 60 * 60 * 1000).toISOString(),
    })
    runStore.loadSnapshot([queued, inProgress], [], [], 3600)
    expect(runStore.queuedRuns.map((r) => r.id)).toContain(200n)
    expect(runStore.inProgressRuns.map((r) => r.id)).toContain(201n)

    // Advance way past the TTL — active rows still present.
    vi.advanceTimersByTime(2 * 60 * 60 * 1000)
    expect(runStore.queuedRuns.map((r) => r.id)).toContain(200n)
    expect(runStore.inProgressRuns.map((r) => r.id)).toContain(201n)
  })

  it('filters jobs against the same predicate as runs', () => {
    // jobs deriver applies the same predicate as completedRuns. Set the run
    // visible so the job isn't pre-filtered by run-level culling.
    const run = createMockRun({
      id: 300n,
      status: 'InProgress',
    })
    const expired = createMockJob({
      id: 3000n,
      runId: 300n,
      status: 'Completed',
      conclusion: 'Success',
      // 2 hours ago, beyond the 1h TTL.
      completedAt: new Date(uiStore.nowMs - 2 * 60 * 60 * 1000).toISOString(),
    })
    const fresh = createMockJob({
      id: 3001n,
      runId: 300n,
      status: 'Completed',
      conclusion: 'Success',
      completedAt: new Date(uiStore.nowMs - 5 * 60 * 1000).toISOString(),
    })
    runStore.loadSnapshot([run], [expired, fresh], [], 3600)
    const visible = runStore.jobs.map((j) => j.id)
    expect(visible).toContain(3001n)
    expect(visible).not.toContain(3000n)
  })

  it('frontend predicate agrees with the server predicate on borderline cases', () => {
    // Three (now, completed_at, ttl) tuples — the same triples that the
    // server-side cutoff comparison evaluates. Predicate-parity check:
    // a row at exactly the boundary stays visible (>=), one millisecond
    // past drops, one millisecond before stays.
    const ttlSec = 3600

    const boundary = createMockRun({
      id: 400n,
      status: 'Completed',
      conclusion: 'Success',
      // Exactly at the cutoff: completedAt + ttl === now.
      completedAt: new Date(uiStore.nowMs - ttlSec * 1000).toISOString(),
      updatedAt: '2026-04-17T09:00:00Z',
    })
    const justExpired = createMockRun({
      id: 401n,
      status: 'Completed',
      conclusion: 'Success',
      // 1ms older than the boundary — must drop.
      completedAt: new Date(uiStore.nowMs - ttlSec * 1000 - 1).toISOString(),
      updatedAt: '2026-04-17T09:00:00Z',
    })
    const justVisible = createMockRun({
      id: 402n,
      status: 'Completed',
      conclusion: 'Success',
      // 1ms newer than the boundary — must stay.
      completedAt: new Date(uiStore.nowMs - ttlSec * 1000 + 1).toISOString(),
      updatedAt: '2026-04-17T09:00:00Z',
    })

    runStore.loadSnapshot([boundary, justExpired, justVisible], [], [], ttlSec)
    const visible = runStore.completedRuns.map((r) => r.id)
    expect(visible).not.toContain(401n)
    expect(visible).toContain(402n)
    // The server SQL uses `completed_at >= cutoff`, so the boundary row stays.
    expect(visible).toContain(400n)
  })
})

describe('RunStore.loadSnapshot — displayTtlSeconds wiring', () => {
  let runStore: typeof import('./runs.svelte')['runStore']
  let createMockRun: typeof import('$lib/test-utils/factories')['createMockRun']

  beforeEach(async () => {
    mockLocalStorage.clear()
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-17T10:00:00Z'))
    vi.resetModules()
    await import('./ui.svelte')
    runStore = (await import('./runs.svelte')).runStore
    createMockRun = (await import('$lib/test-utils/factories')).createMockRun
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
    vi.useRealTimers()
    mockLocalStorage.clear()
  })

  it('stores the displayTtlSeconds parameter from loadSnapshot', () => {
    runStore.loadSnapshot([createMockRun()], [], [], 7200)
    expect(runStore.displayTtlSeconds).toBe(7200)
  })

  it('defaults displayTtlSeconds to 0 when the parameter is omitted', () => {
    runStore.loadSnapshot([createMockRun()], [])
    expect(runStore.displayTtlSeconds).toBe(0)
  })

  it('clear() resets displayTtlSeconds to 0', () => {
    runStore.loadSnapshot([createMockRun()], [], [], 3600)
    expect(runStore.displayTtlSeconds).toBe(3600)
    runStore.clear()
    expect(runStore.displayTtlSeconds).toBe(0)
  })
})
