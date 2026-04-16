import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
import { runnerStore } from './runners.svelte'

describe('RunnerStore', () => {
  beforeEach(() => {
    runnerStore.clear()
  })

  afterEach(() => {
    runnerStore.clear()
  })

  // AC3.6: RunnerStore.loadPools() replaces pool stats
  describe('AC3.6: Load and replace pool stats', () => {
    it('should load pools into the store', () => {
      const pools: RunnerPoolStats[] = [
        {
          labels: ['linux', 'x86_64'],
          queued: 2,
          running: 1,
          groupName: 'Default',
          isElastic: false,
          total: null,
        },
        {
          labels: ['windows', 'x86_64'],
          queued: 0,
          running: 3,
          groupName: 'Windows Runners',
          isElastic: false,
          total: null,
        },
      ]

      runnerStore.loadPools(pools)

      expect(runnerStore.pools.length).toBe(2)
      const pool0 = runnerStore.pools[0]
      const pool1 = runnerStore.pools[1]
      expect(pool0?.queued).toBe(2)
      expect(pool0?.running).toBe(1)
      expect(pool0?.groupName).toBe('Default')
      expect(pool1?.queued).toBe(0)
      expect(pool1?.running).toBe(3)
      expect(pool1?.groupName).toBe('Windows Runners')
    })

    it('should replace all pools when loadPools is called again', () => {
      const initialPools: RunnerPoolStats[] = [
        {
          labels: ['linux', 'x86_64'],
          queued: 1,
          running: 0,
          groupName: 'Old Pool',
          isElastic: false,
          total: null,
        },
      ]

      runnerStore.loadPools(initialPools)
      expect(runnerStore.pools.length).toBe(1)
      const initialPool0 = runnerStore.pools[0]
      expect(initialPool0?.groupName).toBe('Old Pool')

      // Load different pools
      const newPools: RunnerPoolStats[] = [
        {
          labels: ['macos', 'amd64'],
          queued: 5,
          running: 2,
          groupName: 'macOS',
          isElastic: false,
          total: null,
        },
        {
          labels: ['linux', 'arm64'],
          queued: 0,
          running: 1,
          groupName: 'ARM Linux',
          isElastic: false,
          total: null,
        },
        {
          labels: ['windows', 'x86_64'],
          queued: 3,
          running: 4,
          groupName: 'Windows',
          isElastic: false,
          total: null,
        },
      ]

      runnerStore.loadPools(newPools)

      // Verify old pools are completely replaced, not appended
      expect(runnerStore.pools.length).toBe(3)
      expect(runnerStore.pools.map((p) => p.groupName)).toContain('macOS')
      expect(runnerStore.pools.map((p) => p.groupName)).toContain('ARM Linux')
      expect(runnerStore.pools.map((p) => p.groupName)).toContain('Windows')
      expect(runnerStore.pools.map((p) => p.groupName)).not.toContain('Old Pool')
    })

    it('should handle loading an empty pool list', () => {
      const pools: RunnerPoolStats[] = [
        {
          labels: ['linux'],
          queued: 1,
          running: 0,
          groupName: 'Default',
          isElastic: false,
          total: null,
        },
      ]

      runnerStore.loadPools(pools)
      expect(runnerStore.pools.length).toBe(1)

      // Load empty list
      runnerStore.loadPools([])
      expect(runnerStore.pools.length).toBe(0)
    })

    it('should handle pools with null groupName', () => {
      const pools: RunnerPoolStats[] = [
        {
          labels: ['linux'],
          queued: 1,
          running: 0,
          groupName: null,
          isElastic: false,
          total: null,
        },
        {
          labels: ['windows'],
          queued: 2,
          running: 1,
          groupName: 'Windows Group',
          isElastic: false,
          total: null,
        },
      ]

      runnerStore.loadPools(pools)

      expect(runnerStore.pools.length).toBe(2)
      const pool0 = runnerStore.pools[0]
      const pool1 = runnerStore.pools[1]
      expect(pool0?.groupName).toBeNull()
      expect(pool1?.groupName).toBe('Windows Group')
    })
  })
})
