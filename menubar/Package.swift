// swift-tools-version:6.2
import PackageDescription

// ATC Menubar: a macOS menu bar app that lists the latest jobs from an ATC server.
//
// ATCKit holds the wire types and the job ordering, with no UI, so it is
// testable without a window. ATCMenubar is the app: feed, sign-in, and views.
let package = Package(
    name: "ATCMenubar",
    platforms: [.macOS(.v26)],
    targets: [
        .target(name: "ATCKit"),
        .executableTarget(name: "ATCMenubar", dependencies: ["ATCKit"]),
        .testTarget(name: "ATCKitTests", dependencies: ["ATCKit"]),
    ]
)
