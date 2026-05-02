import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

/**
 * A transition is the minimal classified form of a per-run announcement event.
 * Only Requested (queued) and Completed transitions produce announcements;
 * InProgress hops are filtered before classify.
 */
export type TransitionKind = { kind: 'queued' } | { kind: 'completed'; conclusion: RunConclusion }

/**
 * Verb dictionary for completed run conclusions.
 * `Record<RunConclusion, string>` gives compile-time exhaustiveness: adding a
 * new RunConclusion variant in atc-core and regenerating ts-rs types will fail
 * the frontend tsc step until this record is updated.
 */
export const VERB_BY_CONCLUSION: Record<RunConclusion, string> = {
  Success: 'succeeded',
  Failure: 'failed',
  Cancelled: 'cancelled',
  TimedOut: 'timed out',
  ActionRequired: 'requires action',
  Stale: 'went stale',
  Neutral: 'completed neutral',
  Skipped: 'was skipped',
  StartupFailure: 'failed to start',
}

// Compile-time exhaustiveness sentinel: verifies VERB_BY_CONCLUSION's keyset
// exactly equals the RunConclusion union. If RunConclusion grows a new variant,
// this type-level check will cause a tsc error until the new key is added above.
type Expect<T extends true> = T
type Equal<X, Y> =
  (<T>() => T extends X ? 1 : 2) extends <T>() => T extends Y ? 1 : 2 ? true : false
export type _CheckExhaustive = Expect<Equal<keyof typeof VERB_BY_CONCLUSION, RunConclusion>>

/**
 * Classify a SeqEvent as a TransitionKind for announcement purposes.
 *
 * Returns `null` for non-announcement events (InProgress, Job events, etc.).
 * Throws on invariant violations (e.g., a Completed RunEvent with
 * conclusion === null or undefined). The caller (LiveRegion.observeFlush) is
 * expected to wrap each call in try/catch and log+skip on throw.
 */
export function classifyEvent(seqEvent: SeqEvent): TransitionKind | null {
  const webhookEvent = seqEvent.event

  // Only RunEvents can be transitions
  if (webhookEvent.type !== 'Run') {
    return null
  }

  const action = webhookEvent.data.action

  switch (action.type) {
    case 'Requested':
      return { kind: 'queued' }

    case 'InProgress':
      // Intermediate hop — not announcement-relevant
      return null

    case 'Completed': {
      const conclusion = action.data.conclusion
      // Runtime guard for off-shape input: the wire schema types conclusion as
      // RunConclusion (non-nullable), but JSON from untrusted sources can slip
      // through. Throw so the caller can log and skip.
      if (conclusion == null) {
        throw new Error(
          `classifyEvent: invariant violation — Completed RunEvent has null/undefined conclusion. Event seq: ${seqEvent.seq}`,
        )
      }
      return { kind: 'completed', conclusion }
    }

    default: {
      // Exhaustiveness guard: if ts-rs generates a new RunEvent variant,
      // TypeScript will error here until classifyEvent handles it.
      const _: never = action
      throw new Error(`classifyEvent: unhandled RunEvent action type: ${JSON.stringify(_)}`)
    }
  }
}
