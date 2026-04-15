import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { Job } from '$lib/types/generated/Job'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'
import { runStore } from './runs.svelte'

describe('RunStore', () => {
  beforeEach(() => {
    runStore.clear()
  })

  afterEach(() => {
    runStore.clear()
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
})
