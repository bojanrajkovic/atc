import { SvelteMap } from 'svelte/reactivity'
import { summarizeRunners } from '$lib/format/runners'
import type { Job } from '$lib/types/generated/Job'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'
import type { RunnerPoolCapacity } from '$lib/types/generated/RunnerPoolCapacity'
import type { Step } from '$lib/types/generated/Step'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

export interface JobStats {
  readonly completed: number
  readonly total: number
  readonly runnerSummary: string | null
}

class RunStore {
  runs = new SvelteMap<bigint, WorkflowRun>()
  jobsByRun = new SvelteMap<bigint, Job[]>()
  /**
   * Operator-declared runner-pool capacities loaded from the latest snapshot.
   *
   * Replaced atomically by `loadSnapshot()`. Empty by default and on snapshots
   * that lack `runnerPoolCapacities` (older replicas during a rolling deploy)
   * — the wire field is `#[serde(default)]` on the backend, so a missing field
   * decodes to `[]` and the merge in `computePoolStats` is a no-op.
   */
  runnerPoolCapacities = $state<RunnerPoolCapacity[]>([])

  queuedRuns = $derived(
    [...this.runs.values()]
      .filter((r) => r.status === 'Queued')
      .sort((a, b) =>
        a.createdAt === b.createdAt
          ? a.id < b.id
            ? -1
            : a.id > b.id
              ? 1
              : 0
          : a.createdAt < b.createdAt
            ? -1
            : 1,
      ),
  )

  inProgressRuns = $derived(
    [...this.runs.values()]
      .filter((r) => r.status === 'InProgress')
      .sort((a, b) => {
        const aKey = a.runStartedAt ?? a.createdAt
        const bKey = b.runStartedAt ?? b.createdAt
        return aKey === bKey ? (a.id > b.id ? -1 : a.id < b.id ? 1 : 0) : aKey > bKey ? -1 : 1
      }),
  )

  completedRuns = $derived(
    [...this.runs.values()]
      .filter((r) => r.status === 'Completed')
      .sort((a, b) =>
        a.updatedAt === b.updatedAt
          ? a.id > b.id
            ? -1
            : a.id < b.id
              ? 1
              : 0
          : a.updatedAt > b.updatedAt
            ? -1
            : 1,
      ),
  )

  /**
   * Per-run job aggregate. Total-map: every runId present in `this.runs`
   * has a JobStats entry, even if jobsByRun has no entry for that run
   * (empty-jobs fallback: { completed: 0, total: 0, runnerSummary: null }).
   *
   * Iterates this.runs.keys() to establish the authoritative key set so
   * KanbanColumn can call `.get(run.id)!` without null handling.
   */
  jobStatsByRun = $derived.by<ReadonlyMap<bigint, JobStats>>(() => {
    const result = new Map<bigint, JobStats>()
    for (const runId of this.runs.keys()) {
      const jobs = this.jobsByRun.get(runId) ?? []
      const completed = jobs.filter((j) => j.status === 'Completed').length
      result.set(runId, {
        completed,
        total: jobs.length,
        runnerSummary: summarizeRunners(jobs),
      })
    }
    return result
  })

  /**
   * Per-run jobs view. Snapshot rebuilt on every job mutation, mirroring
   * jobStatsByRun's loop pattern. Consumers (e.g., pool filter in
   * KanbanColumn) get the raw Job[] arrays without needing to dip into the
   * internal SvelteMap directly.
   *
   * Iterates this.jobsByRun.entries() — the resulting Map preserves bigint
   * key identity (no string coercion). The total-map invariant of
   * jobStatsByRun does NOT apply here: only runs with at least one job
   * appear as keys.
   */
  jobsByRunId = $derived.by<ReadonlyMap<bigint, Job[]>>(() => {
    const result = new Map<bigint, Job[]>()
    for (const [runId, jobs] of this.jobsByRun) {
      result.set(runId, jobs)
    }
    return result
  })

  /** Flat view across all runs. */
  jobs = $derived.by<Job[]>(() => {
    const result: Job[] = []
    for (const arr of this.jobsByRun.values()) {
      for (const job of arr) result.push(job)
    }
    return result
  })

