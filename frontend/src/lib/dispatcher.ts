import { runStore } from '$lib/stores/runs.svelte'
import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'

/**
 * Tracks event types that have already triggered a console.warn so that a
 * long stream of the same unknown type does not spam the console.
 */
const warnedUnknownTypes = new Set<string>()

class EventDispatcher {
  private buffer: CommittedEvent[] = []
  private rafId: number | null = null
  private onFlushCb: ((events: ReadonlyArray<CommittedEvent>) => void) | null = null

  dispatch(event: CommittedEvent): void {
    this.buffer.push(event)
    if (this.rafId === null) {
      this.rafId = requestAnimationFrame(() => this.processBuffer())
    }
  }

  /** Process buffer synchronously. Used in tests and connection.ts to bypass RAF.
   *  Cancels any pending RAF before draining so that dispatch(); flush() produces
   *  exactly one non-empty callback rather than a real call followed by a phantom
   *  empty-array RAF callback. */
  flush(): void {
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId)
      this.rafId = null
    }
    this.processBuffer()
  }

  /** Drain buffer and cancel pending RAF. Call before loading a new snapshot. */
  clear(): void {
    this.buffer = []
    if (this.rafId !== null) {
      cancelAnimationFrame(this.rafId)
      this.rafId = null
    }
  }

  /**
   * Set a post-flush callback that is invoked with the flushed event list after
   * stores have been mutated. Only invoked when events.length > 0 (empty drains
   * do not invoke the callback). Pass null to detach the callback (reconnect
   * sequences use this to suppress announcements during snapshot + buffered-replay
   * drain).
   *
   * Idempotent: calling setOnFlush twice replaces the prior callback.
   */
  setOnFlush(cb: ((events: ReadonlyArray<CommittedEvent>) => void) | null): void {
    this.onFlushCb = cb
  }

  /** Read-only getter exposing current buffer length. Used by E2E harness
   *  (sendWSBatch / sendWSBatchPaced synchronization fence:
   *  waitForFunction(() => bufferLength === 0)). */
  get bufferLength(): number {
    return this.buffer.length
  }

  private processBuffer(): void {
    this.rafId = null
    const events = this.buffer
    this.buffer = []
    for (const committedEvent of events) {
      this.routeEvent(committedEvent)
    }
    // Invoke post-flush callback only when there were actual events to process.
    if (events.length > 0 && this.onFlushCb !== null) {
      this.onFlushCb(events)
    }
  }

  private routeEvent(committedEvent: CommittedEvent): void {
    const event = committedEvent.event
    switch (event.type) {
      case 'Run':
        runStore.applyRunEvent(event.data)
        break
      case 'Job':
        runStore.applyJobEvent(event.data)
        break
      default: {
        // At a JSON wire boundary, the event.type may not match any known
        // variant (e.g. newer backend, rolling deploy, or malformed payload).
        // Throwing aborts the entire RAF batch and leaves the dashboard stale,
        // which is worse than skipping. Warn once per unknown type, then skip
        // the entire committedEvent.
        const unknownType = (event as { type: string }).type
        if (!warnedUnknownTypes.has(unknownType)) {
          warnedUnknownTypes.add(unknownType)
          // Intentional operator warning: unknown event type from a newer backend or malformed
          // payload; deduped per type so it is not spam. biome-ignore on next line is intentional.
          // biome-ignore lint/suspicious/noConsole: operator-visible warning, not debugging output
          console.warn(
            `EventDispatcher.routeEvent: unknown event type "${unknownType}" — skipping (future occurrences of this type will be silenced)`,
          )
        }
        return
      }
    }
  }
}

export const eventDispatcher = new EventDispatcher()
