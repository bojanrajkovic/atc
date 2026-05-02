import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

class EventDispatcher {
  private buffer: SeqEvent[] = []
  private rafId: number | null = null
  private onFlushCb: ((events: ReadonlyArray<SeqEvent>) => void) | null = null

  dispatch(event: SeqEvent): void {
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
  setOnFlush(cb: ((events: ReadonlyArray<SeqEvent>) => void) | null): void {
    this.onFlushCb = cb
  }

  /** Read-only getter exposing current buffer length. Used by E2E harness
   *  (sendWSBatch synchronization fence: waitForFunction(() => bufferLength === 0)). */
  get bufferLength(): number {
    return this.buffer.length
  }

  private processBuffer(): void {
    this.rafId = null
    const events = this.buffer
    this.buffer = []
    for (const seqEvent of events) {
      this.routeEvent(seqEvent)
    }
    // Invoke post-flush callback only when there were actual events to process.
    if (events.length > 0 && this.onFlushCb !== null) {
      this.onFlushCb(events)
    }
  }

  private routeEvent(seqEvent: SeqEvent): void {
    const event = seqEvent.event
    switch (event.type) {
      case 'Run':
        runStore.applyRunEvent(event.data)
        break
      case 'Job':
        runStore.applyJobEvent(event.data)
        break
      default: {
        const _: never = event
        throw new Error(`EventDispatcher.routeEvent: unhandled event type: ${JSON.stringify(_)}`)
      }
    }
    if (seqEvent.poolStatsAfter != null) {
      runnerStore.loadPools(seqEvent.poolStatsAfter)
    }
  }
}

export const eventDispatcher = new EventDispatcher()
