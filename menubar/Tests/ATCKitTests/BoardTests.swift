import Foundation
import Testing

@testable import ATCKit

private func job(_ id: Int, run: Int = 1, _ status: String, at time: String) -> String {
    let completed = status == "Completed"
    return """
        {"id": \(id), "name": "job-\(id)", "runId": \(run), "status": "\(status)",
         "conclusion": \(completed ? "\"Success\"" : "null"), "runner": null, "labels": [], "steps": [],
         "createdAt": "2026-11-13T10:00:00Z",
         "startedAt": \(status == "Queued" ? "null" : "\"\(time)\""),
         "completedAt": \(completed ? "\"\(time)\"" : "null"), "runAttempt": 1}
        """
}

private func snapshot(jobs: [String]) throws -> StateSnapshot {
    let json = """
        {"lastSeq": 7, "displayTtlSeconds": 3600, "runnerPoolCapacities": [],
         "runs": [{"id": 1, "org": "octo", "repo": "cat", "workflowName": null, "workflowPath": null,
                   "branch": "main", "headSha": "abc", "commitMessage": null, "event": "push",
                   "displayTitle": "t", "status": "InProgress", "conclusion": null,
                   "htmlUrl": "https://github.com/octo/cat/actions/runs/1",
                   "createdAt": "2026-11-13T10:00:00Z", "runStartedAt": null,
                   "updatedAt": "2026-11-13T10:00:00Z", "runAttempt": 1}],
         "jobs": [\(jobs.joined(separator: ","))]}
        """
    return try Board.decode(Data(json.utf8))
}

@Test func decodesTimestampsWithAndWithoutFractionalSeconds() throws {
    let state = try snapshot(jobs: [
        job(1, "InProgress", at: "2026-11-13T10:00:05.123456789Z"),
        job(2, "InProgress", at: "2026-11-13T10:00:05Z"),
    ])
    let whole = try #require(state.jobs[1].startedAt)
    let fractional = try #require(state.jobs[0].startedAt)
    #expect(abs(fractional.timeIntervalSince(whole) - 0.123) < 0.001)
}

@Test func ordersByLaneThenNewestFirst() throws {
    let state = try snapshot(jobs: [
        job(1, "Completed", at: "2026-11-13T10:01:00Z"),
        job(2, "Queued", at: ""),
        job(3, "Completed", at: "2026-11-13T10:03:00Z"),
        job(4, "InProgress", at: "2026-11-13T10:02:00Z"),
        job(5, "InProgress", at: "2026-11-13T10:05:00Z"),
        job(6, "Waiting", at: "2026-11-13T10:06:00Z"),
    ])
    let rows = Board.rows(state)
    #expect(rows.map(\.id) == [5, 4, 2, 6, 3, 1])
    #expect(rows.map(\.lane) == [.running, .running, .queued, .queued, .completed, .completed])
    #expect(rows.map(\.status) == [.inProgress, .inProgress, .queued, .queued, .success, .success])
}

@Test func convertsOKLCHToSRGB() {
    let white = srgb(lightness: 1, chroma: 0, hue: 0)
    #expect(abs(white.red - 1) < 0.001 && abs(white.green - 1) < 0.001 && abs(white.blue - 1) < 0.001)

    // sRGB red is oklch(62.8% 0.2577 29.23).
    let red = srgb(lightness: 0.628, chroma: 0.2577, hue: 29.23)
    #expect(abs(red.red - 1) < 0.01 && red.green < 0.02 && red.blue < 0.02)
}

@Test func limitCutsCompletedJobsButNeverUnfinishedOnes() throws {
    let state = try snapshot(jobs: [
        job(1, "Completed", at: "2026-11-13T10:01:00Z"),
        job(2, "InProgress", at: "2026-11-13T10:02:00Z"),
        job(3, "InProgress", at: "2026-11-13T10:03:00Z"),
        job(4, "Completed", at: "2026-11-13T10:04:00Z"),
    ])
    #expect(Board.rows(state, limit: 3).map(\.id) == [3, 2, 4])
    #expect(Board.rows(state, limit: 1).map(\.id) == [3, 2])
}

@Test func dropsJobsWhoseRunIsMissing() throws {
    let state = try snapshot(jobs: [job(1, run: 99, "InProgress", at: "2026-11-13T10:02:00Z")])
    #expect(Board.rows(state).isEmpty)
}

@Test func rowLinksToTheJobPage() throws {
    let state = try snapshot(jobs: [job(5, "InProgress", at: "2026-11-13T10:02:00Z")])
    #expect(Board.rows(state)[0].url?.absoluteString == "https://github.com/octo/cat/actions/runs/1/job/5")
}

@Test func onlyCommittedFramesCount() {
    #expect(Board.isCommitted(frame: #"{"kind":"Committed","seq":3,"event":{}}"#))
    #expect(!Board.isCommitted(frame: #"{"kind":"ServerHello","version":"v1"}"#))
    #expect(!Board.isCommitted(frame: "not json"))
}
