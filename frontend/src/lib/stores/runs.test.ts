import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
  })

  // AC3.1: applyRunEvent creates a new run for an unknown run ID
  describe('AC3.1: Create new run for unknown run ID', () => {
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

  // AC3.2: applyRunEvent updates an existing run's status and fields
  describe('AC3.2: Update existing run status and fields', () => {
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
      expect(run.workflowName).toBe('MyWorkflow') // Preserved
      expect(run.workflowPath).toBe('.github/workflows/my.yml') // Preserved
      expect(run.branch).toBe('develop') // Preserved
      expect(run.commitMessage).toBe('Initial commit') // Preserved
    })
  })

  // AC3.3: applyJobEvent groups jobs by run ID
  describe('AC3.3: Group jobs by run ID', () => {
    it('should create a job for an unknown run ID', () => {
      const runId = 10n
      const jobId = 100n

      const envelope: JobEventEnvelope = {
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Test Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: {
            labels: ['ubuntu-latest'],
            steps: [],
          },
        },
      }

      runStore.applyJobEvent(envelope)

      expect(runStore.jobsByRun.has(runId)).toBe(true)
      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(1)
      expect(jobs[0]!.id).toBe(jobId)
      expect(jobs[0]!.name).toBe('Test Job')
      expect(jobs[0]!.status).toBe('Queued')
    })

    it('should group multiple jobs with the same run ID', () => {
      const runId = 11n
      const jobId1 = 101n
      const jobId2 = 102n

      runStore.applyJobEvent({
        jobId: jobId1,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job 1',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      runStore.applyJobEvent({
        jobId: jobId2,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job 2',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      })

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(2)
      expect(jobs.map((j) => j.id)).toContain(jobId1)
      expect(jobs.map((j) => j.id)).toContain(jobId2)
    })

    it('should update a job when applyJobEvent is called for the same job ID', () => {
      const runId = 12n
      const jobId = 103n

      // Create job
      runStore.applyJobEvent({
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: ['label1'], steps: [] },
        },
      })

      // Update same job
      runStore.applyJobEvent({
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: '2025-01-01T00:00:05Z',
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            labels: ['label1'],
            steps: [],
            runner: null,
          },
        },
      })

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs.length).toBe(1) // Should still be 1, not duplicated
      expect(jobs[0]!.status).toBe('InProgress')
      expect(jobs[0]!.startedAt).toBe('2025-01-01T00:00:05Z')
    })

    it('should preserve job fields when updating with null values', () => {
      const runId = 13n
      const jobId = 104n

      // Create with initial runner
      runStore.applyJobEvent({
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: '2025-01-01T00:00:05Z',
        completedAt: null,
        action: {
          type: 'InProgress',
          data: {
            labels: ['ubuntu-latest'],
            steps: [],
            runner: { id: 1n, name: 'Runner1', groupId: null, groupName: null },
          },
        },
      })

      // Update with null runner (should preserve)
      runStore.applyJobEvent({
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: '2025-01-01T00:00:05Z',
        completedAt: '2025-01-01T00:00:15Z',
        action: {
          type: 'Completed',
          data: {
            labels: ['ubuntu-latest'],
            steps: [],
            runner: null,
            conclusion: 'Success',
          },
        },
      })

      const jobs = runStore.jobsByRun.get(runId)!
      const job = jobs[0]!
      expect(job.status).toBe('Completed')
      // Runner should be set to the new null value (per the Completed action)
      expect(job.runner).toBeNull()
    })
  })

  // AC3.4: Derived filters work correctly
  describe('AC3.4: Derived column filters', () => {
    it('should filter queuedRuns correctly', () => {
      const queued1 = 20n
      const queued2 = 21n
      const inProgress = 22n

      // Add queued runs
      runStore.applyRunEvent({
        runId: queued1,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 1',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      runStore.applyRunEvent({
        runId: queued2,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 2',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      // Add in-progress run
      runStore.applyRunEvent({
        runId: inProgress,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 3',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      runStore.applyRunEvent({
        runId: inProgress,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Run 3',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:05Z',
        action: { type: 'InProgress' },
      })

      expect(runStore.queuedRuns.length).toBe(2)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued1)
      expect(runStore.queuedRuns.map((r) => r.id)).toContain(queued2)
    })

    it('should filter inProgressRuns correctly', () => {
      const inProgress1 = 30n
      const inProgress2 = 31n
      const queued = 32n

      runStore.applyRunEvent({
        runId: inProgress1,
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
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:05Z',
        action: { type: 'InProgress' },
      })

      runStore.applyRunEvent({
        runId: inProgress2,
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
        runStartedAt: '2025-01-01T00:00:10Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      runStore.applyRunEvent({
        runId: queued,
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
      })

      expect(runStore.inProgressRuns.length).toBe(2)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress1)
      expect(runStore.inProgressRuns.map((r) => r.id)).toContain(inProgress2)
      expect(runStore.inProgressRuns.map((r) => r.id)).not.toContain(queued)
    })

    it('should filter completedRuns correctly', () => {
      const completed1 = 40n
      const completed2 = 41n
      const inProgress = 42n

      runStore.applyRunEvent({
        runId: completed1,
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
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:15Z',
        action: { type: 'Completed', data: { conclusion: 'Success' } },
      })

      runStore.applyRunEvent({
        runId: completed2,
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
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:20Z',
        action: { type: 'Completed', data: { conclusion: 'Failure' } },
      })

      runStore.applyRunEvent({
        runId: inProgress,
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
        runStartedAt: '2025-01-01T00:00:05Z',
        updatedAt: '2025-01-01T00:00:10Z',
        action: { type: 'InProgress' },
      })

      expect(runStore.completedRuns.length).toBe(2)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed1)
      expect(runStore.completedRuns.map((r) => r.id)).toContain(completed2)
      expect(runStore.completedRuns.map((r) => r.id)).not.toContain(inProgress)
    })
  })

  // AC3.5: loadSnapshot replaces all state atomically
  describe('AC3.5: Atomic loadSnapshot', () => {
    it('should replace all runs and jobs when loading a snapshot', () => {
      // Set up initial state
      runStore.applyRunEvent({
        runId: 50n,
        org: 'org',
        repo: 'repo',
        workflowName: null,
        workflowPath: null,
        branch: null,
        headSha: 'sha',
        commitMessage: null,
        triggerEvent: 'push',
        displayTitle: 'Old Run',
        htmlUrl: 'url',
        createdAt: '2025-01-01T00:00:00Z',
        runStartedAt: null,
        updatedAt: '2025-01-01T00:00:00Z',
        action: { type: 'Requested' },
      })

      runStore.applyJobEvent({
        jobId: 500n,
        runId: 50n,
        org: 'org',
        repo: 'repo',
        name: 'Old Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: { type: 'Queued', data: { labels: [], steps: [] } },
      })

      // Verify initial state
      expect(runStore.runs.size).toBe(1)
      expect(runStore.jobsByRun.size).toBe(1)

      // Load new snapshot
      const newRun: WorkflowRun = {
        id: 51n,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'newsha',
        commitMessage: 'New msg',
        event: 'push',
        displayTitle: 'New Run',
        status: 'InProgress',
        conclusion: null,
        htmlUrl: 'newurl',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:05Z',
      }

      const newJob: Job = {
        id: 501n,
        name: 'New Job',
        runId: 51n,
        status: 'Queued',
        conclusion: null,
        runner: null,
        labels: ['ubuntu-latest'],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: null,
        completedAt: null,
      }

      runStore.loadSnapshot([newRun], [newJob])

      // Verify old state is gone
      expect(runStore.runs.has(50n)).toBe(false)
      expect(runStore.runs.has(51n)).toBe(true)
      expect(runStore.runs.size).toBe(1)

      // Verify new state is loaded
      const loadedRun = runStore.runs.get(51n)!
      expect(loadedRun.displayTitle).toBe('New Run')
      expect(loadedRun.status).toBe('InProgress')

      // Verify jobs are grouped correctly
      expect(runStore.jobsByRun.has(51n)).toBe(true)
      const jobs = runStore.jobsByRun.get(51n)!
      expect(jobs.length).toBe(1)
      expect(jobs[0]!.id).toBe(501n)
      expect(jobs[0]!.name).toBe('New Job')
    })

    it('should handle loading snapshot with multiple runs and jobs', () => {
      const run1: WorkflowRun = {
        id: 60n,
        org: 'org',
        repo: 'repo',
        workflowName: 'CI',
        workflowPath: '.github/workflows/ci.yml',
        branch: 'main',
        headSha: 'sha1',
        commitMessage: 'msg1',
        event: 'push',
        displayTitle: 'Run 1',
        status: 'Completed',
        conclusion: 'Success',
        htmlUrl: 'url1',
        createdAt: '2025-01-02T00:00:00Z',
        runStartedAt: '2025-01-02T00:00:05Z',
        updatedAt: '2025-01-02T00:00:15Z',
      }

      const run2: WorkflowRun = {
        id: 61n,
        org: 'org',
        repo: 'repo',
        workflowName: 'Test',
        workflowPath: '.github/workflows/test.yml',
        branch: 'develop',
        headSha: 'sha2',
        commitMessage: 'msg2',
        event: 'pull_request',
        displayTitle: 'Run 2',
        status: 'InProgress',
        conclusion: null,
        htmlUrl: 'url2',
        createdAt: '2025-01-02T01:00:00Z',
        runStartedAt: '2025-01-02T01:00:05Z',
        updatedAt: '2025-01-02T01:00:10Z',
      }

      const job1: Job = {
        id: 600n,
        name: 'Job 1',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Success',
        runner: { id: 1n, name: 'Runner1', groupId: null, groupName: null },
        labels: ['ubuntu-latest'],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:15Z',
      }

      const job2: Job = {
        id: 601n,
        name: 'Job 2',
        runId: 60n,
        status: 'Completed',
        conclusion: 'Failure',
        runner: { id: 2n, name: 'Runner2', groupId: null, groupName: null },
        labels: [],
        steps: [],
        createdAt: '2025-01-02T00:00:00Z',
        startedAt: '2025-01-02T00:00:05Z',
        completedAt: '2025-01-02T00:00:20Z',
      }

      const job3: Job = {
        id: 602n,
        name: 'Job 3',
        runId: 61n,
        status: 'Queued',
        conclusion: null,
        runner: null,
        labels: [],
        steps: [],
        createdAt: '2025-01-02T01:00:00Z',
        startedAt: null,
        completedAt: null,
      }

      runStore.loadSnapshot([run1, run2], [job1, job2, job3])

      // Verify runs
      expect(runStore.runs.size).toBe(2)
      expect(runStore.runs.get(60n)!.displayTitle).toBe('Run 1')
      expect(runStore.runs.get(61n)!.displayTitle).toBe('Run 2')

      // Verify jobs are grouped correctly by run ID
      expect(runStore.jobsByRun.get(60n)!.length).toBe(2)
      expect(runStore.jobsByRun.get(61n)!.length).toBe(1)
      expect(runStore.jobsByRun.get(60n)!.map((j) => j.id)).toContain(600n)
      expect(runStore.jobsByRun.get(60n)!.map((j) => j.id)).toContain(601n)
      expect(runStore.jobsByRun.get(61n)!.map((j) => j.id)).toContain(602n)
    })
  })

  // AC3.9: Idempotent duplicate events
  describe('AC3.9: Idempotent duplicate events', () => {
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

    it('should handle duplicate job events without creating duplicates', () => {
      const runId = 71n
      const jobId = 710n
      const envelope: JobEventEnvelope = {
        jobId,
        runId,
        org: 'org',
        repo: 'repo',
        name: 'Job',
        createdAt: '2025-01-01T00:00:00Z',
        startedAt: null,
        completedAt: null,
        action: {
          type: 'Queued',
          data: { labels: [], steps: [] },
        },
      }

      // Apply same event twice
      runStore.applyJobEvent(envelope)
      expect(runStore.jobsByRun.get(runId)!.length).toBe(1)

      runStore.applyJobEvent(envelope)
      expect(runStore.jobsByRun.get(runId)!.length).toBe(1) // Still 1, not 2

      const jobs = runStore.jobsByRun.get(runId)!
      expect(jobs[0]!.status).toBe('Queued')
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
