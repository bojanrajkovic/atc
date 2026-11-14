import ServiceManagement
import SwiftUI

@main
struct ATCMenubarApp: App {
    @State private var feed = Feed()

    init() {
        // Open at login from the first launch of an installed copy. Only once,
        // so turning it off in the popover or System Settings sticks, and
        // never for a development build run from the build folder.
        let defaults = UserDefaults.standard
        if Bundle.main.bundlePath.hasPrefix("/Applications/"), !defaults.bool(forKey: "registeredLoginItem") {
            defaults.set(true, forKey: "registeredLoginItem")
            try? SMAppService.mainApp.register()
        }
    }

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
