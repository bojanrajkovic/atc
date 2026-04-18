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
