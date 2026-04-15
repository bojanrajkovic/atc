import type { Job } from '$lib/types/generated/Job'
import type { JobConclusion } from '$lib/types/generated/JobConclusion'
import type { JobEventEnvelope } from '$lib/types/generated/JobEventEnvelope'
import type { RunConclusion } from '$lib/types/generated/RunConclusion'
import type { RunEventEnvelope } from '$lib/types/generated/RunEventEnvelope'
import type { RunnerInfo } from '$lib/types/generated/RunnerInfo'
import type { Step } from '$lib/types/generated/Step'
import type { WorkflowRun } from '$lib/types/generated/WorkflowRun'

class RunStore {
  runs = $state<Map<bigint, WorkflowRun>>(new Map())
  jobsByRun = $state<Map<bigint, Job[]>>(new Map())

  queuedRuns = $derived([...this.runs.values()].filter((r) => r.status === 'Queued'))
  inProgressRuns = $derived([...this.runs.values()].filter((r) => r.status === 'InProgress'))
  completedRuns = $derived([...this.runs.values()].filter((r) => r.status === 'Completed'))

  applyRunEvent(envelope: RunEventEnvelope): void {
    const runId = envelope.runId

    // Determine status from the action type
    let status: 'Queued' | 'InProgress' | 'Completed'
    let conclusion: RunConclusion | null = null

    if (envelope.action.type === 'Requested') {
      status = 'Queued'
    } else if (envelope.action.type === 'InProgress') {
      status = 'InProgress'
    } else if (envelope.action.type === 'Completed') {
      status = 'Completed'
      conclusion = envelope.action.data.conclusion
    } else {
      // Exhaustiveness check - this should never be reached
      // @ts-expect-error - exhaustiveness check
      const _: never = envelope.action
      return
    }

    // Get existing run or create new one
    const existing = this.runs.get(runId)
    const run: WorkflowRun = existing ?? {
      id: runId,
      org: envelope.org,
      repo: envelope.repo,
      workflowName: envelope.workflowName,
      workflowPath: envelope.workflowPath,
      branch: envelope.branch,
      headSha: envelope.headSha,
      commitMessage: envelope.commitMessage,
      event: envelope.triggerEvent,
      displayTitle: envelope.displayTitle,
      status,
      conclusion,
      htmlUrl: envelope.htmlUrl,
      createdAt: envelope.createdAt,
      runStartedAt: envelope.runStartedAt,
      updatedAt: envelope.updatedAt,
    }

    // Update existing run with new data — matches backend store.rs semantics
    if (existing) {
      run.status = status
      run.conclusion = conclusion ?? existing.conclusion
      // Preserve optional fields that may be absent in some events
      run.workflowName = envelope.workflowName ?? existing.workflowName
      run.workflowPath = envelope.workflowPath ?? existing.workflowPath
      run.runStartedAt = envelope.runStartedAt ?? existing.runStartedAt
      // Overwrite fields that the backend always replaces
      run.branch = envelope.branch
      run.headSha = envelope.headSha
      run.commitMessage = envelope.commitMessage
      run.displayTitle = envelope.displayTitle
      run.htmlUrl = envelope.htmlUrl
      run.updatedAt = envelope.updatedAt
    }

    this.runs.set(runId, run)
  }

  applyJobEvent(envelope: JobEventEnvelope): void {
    const jobId = envelope.jobId
    const runId = envelope.runId

    // Determine status from the action type
    let status: 'Queued' | 'Waiting' | 'InProgress' | 'Completed'
    let conclusion: JobConclusion | null = null
    let runner: RunnerInfo | null = null
    let labels: Array<string> = []
    let steps: Array<Step> = []

    if (envelope.action.type === 'Queued') {
      status = 'Queued'
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'Waiting') {
      status = 'Waiting'
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'InProgress') {
      status = 'InProgress'
      runner = envelope.action.data.runner
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else if (envelope.action.type === 'Completed') {
      status = 'Completed'
      conclusion = envelope.action.data.conclusion
      runner = envelope.action.data.runner
      labels = envelope.action.data.labels
      steps = envelope.action.data.steps
    } else {
      // Exhaustiveness check - this should never be reached
      // @ts-expect-error - exhaustiveness check
      const _: never = envelope.action
      return
    }

    // Get existing job or create new one
    const existing = this.jobsByRun.get(runId) ?? []
    const jobIndex = existing.findIndex((j) => j.id === jobId)

    // Update existing job with new data
    if (jobIndex === -1) {
      const newJob: Job = {
        id: jobId,
        name: envelope.name,
        runId,
        status,
        conclusion,
        runner,
        labels,
        steps,
        createdAt: envelope.createdAt,
        startedAt: envelope.startedAt,
        completedAt: envelope.completedAt,
      }
      existing.push(newJob)
    } else {
      const existingJob = existing[jobIndex]
      if (existingJob) {
        existingJob.status = status
        existingJob.conclusion = conclusion ?? existingJob.conclusion
        existingJob.runner = runner ?? existingJob.runner
        existingJob.labels = labels
        existingJob.steps = steps
        existingJob.startedAt = envelope.startedAt ?? existingJob.startedAt
        existingJob.completedAt = envelope.completedAt ?? existingJob.completedAt
      }
    }

    this.jobsByRun.set(runId, existing)
  }

  loadSnapshot(runs: WorkflowRun[], jobs: Job[]): void {
    // Atomic replace: clear all existing state, load snapshot data
    this.runs = new Map(runs.map((r) => [r.id, r]))

    // Group jobs by run ID
    const grouped = new Map<bigint, Job[]>()
    for (const job of jobs) {
      const arr = grouped.get(job.runId) ?? []
      arr.push(job)
      grouped.set(job.runId, arr)
    }
    this.jobsByRun = grouped
  }

  clear(): void {
    this.runs = new Map()
    this.jobsByRun = new Map()
  }
}

export const runStore = new RunStore()
