import ATCKit
import SwiftUI

struct JobsView: View {
    let feed: Feed
    /// The server URL being typed. Saved on Return, so a half-typed URL is never stored.
    @State private var draft = UserDefaults.standard.string(forKey: Feed.serverKey) ?? ""
    @State private var editingServer = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            if feed.rows.isEmpty { emptyState } else { jobList }
            if editingServer || feed.phase == .needsServer { serverField }
            Divider()
            footer
        }
        .frame(width: 360)
    }

    // Logo and counts on the left, connection on the right, as in the web UI's top bar.
    private var header: some View {
        HStack(spacing: 8) {
            Text("ATC").font(.system(size: 13, weight: .bold))
            Text(summary)
                .font(.system(size: 11))
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .contentTransition(.numericText())
            Spacer()
            ConnectionIndicator(feed: feed)
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
        .animation(.snappy, value: summary)
    }

    private var summary: String {
        let running = feed.rows.count { $0.lane == .running }
        let queued = feed.rows.count { $0.lane == .queued }
        return [running > 0 ? "\(running) running" : nil, queued > 0 ? "\(queued) queued" : nil]
            .compactMap { $0 }
            .joined(separator: " · ")
    }

    private var jobList: some View {
        let lanes = Lane.allCases
            .map { lane in (lane: lane, rows: feed.rows.filter { $0.lane == lane }) }
            .filter { !$0.rows.isEmpty }
        // A scroll view has no height of its own, so size it to its content.
        let height = lanes.reduce(CGFloat(8)) { $0 + LaneHeader.height + CGFloat($1.rows.count) * JobRowView.height }

        return ScrollView {
            LazyVStack(spacing: 0) {
                ForEach(lanes, id: \.lane) { lane in
                    LaneHeader(lane: lane.lane, count: lane.rows.count)
                    ForEach(lane.rows) { row in
                        JobRowView(row: row).transition(.opacity.combined(with: .move(edge: .top)))
                    }
                }
            }
            .padding(.vertical, 4)
            .animation(.snappy(duration: 0.3), value: feed.rows.map { "\($0.id)-\($0.lane)" })
        }
        .frame(height: min(height, 460))
    }

    private var emptyState: some View {
        VStack(spacing: 6) {
            Text("· · · · · · · ·")
                .font(.system(size: 13, design: .monospaced))
                .foregroundStyle(.tertiary)
                .accessibilityHidden(true)
            Text(emptyText).font(.system(size: 12)).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity)
        .frame(height: 84)
    }

    private var emptyText: String {
        switch feed.phase {
        case .live: "Watching for runs."
        case .connecting: "Contacting the tower…"
        case .signedOut: "Sign in to see your jobs."
        case .needsServer: "Add your ATC server to begin."
        }
    }

    private var serverField: some View {
        TextField("https://atc.example.com", text: $draft)
            .textFieldStyle(.roundedBorder)
            .font(.system(size: 12))
            .onSubmit {
                UserDefaults.standard.set(draft.trimmingCharacters(in: .whitespaces), forKey: Feed.serverKey)
                editingServer = false
                feed.start()
            }
            .padding(.horizontal, 12)
            .padding(.bottom, 10)
            .accessibilityLabel("ATC server URL")
    }

    private var footer: some View {
        HStack(spacing: 10) {
            if let url = feed.server {
                Button {
                    NSWorkspace.shared.open(url)
                } label: {
                    Label("Open dashboard", systemImage: "arrow.up.right")
                        .font(.system(size: 11))
                }
                .buttonStyle(.link)
            }
            Spacer()
            if feed.phase == .signedOut {
                Button("Sign in") { feed.signIn() }
                    .buttonStyle(.borderedProminent)
                    .controlSize(.small)
            }
            IconButton(symbol: "gearshape", label: "Server") {
                withAnimation(.snappy(duration: 0.2)) { editingServer.toggle() }
            }
            IconButton(symbol: "power", label: "Quit ATC") { NSApp.terminate(nil) }
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
    }
}

private struct IconButton: View {
    let symbol: String
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Image(systemName: symbol).font(.system(size: 12)).frame(width: 20, height: 20)
        }
        .buttonStyle(.borderless)
        .help(label)
        .accessibilityLabel(label)
    }
}

