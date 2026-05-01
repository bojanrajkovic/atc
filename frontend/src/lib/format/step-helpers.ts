import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { Step } from '$lib/types/generated/Step'
import { formatDuration, parseIso } from './duration'
import type { StatusKey } from './status-key'

/**
 * Map a Step's (status, conclusion) pair to a StatusKey.
 *
 * Mirrors `resolveJobStatusKey` from status-key.ts but operates on StepStatus,
 * which has only three variants (Queued | InProgress | Completed) with no
 * "Waiting" variant.
 */
export function computeStepStatusKey(step: Step): StatusKey {
  if (step.status === 'Queued') return 'Queued'
  if (step.status === 'InProgress') return 'InProgress'
  // step.status === 'Completed' — must be exhaustive on JobConclusion.
  if (step.conclusion === null) return 'Cancelled' // bare-Completed fallback
  return stepConclusionToKey(step.conclusion)
}

function stepConclusionToKey(conclusion: JobConclusion): StatusKey {
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
      throw new Error(`Unhandled step conclusion: ${String(_exhaustive)}`)
    }
  }
}

/**
 * Compute a static duration string for a step.
 *
 * Returns the MM:SS (or H:MM:SS) interval between `startedAt` and
 * `completedAt`. Returns the em-dash character if either timestamp is absent
 * (step not yet started, or still running without a completion time).
 */
export function computeStepDurationText(step: Step): string {
  if (step.startedAt === null || step.completedAt === null) return '—'
  return formatDuration({
    kind: 'static',
    startMs: parseIso(step.startedAt),
    endMs: parseIso(step.completedAt),
  })
}
