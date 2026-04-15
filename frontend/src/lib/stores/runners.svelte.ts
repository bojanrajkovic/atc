import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'

class RunnerStore {
  pools = $state<RunnerPoolStats[]>([])

  loadPools(pools: RunnerPoolStats[]): void {
    this.pools = pools
  }

  clear(): void {
    this.pools = []
  }
}

export const runnerStore = new RunnerStore()
