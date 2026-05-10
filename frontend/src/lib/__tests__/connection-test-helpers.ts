// Mock WebSocket connections for testing
export class MockWebSocket {
  url: string
  readyState = 0 // CONNECTING
  onopen: ((event: Event) => void) | null = null
  onclose: ((event: CloseEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  messageQueue: string[] = []

  static instances: MockWebSocket[] = []

  constructor(url: string) {
    this.url = url
    MockWebSocket.instances.push(this)
    // Simulate connection delay using Promise microtask (more reliable than setTimeout(0)).
    // Match real-browser semantics: if close() landed before the open microtask
    // drained (readyState already CLOSED), do NOT fire onopen — real WebSockets
    // skip straight to onclose in this case.
    Promise.resolve().then(() => {
      if (this.readyState === 3) return
      this.readyState = 1 // OPEN
      this.onopen?.(new Event('open'))
    })
  }

  send(data: string): void {
    this.messageQueue.push(data)
  }

  close(code?: number): void {
    this.readyState = 3 // CLOSED
    // Use Promise to ensure the close event is handled asynchronously
    Promise.resolve().then(() => {
      this.onclose?.(new CloseEvent('close', { code: code ?? 1000 }))
    })
  }

  // Helper to simulate receiving a message
  receiveMessage(data: string): void {
    Promise.resolve().then(() => {
      this.onmessage?.(new MessageEvent('message', { data }))
    })
  }

  static clearAll(): void {
    MockWebSocket.instances = []
  }

  static getLastInstance(): MockWebSocket | undefined {
    return MockWebSocket.instances[MockWebSocket.instances.length - 1]
  }
}

import { HttpResponse, http } from 'msw'
import { setupServer } from 'msw/node'
import type { StateSnapshot } from '$lib/types/generated/StateSnapshot'

// Helper to serialize snapshots with BigInt → number (matching serde's i64/u64 JSON output)
export const snapshotToJSON = (snapshot: StateSnapshot) => {
  return JSON.parse(
    JSON.stringify(snapshot, (_key, value) => {
      if (typeof value === 'bigint') {
        return Number(value)
      }
      return value
    }),
  )
}

// Default state snapshot for most tests
export const defaultSnapshot: StateSnapshot = {
  lastSeq: 5n,
  runs: [],
  jobs: [],
}

// Setup test server with WebSocket mock
export const setupConnectionTestServer = () => {
  const server = setupServer(
    // Default state handler
    http.get('http://localhost:*/v1/state', () => {
      return HttpResponse.json(snapshotToJSON(defaultSnapshot))
    }),
  )

  return server
}
