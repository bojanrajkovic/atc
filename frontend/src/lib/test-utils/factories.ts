import type { Job } from '$lib/types/generated/Job'
import type { JobStatus } from '$lib/types/generated/JobStatus'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { RunStatus } from '$lib/types/generated/RunStatus'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

/**
 * Create a mock WorkflowRun with sensible defaults for testing.
 * All fields can be overridden via the partial parameter.
 */
export function createMockRun(overrides: Partial<WorkflowRun> = {}): WorkflowRun {
  return {
    id: 1n,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'test commit',
    event: 'push',
    displayTitle: 'CI — main',
    status: 'Queued' as RunStatus,
    conclusion: null,
    htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
    createdAt: '2026-04-16T10:00:00Z',
    runStartedAt: null,
    updatedAt: '2026-04-16T10:00:00Z',
    ...overrides,
  }
}

/**
 * Create a mock Job with sensible defaults for testing.
 * All fields can be overridden via the partial parameter.
 */
export function createMockJob(overrides: Partial<Job> = {}): Job {
  return {
    id: 1n,
    name: 'test-job',
    runId: 1n,
    status: 'Queued' as JobStatus,
    conclusion: null,
    runner: null,
    labels: [],
    steps: [],
    createdAt: '2026-04-16T10:00:00Z',
    startedAt: null,
    completedAt: null,
    ...overrides,
  }
}

/**
 * Create a mock RunEventEnvelope with sensible defaults for testing.
 * All fields can be overridden via the partial parameter.
 */
export function createMockRunEvent(overrides: Partial<RunEventEnvelope> = {}): RunEventEnvelope {
  return {
    runId: 1n,
    org: 'test-org',
    repo: 'test-repo',
    workflowName: 'CI',
    workflowPath: '.github/workflows/ci.yml',
    branch: 'main',
    headSha: 'abc123',
    commitMessage: 'test commit',
    triggerEvent: 'push',
    displayTitle: 'CI — main',
    htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
    createdAt: '2026-04-16T10:00:00Z',
    runStartedAt: null,
    updatedAt: '2026-04-16T10:00:00Z',
    action: { type: 'Requested' },
    ...overrides,
  }
}
