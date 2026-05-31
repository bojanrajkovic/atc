import { SvelteMap } from 'svelte/reactivity'
import { summarizeRunners } from '$lib/format/runners'
import { uiStore } from '$lib/stores/ui.svelte'
import type { Job } from '$lib/types/generated/Job'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { JobStatus } from '$lib/types/generated/JobStatus'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'
import type { RunnerPoolCapacity } from '$lib/types/generated/RunnerPoolCapacity'
import type { RunStatus } from '$lib/types/generated/RunStatus'
import type { Step } from '$lib/types/generated/Step'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

export interface JobStats {
  readonly completed: number
  readonly total: number
  readonly runnerSummary: string | null
}

/**
 * Display-TTL filter predicate, applied identically to runs and jobs.
 *
 * Mirrors the server-side SQL `WHERE` and the in-memory store's
 * `run_passes_cutoff` / `job_passes_cutoff`. Three escape hatches keep
 * the row visible:
 *   1. `displayTtlSeconds === 0` — no filter armed (default for snapshots
 *      from pre-feature replicas during a rolling deploy).
 *   2. Status is anything other than `'Completed'`.
 *   3. `completedAt` is missing or null — `null` on the wire for a row
 *      that completed before the migration backfill landed; `undefined`
 *      when a pre-feature replica omits the field entirely. Both treated
 *      as "no cutoff applies yet" so a mixed-version snapshot cannot
 *      accidentally hide rows the user expects to see.
 */
function isExpired(
  status: RunStatus | JobStatus,
  completedAt: string | null | undefined,
  displayTtlSeconds: number,
  nowMs: number,
): boolean {
  if (displayTtlSeconds === 0) return false
  if (status !== 'Completed') return false
  if (!completedAt) return false
  // Server SQL uses `completed_at >= cutoff` (keeps the row at exactly the
  // boundary), where `cutoff = now - ttl`. Rearranged: keep iff
  // `now - completed_at <= ttl`; expire iff `now - completed_at > ttl`.
  // Strict `>` here mirrors the SQL — agreement matters for the
  // borderline parity check.
  return nowMs - Date.parse(completedAt) > displayTtlSeconds * 1000
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
  /**
   * Display TTL in seconds, mirrored from the snapshot's `displayTtlSeconds`.
   *
   * `0` means "no filter armed" — the snapshot came from a pre-feature
   * backend replica during a rolling deploy, or the operator deliberately
   * disabled the gate. Replaced atomically on every `loadSnapshot()` so a
   * reconnect against a freshly-rolled pod picks up the latest configured
   * value. Drives the `completedRuns` and `jobs` derivers below, paired
   * with `uiStore.nowMs` so completed rows age out reactively without an
   * event arriving.
   */
  displayTtlSeconds = $state<number>(0)

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

  /**
   * Completed runs visible to the kanban "Completed" column.
   *
   * The TTL filter reads `uiStore.nowMs` inside the deriver body so a tick
   * past the threshold reactively drops rows without an event arriving.
   * (Capturing `nowMs` into a local before the deriver expression would
   * sever the reactive dependency — uiStore.nowMs MUST be touched inside
   * `$derived.by`.) Rows without `completedAt` stay visible — see
   * `isExpired` for the predicate.
   */
  completedRuns = $derived.by(() =>
    [...this.runs.values()]
      .filter(
        (r) =>
          r.status === 'Completed' &&
          !isExpired(r.status, r.completedAt, this.displayTtlSeconds, uiStore.nowMs),
      )
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
    for (const [runId, run] of this.runs) {
      // Filter to the run's current attempt so a re-run's card doesn't count
      // the previous attempt's jobs (GitHub assigns fresh job IDs per attempt
      // under the same run_id). Mirrors the backend read filter.
      const jobs = (this.jobsByRun.get(runId) ?? []).filter((j) => j.runAttempt === run.runAttempt)
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
      // When the parent run is known, show only its current attempt's jobs.
      // Unknown parent (job-before-run stub) → keep all; the attempt can't be
      // compared yet and self-heals once the run event lands.
      const run = this.runs.get(runId)
      result.set(runId, run ? jobs.filter((j) => j.runAttempt === run.runAttempt) : jobs)
    }
    return result
  })

  /**
   * Flat view across all runs, with the same display-TTL filter as
   * `completedRuns`. `jobsByRun` (the run-keyed map) is intentionally
   * left unfiltered — `RunDetailPanel` still needs the full job list for
   * any visible run, and run-level filtering already culls the runs
   * whose jobs would otherwise be shown anywhere else.
   */
  jobs = $derived.by<Job[]>(() => {
    const result: Job[] = []
    for (const [runId, arr] of this.jobsByRun) {
      const run = this.runs.get(runId)
      for (const job of arr) {
        // Drop prior-attempt jobs once the parent run has advanced.
        if (run && job.runAttempt !== run.runAttempt) continue
        if (!isExpired(job.status, job.completedAt, this.displayTtlSeconds, uiStore.nowMs)) {
          result.push(job)
        }
      }
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
    // GitHub re-runs reuse the same run_id with a higher run_attempt. When a
    // newer attempt arrives we must NOT carry the prior attempt's terminal
    // fields forward — otherwise a reopened run would keep showing its old
    // conclusion / completedAt. Mirrors the backend reset (atc-store-pg's
    // CASE-on-attempt UPSERT and atc-store-mem's fresh-start bypass).
    const isNewAttempt = existing !== undefined && envelope.runAttempt > existing.runAttempt
    const carryExisting = isNewAttempt ? undefined : existing

    // Mirror the backend's `envelope.completed_at.or(existing.completed_at)`:
    // envelope wins when defined, existing carries through otherwise. The
    // field is `completedAt?: string` (TS optional), so we only include it
    // when defined — `exactOptionalPropertyTypes: true` rejects explicit
    // `completedAt: undefined`. Without this carry, a WS Completed event
    // would leave `completedAt` undefined and the display-TTL filter would
    // never expire the row until the next snapshot fetch. On a new attempt
    // `carryExisting` is undefined, so the stale completedAt is dropped.
    const completedAt = envelope.completedAt ?? carryExisting?.completedAt
    const completedAtPatch = completedAt === undefined ? {} : { completedAt }

    const run: WorkflowRun = carryExisting
      ? {
          ...carryExisting,
          ...completedAtPatch,
          status,
          conclusion: conclusion ?? carryExisting.conclusion,
          // Preserve optional fields that may be absent in some events
          workflowName: envelope.workflowName ?? carryExisting.workflowName,
          workflowPath: envelope.workflowPath ?? carryExisting.workflowPath,
          runStartedAt: envelope.runStartedAt ?? carryExisting.runStartedAt,
          // Overwrite fields that the backend always replaces
          branch: envelope.branch,
          headSha: envelope.headSha,
          commitMessage: envelope.commitMessage,
          displayTitle: envelope.displayTitle,
          htmlUrl: envelope.htmlUrl,
          updatedAt: envelope.updatedAt,
          runAttempt: envelope.runAttempt,
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
          runAttempt: envelope.runAttempt,
          ...completedAtPatch,
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
        runAttempt: envelope.runAttempt,
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
        runAttempt: envelope.runAttempt,
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
    displayTtlSeconds: number = 0,
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
    this.displayTtlSeconds = displayTtlSeconds
  }

  clear(): void {
    this.runs.clear()
    this.jobsByRun.clear()
    this.runnerPoolCapacities = []
    this.displayTtlSeconds = 0
  }
}

export const runStore = new RunStore()
