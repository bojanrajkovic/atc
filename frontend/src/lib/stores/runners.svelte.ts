import { poolKey } from '$lib/filters/pool'
import type { Job } from '$lib/types/generated/Job'
import type { RunnerPoolCapacity } from '$lib/types/generated/RunnerPoolCapacity'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
import type { RunnerPoolTotal } from '$lib/types/generated/RunnerPoolTotal'
import { runStore } from './runs.svelte'

/** Derives runner pool statistics from a flat job list and merges in
 *  operator-declared capacities by canonical label-set key.
 *
 *  - LabelSet equivalence: dedupe via Set, then sort.
 *  - Collision-free map key: JSON.stringify on the deduped sorted array.
 *  - Three-state `total`: `Bounded(n)` when the operator declared an integer
 *    `capacity`, `Unbounded` when the operator declared `capacity: null`,
 *    `Undeclared` when the pool is observed via webhook traffic only.
 *  - Capacity merge: keyed by `poolKey()` (ADR 0001) so insertion-order
 *    differences between the wire payload and the derived label set don't
 *    affect the match. */
export function computePoolStats(
  jobs: Job[],
  capacities: readonly RunnerPoolCapacity[] = [],
): RunnerPoolStats[] {
  const statsMap = new Map<string, RunnerPoolStats>()

  // Build the capacity lookup once per call. `poolKey()` re-sorts on its own,
  // so wire-side canonical order doesn't have to match JS's sort order.
  const capacityByKey = new Map<string, number | null>()
  for (const cap of capacities) {
    capacityByKey.set(poolKey(cap.labels), cap.capacity)
  }

  for (const job of jobs) {
    if (job.status === 'Waiting' || job.status === 'Completed') continue

    const sortedLabels = [...new Set(job.labels)].sort()
    const key = JSON.stringify(sortedLabels)
    if (!statsMap.has(key)) {
      const lookupKey = poolKey(sortedLabels)
      const total: RunnerPoolTotal = capacityByKey.has(lookupKey)
        ? capacityByKey.get(lookupKey) === null
          ? { kind: 'Unbounded' }
          : { kind: 'Bounded', value: capacityByKey.get(lookupKey) as number }
        : { kind: 'Undeclared' }
      statsMap.set(key, {
        labels: sortedLabels,
        queued: 0,
        running: 0,
        groupName: null,
        total,
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
    }
  }

  return [...statsMap.values()].sort((a, b) => {
    const la = a.labels
    const lb = b.labels
    const len = Math.min(la.length, lb.length)
    for (let i = 0; i < len; i++) {
      if (la[i]! < lb[i]!) return -1
      if (la[i]! > lb[i]!) return 1
    }
    return la.length - lb.length
  })
}

class RunnerStore {
  readonly pools = $derived.by(() => {
    // GitHub does not emit workflow_job events for jobs that were Queued but
    // never started when a run is cancelled. Filter those orphans out so they
    // don't inflate the queued count in the runner bar.
    const liveJobs = runStore.jobs.filter((j) => runStore.runs.get(j.runId)?.status !== 'Completed')
    return computePoolStats(liveJobs, runStore.runnerPoolCapacities)
  })
}

export const runnerStore = new RunnerStore()
