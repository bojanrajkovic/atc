import type { Job } from '$lib/types/generated/Job'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
import { runStore } from './runs.svelte'

/** Frontend replica of backend `StateStore::pool_stats()`.
 *  - LabelSet equivalence: dedupe via Set, then sort.
 *  - Collision-free map key: JSON.stringify on the deduped sorted array.
 *  - Bigint-aware: `groupId === 0n` (RunnerInfo.groupId is bigint | null). */
export function computePoolStats(jobs: Job[]): RunnerPoolStats[] {
  const statsMap = new Map<string, RunnerPoolStats>()

  for (const job of jobs) {
    if (job.status === 'Waiting' || job.status === 'Completed') continue

    const sortedLabels = [...new Set(job.labels)].sort()
    const key = JSON.stringify(sortedLabels)
    if (!statsMap.has(key)) {
      statsMap.set(key, {
        labels: sortedLabels,
        queued: 0,
        running: 0,
        groupName: null,
        isElastic: false,
        total: null,
      })
    }
    const entry = statsMap.get(key)!

    if (job.status === 'Queued') {
      entry.queued++
    } else if (job.status === 'InProgress') {
      entry.running++
      if (job.runner?.groupName != null) {
        entry.groupName = job.runner.groupName
      }
      if (job.runner?.groupId === 0n) {
        entry.isElastic = true
      }
    }
  }

  return [...statsMap.values()].sort((a, b) => {
    const ka = JSON.stringify(a.labels)
    const kb = JSON.stringify(b.labels)
    return ka < kb ? -1 : ka > kb ? 1 : 0
  })
}

class RunnerStore {
  readonly pools = $derived.by(() => computePoolStats(runStore.jobs))
}

export const runnerStore = new RunnerStore()
