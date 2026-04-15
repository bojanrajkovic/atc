import { runStore } from '$lib/stores/runs.svelte'
import type { SeqEvent } from '$lib/types/generated/SeqEvent'
import type { WebhookEvent } from '$lib/types/generated/WebhookEvent'

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

  private processBuffer(): void {
    this.rafId = null
    const events = this.buffer
    this.buffer = []
    for (const seqEvent of events) {
      this.routeEvent(seqEvent.event)
    }
  }

  private routeEvent(event: WebhookEvent): void {
    switch (event.type) {
      case 'Run':
        runStore.applyRunEvent(event.data)
        break
      case 'Job':
        runStore.applyJobEvent(event.data)
        break
    }
  }
}

export const eventDispatcher = new EventDispatcher()
