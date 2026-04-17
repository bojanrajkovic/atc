import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createMockRunEvent } from '$lib/test-utils/factories'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  // AC3.4: Derived filters work correctly
  describe('AC3.4: Derived column filters', () => {
    it('should filter queuedRuns correctly', () => {
      const queued1 = 20n
      const queued2 = 21n
      const inProgress = 22n

      // Add queued runs
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued1,
          displayTitle: 'Run 1',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued2,
          displayTitle: 'Run 2',
          action: { type: 'Requested' },
        }),
      )

      // Add in-progress run
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          displayTitle: 'Run 3',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          displayTitle: 'Run 3',
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.queuedRuns.length).toBe(2)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued1)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued2)
    })

    it('should filter inProgressRuns correctly', () => {
      const inProgress1 = 30n
      const inProgress2 = 31n
      const queued = 32n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress1,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress2,
          runStartedAt: '2025-01-01T00:00:10Z',
          updatedAt: '2025-01-01T00:00:10Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: queued,
          runStartedAt: null,
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.inProgressRuns.length).toBe(2)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress1)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress2)
      expect(runStore.inProgressRuns.map((r) => r.id)).not.toContain(queued)
    })

    it('should filter completedRuns correctly', () => {
      const completed1 = 40n
      const completed2 = 41n
      const inProgress = 42n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: completed1,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: completed2,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:20Z',
          action: { type: 'Completed', data: { conclusion: 'Failure' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: inProgress,
          runStartedAt: '2025-01-01T00:00:05Z',
          updatedAt: '2025-01-01T00:00:10Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.completedRuns.length).toBe(2)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed1)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed2)
      expect(runStore.completedRuns.map((r) => r.id)).not.toContain(inProgress)
    })
  })

  // AC3.1-AC3.5, AC3.7: Sort order tests
  describe('AC3.1-AC3.7: Sort strategies', () => {
    // AC3.1: queuedRuns sorted ascending by createdAt
    it('AC3.1: queuedRuns sorted ascending by createdAt', () => {
      const runId1 = 100n
      const runId2 = 101n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Later run',
          createdAt: '2026-04-16T10:00:00Z',
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Earlier run',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.queuedRuns[0]?.id).toBe(runId2) // Earlier first
      expect(runStore.queuedRuns[1]?.id).toBe(runId1) // Later second
    })

    // AC3.2: inProgressRuns sorted descending by runStartedAt
    it('AC3.2: inProgressRuns sorted descending by runStartedAt', () => {
      const runId1 = 110n
      const runId2 = 111n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Earlier start',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Later start',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T10:00:05Z',
          updatedAt: '2026-04-16T10:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns[0]?.id).toBe(runId2) // Later start first
      expect(runStore.inProgressRuns[1]?.id).toBe(runId1) // Earlier start second
    })

    // AC3.3: inProgressRuns with null runStartedAt falls back to createdAt
    it('AC3.3: inProgressRuns with null runStartedAt falls back to createdAt', () => {
      const runIdNull = 120n
      const runIdWithStart = 121n

      // Run with null runStartedAt (uses createdAt for sort)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdNull,
          displayTitle: 'No start time',
          createdAt: '2026-04-16T10:00:00Z',
          runStartedAt: null,
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      // Transition to InProgress without runStartedAt (stays null)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdNull,
          displayTitle: 'No start time',
          createdAt: '2026-04-16T10:00:00Z',
          runStartedAt: null,
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'InProgress' },
        }),
      )

      // Run with runStartedAt set
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runIdWithStart,
          displayTitle: 'With start time',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns.length).toBe(2)
      // The null-fallback run (createdAt '2026-04-16T10:00:00Z') is later in time
      // than the started run ('2026-04-16T09:00:05Z'), so under descending sort it comes first
      expect(runStore.inProgressRuns[0]?.id).toBe(runIdNull) // null-fallback run (createdAt 10:00)
      expect(runStore.inProgressRuns[1]?.id).toBe(runIdWithStart) // started run (runStartedAt 09:00:05)
    })

    // AC3.4: completedRuns sorted descending by updatedAt
    it('AC3.4: completedRuns sorted descending by updatedAt', () => {
      const runId1 = 130n
      const runId2 = 131n

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Earlier update',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Later update',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:20Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      expect(runStore.completedRuns[0]?.id).toBe(runId2) // Later update first
      expect(runStore.completedRuns[1]?.id).toBe(runId1) // Earlier update second
    })

    // AC3.5: Tie-breaker tests using run.id
    it('AC3.5a: queuedRuns tie-breaker uses ascending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same createdAt - ordering should be determined by id (ascending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      expect(runStore.queuedRuns[0]?.id).toBe(runId2) // 1n
      expect(runStore.queuedRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.queuedRuns[2]?.id).toBe(runId1) // 3n
    })

    // AC3.5b: inProgressRuns tie-breaker uses descending id
    it('AC3.5b: inProgressRuns tie-breaker uses descending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same runStartedAt - ordering should be determined by id (descending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:05Z',
          action: { type: 'InProgress' },
        }),
      )

      expect(runStore.inProgressRuns[0]?.id).toBe(runId1) // 3n (descending)
      expect(runStore.inProgressRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.inProgressRuns[2]?.id).toBe(runId2) // 1n
    })

    // AC3.5c: completedRuns tie-breaker uses descending id
    it('AC3.5c: completedRuns tie-breaker uses descending id', () => {
      const runId1 = 3n
      const runId2 = 1n
      const runId3 = 2n

      // All have the same updatedAt - ordering should be determined by id (descending)
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 3',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId3,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          runStartedAt: '2026-04-16T09:00:05Z',
          updatedAt: '2026-04-16T09:00:15Z',
          action: { type: 'Completed', data: { conclusion: 'Success' } },
        }),
      )

      expect(runStore.completedRuns[0]?.id).toBe(runId1) // 3n (descending)
      expect(runStore.completedRuns[1]?.id).toBe(runId3) // 2n
      expect(runStore.completedRuns[2]?.id).toBe(runId2) // 1n
    })

    // AC3.7: Sort uses lexical comparison, not localeCompare
    // This test has two parts: behavioral verification and source-level assertion
    it('AC3.7: Sort implementation uses direct lexical comparison', () => {
      // Create runs with timestamps that would differ under locale-aware sorting
      const runId1 = 150n
      const runId2 = 151n

      // ISO-8601 timestamps: "2026-04-16T10:00:00Z" > "2026-04-16T09:00:00Z" lexically
      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId1,
          displayTitle: 'Run 1',
          createdAt: '2026-04-16T10:00:00Z',
          updatedAt: '2026-04-16T10:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      runStore.applyRunEvent(
        createMockRunEvent({
          runId: runId2,
          displayTitle: 'Run 2',
          createdAt: '2026-04-16T09:00:00Z',
          updatedAt: '2026-04-16T09:00:00Z',
          action: { type: 'Requested' },
        }),
      )

      // Behavioral: If using direct < comparison: '2026-04-16T09:00:00Z' < '2026-04-16T10:00:00Z' = true
      // runId2 should come before runId1
      expect(runStore.queuedRuns[0]?.id).toBe(runId2)
      expect(runStore.queuedRuns[1]?.id).toBe(runId1)

      // Source-level: Assert the implementation does not use localeCompare
      const storeSource = readFileSync(
        resolve(dirname(fileURLToPath(import.meta.url)), './runs.svelte.ts'),
        'utf-8',
      )
      expect(storeSource).not.toContain('localeCompare')
    })
  })
})
