import type { CommittedEvent } from '$lib/types/generated/CommittedEvent'
import type { Job } from '$lib/types/generated/Job'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { JobStatus } from '$lib/types/generated/JobStatus'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'
import type { RunStatus } from '$lib/types/generated/RunStatus'
import type { Step } from '$lib/types/generated/Step'
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
    status: 'Queued' satisfies RunStatus,
    conclusion: null,
    htmlUrl: 'https://github.com/test-org/test-repo/actions/runs/1',
    createdAt: '2026-04-16T10:00:00Z',
    runStartedAt: null,
    updatedAt: '2026-04-16T10:00:00Z',
    runAttempt: 1,
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
    status: 'Queued' satisfies JobStatus,
    conclusion: null,
    runner: null,
    labels: [],
    steps: [],
    createdAt: '2026-04-16T10:00:00Z',
    startedAt: null,
    completedAt: null,
    runAttempt: 1,
    ...overrides,
  }
}

/**
 * Create a mock Step with sensible defaults for testing.
 * All fields can be overridden via the partial parameter.
 */
export function createMockStep(overrides: Partial<Step> = {}): Step {
  return {
    number: 1n,
    name: 'test step',
    status: 'Queued',
    conclusion: null,
    startedAt: null,
    completedAt: null,
    ...overrides,
  }
}

/**
 * Create a mock RunnerInfo with sensible defaults for testing.
 * All fields can be overridden via the partial parameter.
 */
export function createMockRunner(overrides: Partial<RunnerInfo> = {}): RunnerInfo {
  return {
    id: 1n,
    name: 'runner-1',
    groupName: null,
    ...overrides,
  }
}

/**
 * Create a mock RunEventEnvelope with sensible defaults for testing.
 * All fields can be overridden via the partial parameter. The default action
 * is `Requested`; pass `action` to model a different transition.
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
    runAttempt: 1,
    action: { type: 'Requested' },
    ...overrides,
  }
}

/**
 * Create a mock JobEventEnvelope with sensible defaults for testing.
 * All fields can be overridden via the partial parameter. The default action
 * is `Queued` with empty labels/steps; pass `action` to model a different
 * transition.
 */
export function createMockJobEvent(overrides: Partial<JobEventEnvelope> = {}): JobEventEnvelope {
  return {
    jobId: 1n,
    runId: 1n,
    org: 'test-org',
    repo: 'test-repo',
    name: 'test-job',
    createdAt: '2026-04-16T10:00:00Z',
    startedAt: null,
    completedAt: null,
    runAttempt: 1,
    action: { type: 'Queued', data: { labels: [], steps: [] } },
    ...overrides,
  }
}

/**
 * Wrap a `RunEventEnvelope` in the `CommittedEvent` shell (the
 * `{ seq, event: { type: 'Run', data } }` wire frame). Envelope fields are
 * built via `createMockRunEvent`, so overrides follow the same defaults.
 */
export function createMockRunCommittedEvent(
  seq: bigint,
  overrides: Partial<RunEventEnvelope> = {},
): CommittedEvent {
  return { seq, event: { type: 'Run', data: createMockRunEvent(overrides) } }
}

/**
 * Wrap a `JobEventEnvelope` in the `CommittedEvent` shell (the
 * `{ seq, event: { type: 'Job', data } }` wire frame). Envelope fields are
 * built via `createMockJobEvent`, so overrides follow the same defaults.
 */
export function createMockJobCommittedEvent(
  seq: bigint,
  overrides: Partial<JobEventEnvelope> = {},
): CommittedEvent {
  return { seq, event: { type: 'Job', data: createMockJobEvent(overrides) } }
}
