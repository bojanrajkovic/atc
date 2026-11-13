import Foundation

/// A job joined with its workflow run: one row in the menu.
public struct JobRow: Identifiable, Sendable {
    public let job: Job
    public let run: WorkflowRun

    public var id: Int64 { job.id }
    public var isActive: Bool { job.status != .completed }

    public var lane: Lane {
        switch job.status {
        case .inProgress: .running
        case .queued, .waiting: .queued
        case .completed: .completed
        }
    }

    public var status: StatusKey {
        switch (job.status, job.conclusion) {
        case (.queued, _), (.waiting, _): .queued
        case (.inProgress, _): .inProgress
        case (.completed, .success): .success
        case (.completed, .failure): .failure
        case (.completed, .cancelled): .cancelled
        case (.completed, .timedOut): .timedOut
        case (.completed, .actionRequired): .actionRequired
        // A completed job always carries a conclusion; treat a missing one as neutral.
        case (.completed, .stale), (.completed, .neutral), (.completed, .skipped), (.completed, nil): .neutral
        }
    }

    /// The job's page on GitHub.
    public var url: URL? { URL(string: "\(run.htmlUrl)/job/\(job.id)") }
}

/// The menu's sections, in display order.
public enum Lane: Sendable, CaseIterable {
    case running, queued, completed
}

/// A job's status and conclusion folded into the one value the UI draws.
/// The same folding as the web UI's `status-key.ts`.
public enum StatusKey: Sendable {
    case queued, inProgress, success, failure, cancelled, timedOut, actionRequired, neutral
}

public enum Board {
    /// Decodes a `GET /v1/state` body.
    ///
    /// ATC writes RFC 3339 timestamps with or without fractional seconds,
    /// so both forms parse.
    public static func decode(_ data: Data) throws -> StateSnapshot {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .custom { decoder in
            let container = try decoder.singleValueContainer()
            let text = try container.decode(String.self)
            if let date = try? Date.ISO8601FormatStyle(includingFractionalSeconds: true).parse(text) {
                return date
            }
            if let date = try? Date.ISO8601FormatStyle().parse(text) {
                return date
            }
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "not an RFC 3339 timestamp: \(text)")
        }
        return try decoder.decode(StateSnapshot.self, from: data)
    }

    /// Orders jobs for the menu by lane: running, then queued, then completed,
    /// newest first within each, up to `limit` rows in total.
    /// Unfinished jobs are never cut. A job whose run is missing from the
    /// snapshot is dropped, because the row needs the run's repository and URL.
    public static func rows(_ snapshot: StateSnapshot, limit: Int = 25) -> [JobRow] {
        let runs = Dictionary(snapshot.runs.map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })
        let rows = snapshot.jobs.compactMap { job in runs[job.runId].map { JobRow(job: job, run: $0) } }

        func lane(_ lane: Lane, by date: (Job) -> Date) -> [JobRow] {
            rows.filter { $0.lane == lane }.sorted { date($0.job) > date($1.job) }
        }
        let active =
            lane(.running) { $0.startedAt ?? $0.createdAt } + lane(.queued) { $0.createdAt }
        let completed = lane(.completed) { $0.completedAt ?? $0.createdAt }

        return active + completed.prefix(max(0, limit - active.count))
    }

    /// Whether a WebSocket text frame reports a committed run or job event.
    /// The other frame kinds (server hello, config updates) don't change jobs.
    public static func isCommitted(frame: String) -> Bool {
        struct Frame: Decodable { let kind: String }
        return (try? JSONDecoder().decode(Frame.self, from: Data(frame.utf8)))?.kind == "Committed"
    }
}
