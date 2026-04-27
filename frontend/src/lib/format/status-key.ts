import type { Job } from '$lib/types/generated/Job'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

/** Every value of StatusKey, in the canonical order used by Status Symbols. */
export const STATUS_KEYS = [
  'Queued',
  'InProgress',
  'Success',
  'Failure',
  'Cancelled',
  'TimedOut',
  'ActionRequired',
  'StartupFailure',
  'Stale',
  'Neutral',
  'Skipped',
] as const

export type StatusKey = (typeof STATUS_KEYS)[number]

/** Maps a StatusKey to a human-readable label (title-cased, space-separated). */
export function statusKeyToHumanLabel(key: StatusKey): string {
  switch (key) {
    case 'Queued':
      return 'Queued'
    case 'InProgress':
      return 'In progress'
    case 'Success':
      return 'Success'
    case 'Failure':
      return 'Failure'
    case 'Cancelled':
      return 'Cancelled'
    case 'TimedOut':
      return 'Timed out'
    case 'ActionRequired':
      return 'Action required'
    case 'StartupFailure':
      return 'Startup failure'
    case 'Stale':
      return 'Stale'
    case 'Neutral':
      return 'Neutral'
    case 'Skipped':
      return 'Skipped'
  }
}

/**
 * Normalize a WorkflowRun's (status, conclusion) pair into one of 11 StatusKey values.
 *
 * Accepts Pick<WorkflowRun, 'status' | 'conclusion'> to allow lightweight test inputs.
 */
export function resolveStatusKey(run: Pick<WorkflowRun, 'status' | 'conclusion'>): StatusKey {
  if (run.status === 'Queued') return 'Queued'
  if (run.status === 'InProgress') return 'InProgress'
  // run.status === 'Completed' — must be exhaustive on RunConclusion.
  if (run.conclusion === null) return 'Cancelled' // bare-Completed fallback (see docs/design-plans/2026-04-17-run-cards.md, "StatusKey normalization at the boundary")
  return conclusionToKey(run.conclusion)
}

function conclusionToKey(conclusion: RunConclusion): StatusKey {
  switch (conclusion) {
    case 'Success':
      return 'Success'
    case 'Failure':
      return 'Failure'
    case 'Cancelled':
      return 'Cancelled'
    case 'TimedOut':
      return 'TimedOut'
    case 'ActionRequired':
      return 'ActionRequired'
    case 'StartupFailure':
      return 'StartupFailure'
    case 'Stale':
      return 'Stale'
    case 'Neutral':
      return 'Neutral'
    case 'Skipped':
      return 'Skipped'
    default: {
      const _exhaustive: never = conclusion
      throw new Error(`Unhandled run conclusion: ${String(_exhaustive)}`)
    }
  }
}

/**
 * Normalize a Job's (status, conclusion) pair into one of 11 StatusKey values.
 *
 * Accepts Pick<Job, 'status' | 'conclusion'> to allow lightweight test inputs.
 */
export function resolveJobStatusKey(job: Pick<Job, 'status' | 'conclusion'>): StatusKey {
  if (job.status === 'Queued') return 'Queued'
  if (job.status === 'Waiting') return 'InProgress'
  if (job.status === 'InProgress') return 'InProgress'
  // job.status === 'Completed' — must be exhaustive on JobConclusion.
  if (job.conclusion === null) return 'Cancelled' // bare-Completed fallback
  return jobConclusionToKey(job.conclusion)
}

function jobConclusionToKey(conclusion: JobConclusion): StatusKey {
  switch (conclusion) {
    case 'Success':
      return 'Success'
    case 'Failure':
      return 'Failure'
    case 'Cancelled':
      return 'Cancelled'
    case 'TimedOut':
      return 'TimedOut'
    case 'ActionRequired':
      return 'ActionRequired'
    case 'Stale':
      return 'Stale'
    case 'Neutral':
      return 'Neutral'
    case 'Skipped':
      return 'Skipped'
    default: {
      const _exhaustive: never = conclusion
      throw new Error(`Unhandled job conclusion: ${String(_exhaustive)}`)
    }
  }
}
