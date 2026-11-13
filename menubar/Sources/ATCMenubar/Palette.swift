import ATCKit
import AppKit
import SwiftUI

/// ATC's status colors: the dark and light tokens from `.impeccable.md`.
/// As in the web UI, these are the only saturated colors on screen.
enum Palette {
    static let queued = token(dark: (0.72, 0.15, 250), light: (0.45, 0.18, 250))
    static let running = token(dark: (0.78, 0.16, 80), light: (0.45, 0.15, 80))
    static let success = token(dark: (0.72, 0.16, 155), light: (0.42, 0.15, 155))
    static let failed = token(dark: (0.72, 0.17, 25), light: (0.48, 0.18, 25))
    static let timedOut = token(dark: (0.76, 0.18, 40), light: (0.49, 0.18, 40))
    static let actionRequired = token(dark: (0.88, 0.20, 55), light: (0.50, 0.18, 55))

    private typealias LCH = (l: Double, c: Double, h: Double)

    private static func token(dark: LCH, light: LCH) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            let isDark = appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
            let lch = isDark ? dark : light
            let rgb = srgb(lightness: lch.l, chroma: lch.c, hue: lch.h)
            return NSColor(srgbRed: rgb.red, green: rgb.green, blue: rgb.blue, alpha: 1)
        })
    }
}

/// Color and symbol for each status. Every status has both, so none is told
/// apart by color alone. The shapes match the web UI's status glyphs.
extension StatusKey {
    var color: Color {
        switch self {
        case .queued: Palette.queued
        case .inProgress: Palette.running
        case .success: Palette.success
        case .failure: Palette.failed
        case .timedOut: Palette.timedOut
        case .actionRequired: Palette.actionRequired
        case .cancelled, .neutral: .secondary
        }
    }

    var symbol: String {
        switch self {
        case .queued: "circle.lefthalf.filled"
        case .inProgress: "play.fill"
        case .success: "checkmark"
        case .failure: "xmark"
        case .cancelled: "circle.slash"
        case .timedOut: "stopwatch"
        case .actionRequired: "exclamationmark.triangle"
        case .neutral: "circle"
        }
    }

    var label: String {
        switch self {
        case .queued: "Queued"
        case .inProgress: "In progress"
        case .success: "Success"
        case .failure: "Failure"
        case .cancelled: "Cancelled"
        case .timedOut: "Timed out"
        case .actionRequired: "Action required"
        case .neutral: "Neutral"
        }
    }
}