/// An uppercase lane label with its count, like the web UI's column headers.
private struct LaneHeader: View {
    static let height: CGFloat = 24

    let lane: Lane
    let count: Int

    var body: some View {
        HStack(spacing: 6) {
            Text(title).tracking(1)
            Text("\(count)").monospacedDigit().foregroundStyle(.tertiary)
            Spacer()
        }
        .font(.system(size: 10, weight: .medium))
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .frame(height: Self.height)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }

    private var title: String {
        switch lane {
        case .running: "RUNNING"
        case .queued: "QUEUED"
        case .completed: "COMPLETED"
        }
    }
}

private struct JobRowView: View {
    static let height: CGFloat = 40

    let row: JobRow
    @State private var hovered = false

    var body: some View {
        Button {
            if let url = row.url { NSWorkspace.shared.open(url) }
        } label: {
            HStack(spacing: 8) {
                StatusGlyph(status: row.status)
                VStack(alignment: .leading, spacing: 1) {
                    Text(row.job.name)
                        .font(.system(size: 13, weight: .semibold))
                        .tracking(-0.13)
                        .lineLimit(1)
                    Text(meta)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                Spacer(minLength: 8)
                timing
            }
            .padding(.horizontal, 8)
            .frame(height: Self.height - 2)
            .background(background, in: RoundedRectangle(cornerRadius: 6))
            .padding(.horizontal, 4)
            .frame(height: Self.height)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .help("\(row.run.org)/\(row.run.repo) — \(row.status.label). Click to open on GitHub.")
        .accessibilityLabel("\(row.job.name), \(row.status.label), \(meta)")
    }

    private var meta: String {
        [row.run.repo, row.run.workflowName, row.run.branch].compactMap { $0 }.joined(separator: " · ")
    }

    /// Failures stay tinted so they are unmistakable while scanning.
    private var background: Color {
        if hovered { return .primary.opacity(0.08) }
        return row.status == .failure || row.status == .timedOut ? Palette.failed.opacity(0.10) : .clear
    }

    /// A running clock for a job in progress; run time and age for a finished one.
    @ViewBuilder private var timing: some View {
        if let started = row.job.startedAt {
            VStack(alignment: .trailing, spacing: 1) {
                if let completed = row.job.completedAt {
                    Text(Duration.seconds(completed.timeIntervalSince(started))
                        .formatted(.units(allowed: [.hours, .minutes, .seconds], width: .narrow)))
                    // The style formats against the current time, so re-render on a timer.
                    TimelineView(.periodic(from: .now, by: 30)) { _ in
                        Text(completed.formatted(.relative(presentation: .numeric, unitsStyle: .narrow)))
                    }
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                } else if row.job.status == .inProgress {
                    Text(started, style: .timer).foregroundStyle(row.status.color)
                }
            }
            .font(.system(size: 11))
            .monospacedDigit()
            .foregroundStyle(.secondary)
        }
    }
}

/// The status symbol. A running job gets the web UI's pulsing halo.
private struct StatusGlyph: View {
    let status: StatusKey

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var pulsed = false

    var body: some View {
        Image(systemName: status.symbol)
            .font(.system(size: 11, weight: .bold))
            .foregroundStyle(status.color)
            .frame(width: 20, height: 20)
            .background {
                if status == .inProgress && !reduceMotion {
                    Circle()
                        .fill(status.color.opacity(0.25))
                        .scaleEffect(pulsed ? 1.15 : 0.6)
                        .opacity(pulsed ? 1 : 0)
                        .animation(.easeInOut(duration: 1).repeatForever(autoreverses: true), value: pulsed)
                        .onAppear { pulsed = true }
                        .onDisappear { pulsed = false }
                }
            }
            .accessibilityHidden(true)
    }
}
