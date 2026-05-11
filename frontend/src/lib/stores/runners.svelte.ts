import type { Job } from '$lib/types/generated/Job'
import type { RunnerPoolStats } from '$lib/types/generated/RunnerPoolStats'
import { runStore } from './runs.svelte'

/** Derives runner pool statistics from a flat job list.
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
    return computePoolStats(liveJobs)
  })
}

export const runnerStore = new RunnerStore()
