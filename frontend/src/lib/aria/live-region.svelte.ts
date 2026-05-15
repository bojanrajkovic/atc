import { runStore } from '$lib/stores/runs.svelte'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import { formatRunTransition } from './format-run-transition'
import { classifyEvent, VERB_BY_CONCLUSION } from './transition-kinds'

const BURST_THRESHOLD = 3
const BURST_DEBOUNCE_MS = 200

/**
 * Accumulated counts for a burst window.
 * Key format: 'queued' | 'completed:<conclusion>'.
 */
interface BurstAccumulator {
  active: boolean
  timerId: ReturnType<typeof setTimeout> | null
  queued: number
  // Per-conclusion counts keyed by RunConclusion string
  completed: Map<string, number>
}

export class LiveRegion {
  message: string = $state('')
  busy: boolean = $state(false)

  private burst: BurstAccumulator = {
    active: false,
    timerId: null,
    queued: 0,
    completed: new Map(),
  }

  /**
   * Called by EventDispatcher's setOnFlush callback after each RAF flush.
   * Walks the event list, classifies each into a TransitionKind, and either:
   * - Emits per-run messages (≤ BURST_THRESHOLD transitions in flush, no open burst)
   * - Opens/extends a BurstAccumulator (>BURST_THRESHOLD, or burst already open)
   *
   * Per-event try/catch: classifyEvent can throw on invariant violations.
   * A bad event is logged and skipped; remaining events in the batch still announce.
   */
  observeFlush(events: ReadonlyArray<CommittedEvent>): void {
    // Walk events and classify
    const transitions: Array<{
      committedEvent: CommittedEvent
      kind: ReturnType<typeof classifyEvent>
    }> = []

    for (const committedEvent of events) {
      try {
        const kind = classifyEvent(committedEvent)
        if (kind !== null) {
          transitions.push({ committedEvent, kind })
        }
      } catch (err) {
        console.error(
          `classifyEvent invariant violation: ${err instanceof Error ? err.message : String(err)}`,
          committedEvent,
        )
      }
    }

    if (transitions.length === 0) {
      return
    }

    // If burst is already open, add all transitions to the accumulator regardless of count
    if (this.burst.active) {
      this.accumulateTransitions(transitions)
      this.resetDebounce()
      return
    }

    // No burst open: check if this flush exceeds threshold
    if (transitions.length > BURST_THRESHOLD) {
      this.openBurst(transitions)
      return
    }

    // Below threshold, no open burst: emit per-run messages
    const messages: string[] = []
    for (const { committedEvent, kind } of transitions) {
      if (kind === null) continue // Already filtered above but TS narrows
      // Look up the run in the store for message formatting
      const runId = (committedEvent.event as { type: 'Run'; data: { runId: bigint } }).data.runId
      const run = runStore.runs.get(runId)
      if (run == null) {
        // Run not in store yet (edge case) — skip formatting
        continue
      }
      messages.push(formatRunTransition(run, kind))
    }

    if (messages.length > 0) {
      this.message = messages.join('. ')
    }
  }

  private openBurst(
    transitions: Array<{ committedEvent: CommittedEvent; kind: ReturnType<typeof classifyEvent> }>,
  ): void {
    this.burst.active = true
    this.busy = true
    this.burst.queued = 0
    this.burst.completed = new Map()
    this.accumulateTransitions(transitions)
    this.resetDebounce()
  }

  private accumulateTransitions(
    transitions: Array<{ committedEvent: CommittedEvent; kind: ReturnType<typeof classifyEvent> }>,
  ): void {
    for (const { kind } of transitions) {
      if (kind === null) continue
      if (kind.kind === 'queued') {
        this.burst.queued++
      } else if (kind.kind === 'completed') {
        const key = kind.conclusion
        this.burst.completed.set(key, (this.burst.completed.get(key) ?? 0) + 1)
      }
    }
  }

  private resetDebounce(): void {
    if (this.burst.timerId !== null) {
      clearTimeout(this.burst.timerId)
    }
    this.burst.timerId = setTimeout(() => {
      this.closeBurst()
    }, BURST_DEBOUNCE_MS)
  }

  /**
   * Cancel an in-flight burst without announcing. Called by ConnectionManager
   * on disconnect/reconnect: if observeFlush had opened a burst whose 200ms
   * debounce had not yet fired, the queued closeBurst() would otherwise
   * announce a stale summary while the app is already reconnecting. Resets
   * the accumulator, clears the timer, and drops aria-busy without touching
   * `message` (any prior announcement stays in the DOM as last-known state).
   */
  cancelBurst(): void {
    if (this.burst.timerId !== null) {
      clearTimeout(this.burst.timerId)
      this.burst.timerId = null
    }
    this.burst.active = false
    this.burst.queued = 0
    this.burst.completed = new Map()
    this.busy = false
  }

  private closeBurst(): void {
    const queued = this.burst.queued
    const completedMap = this.burst.completed
    const totalCompleted = [...completedMap.values()].reduce((a, b) => a + b, 0)

    const parts: string[] = []

    if (queued > 0) {
      parts.push(`${queued} ${queued === 1 ? 'run' : 'runs'} queued`)
    }

    if (totalCompleted > 0) {
      // Build per-conclusion breakdown, eliding absent conclusions
      const breakdownParts: string[] = []
      // Iterate over all RunConclusion variants in a defined order
      for (const [conclusion, verb] of Object.entries(VERB_BY_CONCLUSION)) {
        const count = completedMap.get(conclusion)
        if (count && count > 0) {
          breakdownParts.push(`${count} ${verb}`)
        }
      }
      const breakdown = breakdownParts.length > 0 ? ` (${breakdownParts.join(', ')})` : ''
      parts.push(`${totalCompleted} completed${breakdown}`)
    }

    this.message = `${parts.join(', ')}.`
    this.busy = false

    // Reset accumulator
    this.burst.active = false
    this.burst.timerId = null
    this.burst.queued = 0
    this.burst.completed = new Map()
  }
}

export const liveRegion = new LiveRegion()
