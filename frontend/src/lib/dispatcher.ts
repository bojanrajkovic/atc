import { runnerStore } from '$lib/stores/runners.svelte'
import { runStore } from '$lib/stores/runs.svelte'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'

class EventDispatcher {
  private buffer: SeqEvent[] = []
  private rafId: number | null = null

  dispatch(event: SeqEvent): void {
    this.buffer.push(event)
    if (this.rafId === null) {
      this.rafId = requestAnimationFrame(() => this.processBuffer())
    }
  }

  /** Process buffer synchronously. Used in tests to bypass RAF. */
  flush(): void {
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

  private processBuffer(): void {
    this.rafId = null
    const events = this.buffer
    this.buffer = []
    for (const seqEvent of events) {
      this.routeEvent(seqEvent)
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
    }
    if (seqEvent.poolStatsAfter != null) {
      runnerStore.loadPools(seqEvent.poolStatsAfter)
    }
  }
}

export const eventDispatcher = new EventDispatcher()
