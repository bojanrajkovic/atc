import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  describe('Create new run for unknown run ID', () => {
    it('should create a new run when given an envelope for an unknown run ID', () => {
      const runId = 1n
      const envelope: RunEventEnvelope = {
        runId,
        org: 'test-org',
        repo: 'test-repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'abc123',
        commitMessage: 'Test commit',
        triggerEvent: 'push',
        displayTitle: 'Test Run',
        htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      }

      runStore.applyRunEvent(envelope)

      expect(runStore.runs.has(runId)).toBe(true)
      const run = runStore.runs.get(runId)!
      expect(run.id).toBe(runId)
      expect(run.org).toBe('test-org')
      expect(run.repo).toBe('test-repo')
      expect(run.status).toBe('Queued')
    })

    it('should handle Requested action creating a Queued run', () => {
      const envelope: RunEventEnvelope = {
        runId: 2n,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      }

      runStore.applyRunEvent(envelope)

      const run = runStore.runs.get(2n)!
      expect(run.status).toBe('Queued')
      expect(run.conclusion).toBeNull()
    })
  })

  describe('Update existing run status and fields', () => {
    it('should update an existing run from Queued to InProgress', () => {
      const runId = 3n

      // Create initial Queued run
      const queuedEnvelope: RunEventEnvelope = {
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'message',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      }

      runStore.applyRunEvent(queuedEnvelope)
      expect(runStore.runs.get(runId)!.status).toBe('Queued')

      // Update to InProgress
      const inProgressEnvelope: RunEventEnvelope = {
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: null, // GitHub often sends null in subsequent events
        workflowPath: null,
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'message',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:10Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      }

      runStore.applyRunEvent(inProgressEnvelope)

      const updated = runStore.runs.get(runId)!
      expect(updated.status).toBe('InProgress')
      expect(updated.workflowName).toBe('CI') // Should be preserved from first event
      expect(updated.runStartedAt).toBe('2025-01-01T00:00:10Z')
    })

    it('should update from InProgress to Completed with conclusion', () => {
      const runId = 4n

      // Create Queued
      runStore.applyRunEvent({
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'msg',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      // Update to InProgress
      runStore.applyRunEvent({
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'msg',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:10Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      // Update to Completed
      runStore.applyRunEvent({
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'msg',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:10Z',
        updatedAt: '2025-01-01T00:00:20Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      })

      const completed = runStore.runs.get(runId)!
      expect(completed.status).toBe('Completed')
      expect(completed.conclusion).toBe('Success')
      expect(completed.workflowName).toBe('CI') // Preserved from first event
    })

    it('should preserve existing fields when new envelope has null values', () => {
      const runId = 5n

      // Initial event with all fields
      runStore.applyRunEvent({
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'MyWorkflow',
        workflowPath: '.github/workflows/my.yml',
        branch: 'develop',
        headSha: 'sha123',
        commitMessage: 'Initial commit',
        triggerEvent: 'push',
        displayTitle: 'Run 1',
        htmlUrl: 'url1',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      // Second event with nulls (typical for GitHub events)
      runStore.applyRunEvent({
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha123',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 1',
        htmlUrl: 'url1',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null, // But new startedAt should not overwrite if different
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      const run = runStore.runs.get(runId)!
      expect(run.workflowName).toBe('MyWorkflow') // Preserved (optional field)
      expect(run.workflowPath).toBe('.github/workflows/my.yml') // Preserved (optional field)
      expect(run.branch).toBeNull() // Overwritten (backend always replaces)
      expect(run.commitMessage).toBeNull() // Overwritten (backend always replaces)
    })
  })

  describe('Idempotent duplicate events', () => {
    it('should handle duplicate run events without creating duplicates', () => {
      const runId = 70n
      const envelope: RunEventEnvelope = {
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'msg',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      }

      // Apply same event twice
      runStore.applyRunEvent(envelope)
      expect(runStore.runs.size).toBe(1)
      const firstRun = runStore.runs.get(runId)!

      runStore.applyRunEvent(envelope)
      expect(runStore.runs.size).toBe(1) // Still 1, not 2
      const secondRun = runStore.runs.get(runId)!

      // Same run object (idempotent)
      expect(firstRun.status).toBe('Queued')
      expect(secondRun.status).toBe('Queued')
    })

    it('should handle the same status update multiple times without error', () => {
      const runId = 72n

      // Scenario: Completed event fires multiple times with same data
      const completionEnvelope: RunEventEnvelope = {
        runId,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'sha',
        commitMessage: 'msg',
        triggerEvent: 'push',
        displayTitle: 'Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:15Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      }

      // Apply multiple times
      runStore.applyRunEvent(completionEnvelope)
      expect(runStore.runs.get(runId)!.conclusion).toBe('Success')

      runStore.applyRunEvent(completionEnvelope)
      expect(runStore.runs.get(runId)!.conclusion).toBe('Success')
      expect(runStore.runs.size).toBe(1) // Still just one run
    })
  })
})
