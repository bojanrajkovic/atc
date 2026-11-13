import Foundation

// The subset of ATC's `GET /v1/state` response the menu shows. Field names
// match the camelCase JSON that `atc-wire` emits.

public struct StateSnapshot: Decodable, Sendable {
    public let runs: [WorkflowRun]
    public let jobs: [Job]
}

public struct WorkflowRun: Decodable, Sendable {
    public let id: Int64
    public let org: String
    public let repo: String
    public let workflowName: String?
    public let branch: String?
    public let htmlUrl: String
}

public enum JobStatus: String, Decodable, Sendable {
    case queued = "Queued"
    case waiting = "Waiting"
    case inProgress = "InProgress"
    case completed = "Completed"
}

public enum JobConclusion: String, Decodable, Sendable {
    case success = "Success"
    case failure = "Failure"
    case cancelled = "Cancelled"
    case timedOut = "TimedOut"
    case actionRequired = "ActionRequired"
    case stale = "Stale"
    case neutral = "Neutral"
    case skipped = "Skipped"
}

public struct Job: Decodable, Sendable {
    public let id: Int64
    public let name: String
    public let runId: Int64
    public let status: JobStatus
    public let conclusion: JobConclusion?
    public let createdAt: Date
    public let startedAt: Date?
    public let completedAt: Date?
}
