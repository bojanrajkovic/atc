import ATCKit
import Foundation
import Observation

/// Keeps `rows` in step with an ATC server.
///
/// ATC's WebSocket sends one frame per committed run or job event. The feed
/// treats each frame as a signal to re-read `GET /v1/state`, so the server
/// stays the only place that applies events to state.
// shortcut: one snapshot fetch per burst of events; apply events locally if snapshots get large.
@MainActor @Observable
final class Feed {
    enum Phase: Equatable {
        case needsServer
        /// Connecting, or waiting to retry. `attempt` is 0 on the first try;
        /// `reason` is why the previous attempt failed.
        case connecting(attempt: Int, reason: String?)
        case live
        case signedOut
    }

    private enum FeedError: Error {
        /// 401 from ATC: no session, or its repository list is older than the server's TTL.
        case unauthorized
        case http(Int)
    }

    static let serverKey = "server"

    private(set) var phase = Phase.needsServer
    private(set) var rows: [JobRow] = []
    /// When the server last answered: a frame, a pong, or a snapshot.
    private(set) var lastContact = Date.now
    var activeCount: Int { rows.count(where: \.isActive) }

    /// Holds the ATC session cookie in memory. Each launch signs in again
    /// through the web view, which keeps the durable GitHub session.
    private let cookies: HTTPCookieStorage
    private let session: URLSession
    private let login = Login()
    private var loop: Task<Void, Never>?
    /// The server `rows` came from, so a retry keeps them and a new server doesn't.
    private var rowsServer: URL?

    init() {
        let configuration = URLSessionConfiguration.ephemeral
        cookies = configuration.httpCookieStorage!
        session = URLSession(configuration: configuration)
        start()
    }

    /// Connects to the server saved in user defaults, replacing any current connection.
    func start() {
        loop?.cancel()
        if server != rowsServer {
            rows = []
            rowsServer = server
        }
        guard let server else {
            phase = .needsServer
            return
        }
        loop = Task { await run(server) }
    }

    /// Shows the GitHub sign-in window, then reconnects.
    func signIn() {
        guard let server else { return }
        loop?.cancel()
        loop = Task {
            guard await authenticate(server, interactive: true) else { return }
            await run(server)
        }
    }

    var server: URL? {
        let text = UserDefaults.standard.string(forKey: Self.serverKey) ?? ""
        guard let url = URL(string: text), url.scheme == "https" || url.scheme == "http", url.host() != nil else {
            return nil
        }
        return url
    }

    private func run(_ server: URL) async {
        var attempt = 0
        var reason: String?
        var freshSignIn = false
        while !Task.isCancelled {
            do {
                phase = .connecting(attempt: attempt, reason: reason)
                try await stream(server) {
                    attempt = 0
                    freshSignIn = false
                }
            } catch FeedError.unauthorized {
                // The session expired or went stale. Sign in again without a
                // window; GitHub redirects straight back if the app is authorized.
                // A 401 straight after signing in means the server won't take
                // the cookie; stop instead of signing in forever.
                guard !freshSignIn, await authenticate(server, interactive: false) else {
                    if !Task.isCancelled { phase = .signedOut }
                    return
                }
                freshSignIn = true
                continue
            } catch FeedError.http(let status) {
                reason = "ATC returned HTTP \(status)"
            } catch {
                reason = error.localizedDescription
            }
            // A replaced loop must not write state; cancellation surfaces as
            // several error types, so check the task instead of the error.
            guard !Task.isCancelled else { return }
            attempt += 1
            phase = .connecting(attempt: attempt, reason: reason)
            // 1s, 2s, 4s … capped at 30s.
            try? await Task.sleep(for: .seconds(min(1 << min(attempt - 1, 5), 30)))
        }
    }

    /// Runs the sign-in flow and stores the session cookie. Sets `.signedOut` on failure.
    private func authenticate(_ server: URL, interactive: Bool) async -> Bool {
        guard let cookie = await login.signIn(server: server, interactive: interactive) else {
            if !Task.isCancelled { phase = .signedOut }
            return false
        }
        cookies.setCookie(cookie)
        return true
    }

    /// Streams until the connection fails. Calls `connected` once the feed is live.
    private func stream(_ server: URL, connected: () -> Void) async throws {
        // Fetch first: a 401 here is readable, a rejected WebSocket upgrade is not.
        try await refresh(server)

        var components = URLComponents(url: server.appending(path: "v1/ws"), resolvingAgainstBaseURL: false)!
        components.scheme = server.scheme == "https" ? "wss" : "ws"
        var request = URLRequest(url: components.url!)
        // ATC rejects an upgrade whose Origin isn't its own public origin.
        request.setValue(origin(of: server), forHTTPHeaderField: "Origin")
        let socket = session.webSocketTask(with: request)
        socket.resume()
        // `receive()` ignores task cancellation, so every exit closes the socket to end it.
        let close: @Sendable () -> Void = { socket.cancel(with: .goingAway, reason: nil) }
        defer { close() }

        try await withTaskCancellationHandler {
            // The server's hello frame proves the subscription is open, so the
            // snapshot fetched next can't miss an event.
            _ = try await socket.receive()
            try await refresh(server)
            phase = .live
            connected()
            try await pump(socket, server: server, close: close)
        } onCancel: {
            close()
        }
    }

    /// Reads frames, refreshes on changes, and pings, until one of them fails.
    private func pump(_ socket: URLSessionWebSocketTask, server: URL, close: @escaping @Sendable () -> Void)
        async throws
    {

        // A burst of frames collapses into one pending refresh.
        let (changes, changed) = AsyncStream<Void>.makeStream(bufferingPolicy: .bufferingNewest(1))
        try await withThrowingTaskGroup(of: Void.self) { group in
            // The group waits for every child before it returns, so close
            // the socket first or the receive loop holds it open.
            defer {
                group.cancelAll()
                close()
            }
            group.addTask {
                while true {
                    let message = try await socket.receive()
                    await self.contacted()
                    if case .string(let frame) = message, Board.isCommitted(frame: frame) {
                        changed.yield()
                    }
                }
            }
            // ATC sends nothing while CI is idle, so ping to tell a quiet
            // server from a dead connection. A minute without an answer reconnects.
            group.addTask {
                while true {
                    socket.sendPing { error in
                        if error == nil { Task { await self.contacted() } }
                    }
                    try await Task.sleep(for: .seconds(15))
                    if await Date.now.timeIntervalSince(self.lastContact) > 60 {
                        throw URLError(.timedOut)
                    }
                }
            }
            group.addTask {
                for await _ in changes {
                    try await Task.sleep(for: .milliseconds(300))
                    try await self.refresh(server)
                }
            }
            try await group.next()
        }
    }

    private func refresh(_ server: URL) async throws {
        let (data, response) = try await session.data(from: server.appending(path: "v1/state"))
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if status == 401 { throw FeedError.unauthorized }
        guard status == 200 else { throw FeedError.http(status) }
        try Task.checkCancellation()
        rows = Board.rows(try Board.decode(data))
        contacted()
    }

    private func contacted() { lastContact = .now }

    private func origin(of server: URL) -> String {
        var origin = "\(server.scheme!)://\(server.host()!)"
        if let port = server.port { origin += ":\(port)" }
        return origin
    }
}
