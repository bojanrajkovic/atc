import SwiftUI

/// The connection dot and its one-line detail.
///
/// | Color  | Meaning                                             |
/// |--------|-----------------------------------------------------|
/// | green  | live: the server answered in the last 30 seconds    |
/// | yellow | stale: connected, but the server has gone quiet     |
/// | orange | disconnected and retrying                           |
/// | red    | dead: signed out, nothing retries until you sign in |
/// | gray   | no server configured                                |
///
/// Clicking it reconnects now, or signs in when signed out.
struct ConnectionIndicator: View {
    let feed: Feed

    /// Matches the web UI's stale threshold.
    private static let staleAfter: TimeInterval = 30

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { timeline in
            let quiet = timeline.date.timeIntervalSince(feed.lastContact)
            let state = state(quiet: quiet)

            Button {
                if feed.phase == .signedOut { feed.signIn() } else { feed.start() }
            } label: {
                HStack(spacing: 6) {
                    Text(state.detail)
                        .font(.system(size: 11))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    StatusDot(color: state.color, glows: state.isLive, pings: state.pings)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(state.isLive || feed.phase == .needsServer)
            .help(state.help)
            .accessibilityLabel("Connection: \(state.detail)")
        }
    }

    private typealias State = (color: Color, detail: String, help: String, isLive: Bool, pings: Bool)

    private func state(quiet: TimeInterval) -> State {
        switch feed.phase {
        case .live where quiet > Self.staleAfter:
            (Palette.running, "No response for \(Int(quiet))s", "The server has gone quiet — click to reconnect", false, false)
        case .live:
            (Palette.success, "Live", "Connected", true, false)
        case .connecting(let attempt, let reason):
            (
                Palette.timedOut,
                attempt == 0 ? "Connecting…" : "Reconnecting (attempt \(attempt))…",
                [reason, "Click to retry now"].compactMap { $0 }.joined(separator: " — "),
                false, true
            )
        case .signedOut:
            (Palette.failed, "Signed out", "Click to sign in with GitHub", false, false)
        case .needsServer:
            (.secondary, "No server", "Enter your ATC server's URL below", false, false)
        }
    }
}

/// A status dot. `glows` adds the web UI's soft halo for the live state;
/// `pings` adds its expanding ring while connecting.
private struct StatusDot: View {
    let color: Color
    let glows: Bool
    let pings: Bool

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var expanded = false

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: 8, height: 8)
            .shadow(color: glows ? color.opacity(0.9) : .clear, radius: 3)
            .background {
                if pings && !reduceMotion {
                    Circle()
                        .fill(color)
                        .scaleEffect(expanded ? 2.4 : 1)
                        .opacity(expanded ? 0 : 0.75)
                        .animation(.easeOut(duration: 1).repeatForever(autoreverses: false), value: expanded)
                        .onAppear { expanded = true }
                        .onDisappear { expanded = false }
                }
            }
            .frame(width: 14, height: 14)
            .animation(.easeInOut(duration: 0.25), value: color)
    }
}
