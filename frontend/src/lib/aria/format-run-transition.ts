import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { type TransitionKind, VERB_BY_CONCLUSION } from './transition-kinds'

/**
 * Build the SR announcement string for a single run transition.
 *
 * Format: "Run {displayTitle} for {org}/{repo} on {branch} ({event}) {verb}"
 *
 * Branch elision: when `run.branch` is null, the "on {branch}" segment is
 * omitted entirely.
 *
 * Verb lookup: for Requested transitions, always "queued"; for Completed
 * transitions, the conclusion-specific verb from VERB_BY_CONCLUSION.
 */
export function formatRunTransition(run: WorkflowRun, transitionKind: TransitionKind): string {
  const branchSegment = run.branch == null ? '' : ` on ${run.branch}`

  let verb: string
  if (transitionKind.kind === 'queued') {
    verb = 'queued'
  } else if (transitionKind.kind === 'completed') {
    verb = VERB_BY_CONCLUSION[transitionKind.conclusion]
  } else {
    // Exhaustiveness guard
    const _: never = transitionKind
    throw new Error(`formatRunTransition: unhandled TransitionKind: ${JSON.stringify(_)}`)
  }

  return `Run ${run.displayTitle} for ${run.org}/${run.repo}${branchSegment} (${run.event}) ${verb}`
}