  applyRunEvent(envelope: RunEventEnvelope): void {
    const runId = envelope.runId

    // Determine status from the action type
    let status: 'Queued' | 'InProgress' | 'Completed'
    let conclusion: RunConclusion | null = null

    if (envelope.action.type === 'Requested') {
      status = 'Queued'
    } else if (envelope.action.type === 'InProgress') {
      status = 'InProgress'
    } else if (envelope.action.type === 'Completed') {
      status = 'Completed'
      conclusion = envelope.action.data.conclusion
    } else {
      // Exhaustiveness check - this should never be reached
      // @ts-expect-error - exhaustiveness check
      const _: never = envelope.action
      return
    }

    // Build the next run as a fresh object — never mutate the existing one in
    // place. SvelteMap.set short-circuits when the value reference is
    // unchanged, so an in-place mutation followed by .set(runId, sameRef)
    // would not invalidate per-key subscribers. Mirrors applyJobEvent's
    // immutable-update pattern and the backend atc-core CoW semantics.
    const existing = this.runs.get(runId)
    const run: WorkflowRun = existing
      ? {
          ...existing,
          status,
          conclusion: conclusion ?? existing.conclusion,
          // Preserve optional fields that may be absent in some events
          workflowName: envelope.workflowName ?? existing.workflowName,
          workflowPath: envelope.workflowPath ?? existing.workflowPath,
          runStartedAt: envelope.runStartedAt ?? existing.runStartedAt,
          // Overwrite fields that the backend always replaces
          branch: envelope.branch,
          headSha: envelope.headSha,
          commitMessage: envelope.commitMessage,
          displayTitle: envelope.displayTitle,
          htmlUrl: envelope.htmlUrl,
          updatedAt: envelope.updatedAt,
        }
      : {
          id: runId,
          org: envelope.org,
          repo: envelope.repo,
          workflowName: envelope.workflowName,
          workflowPath: envelope.workflowPath,
          branch: envelope.branch,
          headSha: envelope.headSha,
          commitMessage: envelope.commitMessage,
          event: envelope.triggerEvent,
          displayTitle: envelope.displayTitle,
          status,
          conclusion,
          htmlUrl: envelope.htmlUrl,
          createdAt: envelope.createdAt,
          runStartedAt: envelope.runStartedAt,
          updatedAt: envelope.updatedAt,
        }

    this.runs.set(runId, run)
  }

  applyJobEvent(envelope: JobEventEnvelope): void {
    const jobId = envelope.jobId
    const runId = envelope.runId

    // Determine status from the action type
    let status: 'Queued' | 'Waiting' | 'InProgress' | 'Completed'
    let conclusion: JobConclusion | null = null
    let runner: RunnerInfo | null = null
    let labels: Array<string> = []
    let steps: Array<Step> = []

    if (envelope.action.type === 'Queued') {
      status = 'Queued'
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'Waiting') {
      status = 'Waiting'
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'InProgress') {
      status = 'InProgress'
      runner = envelope.action.data.runner
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'Completed') {
      status = 'Completed'
      conclusion = envelope.action.data.conclusion
      runner = envelope.action.data.runner
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else {
      // Exhaustiveness check - this should never be reached
      // @ts-expect-error - exhaustiveness check
      const _: never = envelope.action
      return
    }

    // Get existing jobs array for this run
    const existing = this.jobsByRun.get(runId) ?? []
    const jobIndex = existing.findIndex((j) => j.id === jobId)

    // Create or update job
    let jobs: Job[]
    if (jobIndex === -1) {
      // New job: push to array
      const newJob: Job = {
        id: jobId,
        name: envelope.name,
        runId,
        status,
        conclusion,
        runner,
        labels,
        steps,
        createdAt: envelope.createdAt,
        startedAt: envelope.startedAt,
        completedAt: envelope.completedAt,
      }
      jobs = [...existing, newJob]
    } else {
      // Existing job: create new job object and new array for copy-on-write
      const prev = existing[jobIndex]
      if (!prev) {
        return
      }
      const updated: Job = {
        id: jobId,
        name: envelope.name,
        runId,
        status,
        conclusion: conclusion ?? prev.conclusion,
        runner: runner ?? prev.runner,
        labels,
        steps,
        createdAt: envelope.createdAt,
        startedAt: envelope.startedAt ?? prev.startedAt,
        completedAt: envelope.completedAt ?? prev.completedAt,
      }
      jobs = [...existing]
      jobs[jobIndex] = updated
    }

    this.jobsByRun.set(runId, jobs)
  }

  /**
   * Replace the operator-declared capacity list in-place.
   *
   * Called by `ConnectionManager` when a `ConfigUpdate` WireFrame arrives
   * (the backend's hot-reload watcher pushes the full new list, not a
   * delta). The atomic assignment invalidates the `$state` slice; derived
   * computations downstream (`computePoolStats`) recompute on the next read.
   */
  applyConfigUpdate(runnerPoolCapacities: RunnerPoolCapacity[]): void {
    this.runnerPoolCapacities = runnerPoolCapacities
  }

  loadSnapshot(
    runs: WorkflowRun[],
    jobs: Job[],
    runnerPoolCapacities: RunnerPoolCapacity[] = [],
  ): void {
    this.runs.clear()
    for (const r of runs) this.runs.set(r.id, r)

    // Group into a plain Map first; arrays must be fully built before they
    // reach the SvelteMap (push-into-an-already-installed-array would not
    // notify subscribers — see svelte#14409).
    const grouped = new Map<bigint, Job[]>()
    for (const job of jobs) {
      const arr = grouped.get(job.runId) ?? []
      arr.push(job)
      grouped.set(job.runId, arr)
    }

    this.jobsByRun.clear()
    for (const [runId, arr] of grouped) this.jobsByRun.set(runId, arr)

    this.runnerPoolCapacities = runnerPoolCapacities
  }

  clear(): void {
    this.runs.clear()
    this.jobsByRun.clear()
    this.runnerPoolCapacities = []
  }
}

export const runStore = new RunStore()
