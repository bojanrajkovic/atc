import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { formatDuration, parseIso } from './duration'

/**
 * Compute the duration text shown in a RunCard's JobHeader.
 *
 * Pure function — no store reads, no side effects. Accepts `nowMs` as an
 * explicit parameter so callers (RunCard's $derived, tests) control it.
 *
 * The 'static' branch (Completed without ActionRequired) is independent of
 * nowMs; RunCard's $derived wraps this function so that static runs do not
 * subscribe to the wall-clock tick (see AC12.7).
 */
export function computeDurationText(
  run: Pick<WorkflowRun, 'status' | 'conclusion' | 'runStartedAt' | 'createdAt' | 'updatedAt'>,
  nowMs: number,
): string {
  if (run.status === 'Completed' && run.conclusion !== 'ActionRequired') {
    if (run.runStartedAt === null) return '\u2014'
    return formatDuration({
      kind: 'static',
      startMs: parseIso(run.runStartedAt),
      endMs: parseIso(run.updatedAt),
    })
  }
  if (run.status === 'Queued') {
    return `waiting ${formatDuration({
      kind: 'live',
      startMs: parseIso(run.createdAt),
      nowMs,
    })}`
  }
  if (run.status === 'InProgress') {
    const startIso = run.runStartedAt ?? run.createdAt
    return formatDuration({ kind: 'live', startMs: parseIso(startIso), nowMs })
  }
  // Completed + ActionRequired
  return `awaiting action ${formatDuration({
    kind: 'live',
    startMs: parseIso(run.updatedAt),
    nowMs,
  })}`
}

/**
 * Compute the duration text for a Job. Mirrors computeDurationText's
 * static/live branching using job-level fields.
 *
 * Pure function — no store reads, no side effects. Accepts `nowMs` as an
 * explicit parameter so callers control the clock.
 */
export function computeJobDurationText(
  job: Pick<Job, 'status' | 'conclusion' | 'startedAt' | 'completedAt' | 'createdAt'>,
  nowMs: number,
): string {
  if (job.status === 'Completed' && job.conclusion !== 'ActionRequired') {
    if (job.startedAt === null) return '—'
    if (job.completedAt === null) return '—'
    return formatDuration({
      kind: 'static',
      startMs: parseIso(job.startedAt),
      endMs: parseIso(job.completedAt),
    })
  }
  if (job.status === 'Queued' || job.status === 'Waiting') {
    return `waiting ${formatDuration({
      kind: 'live',
      startMs: parseIso(job.createdAt),
      nowMs,
    })}`
  }
  if (job.status === 'InProgress') {
    const startIso = job.startedAt ?? job.createdAt
    return formatDuration({ kind: 'live', startMs: parseIso(startIso), nowMs })
  }
  // Completed + ActionRequired
  if (job.completedAt === null) return '—'
  return `awaiting action ${formatDuration({
    kind: 'live',
    startMs: parseIso(job.completedAt),
    nowMs,
  })}`
}
