import Foundation

/// Converts an OKLCH color to gamma-encoded sRGB components in 0...1.
///
/// ATC's design tokens (`.impeccable.md`) are written in OKLCH. Converting at
/// run time keeps the app's palette readable against that file.
/// `lightness` is 0...1, `hue` is in degrees. Out-of-gamut results are clamped.
public func srgb(lightness: Double, chroma: Double, hue: Double) -> (red: Double, green: Double, blue: Double) {
    let a = chroma * cos(hue * .pi / 180)
    let b = chroma * sin(hue * .pi / 180)

    let l = pow(lightness + 0.3963377774 * a + 0.2158037573 * b, 3)
    let m = pow(lightness - 0.1055613458 * a - 0.0638541728 * b, 3)
    let s = pow(lightness - 0.0894841775 * a - 1.2914855480 * b, 3)

    func encode(_ linear: Double) -> Double {
        let x = min(max(linear, 0), 1)
        return x <= 0.0031308 ? 12.92 * x : 1.055 * pow(x, 1 / 2.4) - 0.055
    }
    return (
        encode(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s),
        encode(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s),
        encode(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    )
}
