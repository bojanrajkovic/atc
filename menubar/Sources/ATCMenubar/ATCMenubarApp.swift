import SwiftUI

@main
struct ATCMenubarApp: App {
    @State private var feed = Feed()

    var body: some Scene {
        MenuBarExtra {
            JobsView(feed: feed)
        } label: {
            // The count is the number of queued and running jobs.
            Image(systemName: feed.phase == .live ? "airplane" : "airplane.circle").accessibilityLabel("ATC")
            if feed.activeCount > 0 { Text("\(feed.activeCount)") }
        }
        .menuBarExtraStyle(.window)
    }
}
