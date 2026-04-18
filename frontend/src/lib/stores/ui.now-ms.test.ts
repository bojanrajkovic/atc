import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Shared mock localStorage (ui.svelte.ts reads/writes localStorage in its
// $effect.root for theme/mode/density persistence — the singleton module
// dies without it).
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

describe('uiStore.nowMs', () => {
  let uiStore: typeof import('./ui.svelte')['uiStore']

  beforeEach(async () => {
    mockLocalStorage.clear()
    // CRITICAL ORDER: install fake timers BEFORE resetting/importing the
    // module, so the singleton's constructor-level setInterval lands on the
    // virtual-time queue.
    vi.useFakeTimers()
    vi.setSystemTime(new Date('2026-04-17T10:00:00Z'))
    vi.resetModules()
    const mod = await import('./ui.svelte')
    uiStore = mod.uiStore
  })

  afterEach(() => {
    uiStore.destroy()
    // AC2.5: Verify no leaked timers survive the test (check before switching to real timers)
    expect(vi.getTimerCount()).toBe(0)
    vi.useRealTimers()
    mockLocalStorage.clear()
  })

  // AC2.1: uiStore.nowMs is a public number field backed by $state(Date.now())
  it('AC2.1: should initialize nowMs as epoch milliseconds', () => {
    expect(typeof uiStore.nowMs).toBe('number')
    const expectedTime = new Date('2026-04-17T10:00:00Z').getTime()
    expect(uiStore.nowMs).toBe(expectedTime)
  })

  // AC2.2: Advancing the clock by 1050ms causes nowMs to update
  it('AC2.2: should update nowMs after setInterval fires', () => {
    const t0 = uiStore.nowMs
    vi.advanceTimersByTime(1050)
    expect(uiStore.nowMs).toBeGreaterThanOrEqual(t0 + 1000)
  })

  // AC2.3: destroy() clears the interval; subsequent advances do NOT update
  it('AC2.3: should stop updating nowMs after destroy()', () => {
    // First, advance to observe a tick
    vi.advanceTimersByTime(1050)
    const afterFirstTick = uiStore.nowMs
    expect(afterFirstTick).toBeGreaterThan(0)

    // Destroy the interval
    uiStore.destroy()
    const afterDestroy = uiStore.nowMs

    // Advance time again — should NOT update nowMs
    vi.advanceTimersByTime(2000)
    expect(uiStore.nowMs).toBe(afterDestroy)
  })

  // AC2.4: Re-constructing UIStore after destroy() restarts the interval
  it('AC2.4: should restart interval cleanly after reconstruction', async () => {
    // Advance once on the original instance
    vi.advanceTimersByTime(1050)
    const originalTick = uiStore.nowMs
    expect(originalTick).toBeGreaterThan(0)

    // Destroy and discard the reference
    uiStore.destroy()
    uiStore = null as unknown as typeof uiStore

    // Re-import to get a fresh singleton
    vi.resetModules()
    const newMod = await import('./ui.svelte')
    uiStore = newMod.uiStore

    // Reset system time for the fresh instance (it initialized at module load)
    const newInstanceInitial = uiStore.nowMs
    expect(typeof newInstanceInitial).toBe('number')

    // Advance and verify the new instance's interval works
    vi.advanceTimersByTime(1050)
    expect(uiStore.nowMs).toBeGreaterThan(newInstanceInitial)
  })

  // AC2.5: All tests run under fake timers; no real setInterval leaks
  // (verification happens in afterEach via vi.getTimerCount() === 0)
  it('AC2.5: should run all tests under fake timers', () => {
    expect(vi.isFakeTimers()).toBe(true)
  })
})
