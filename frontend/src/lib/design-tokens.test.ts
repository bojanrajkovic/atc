import { describe, expect, it } from 'vitest'

// WCAG Contrast Ratio Test: Verify all status/conclusion tokens meet AA (4.5:1) on --surface
// across all four theme hues (warm/radar/violet/pink) in both dark and light modes.
// AAA misses (< 7:1 but >= 4.5:1) are reported as informational output only.

// ========================= OKLCH -> contrast math =========================
// Ported from docs/ideation/status-token-playground.html:756–793

interface OklchTriple {
  L: number // 0–100 percentage form
  C: number // unit chroma (0–0.4 typical)
  H: number // degrees 0–360, NaN means follows active theme hue
}

function oklchToLinearRgb(
  L100: number,
  C: number,
  hDeg: number,
): { r: number; g: number; b: number } {
  const L = L100 / 100
  const hRad = (hDeg * Math.PI) / 180
  const a = C * Math.cos(hRad)
  const b = C * Math.sin(hRad)

  const l_ = L + 0.3963377774 * a + 0.2158037573 * b
  const m_ = L - 0.1055613458 * a - 0.0638541728 * b
  const s_ = L - 0.0894841775 * a - 1.291485548 * b

  const l = l_ ** 3
  const m = m_ ** 3
  const s = s_ ** 3

  return {
    r: 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    g: -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    b: -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  }
}

function luminance({ r, g, b }: { r: number; g: number; b: number }): number {
  r = Math.max(0, Math.min(1, r))
  g = Math.max(0, Math.min(1, g))
  b = Math.max(0, Math.min(1, b))
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

function contrast(fg: OklchTriple, bg: OklchTriple): number {
  const l1 = luminance(oklchToLinearRgb(fg.L, fg.C, fg.H))
  const l2 = luminance(oklchToLinearRgb(bg.L, bg.C, bg.H))
  const light = Math.max(l1, l2)
  const dark = Math.min(l1, l2)
  return (light + 0.05) / (dark + 0.05)
}

// ========================= Token definitions =========================

type TokenName =
  | 'queued'
  | 'running'
  | 'success'
  | 'failed'
  | 'cancelled'
  | 'timed-out'
  | 'action-required'
  | 'neutral'
  | 'startup-failure'
  | 'stale'
  | 'skipped'

// Theme hue map from docs/ideation/status-token-playground.html:753
const themeHueMap = {
  warm: 70,
  radar: 155,
  violet: 280,
  pink: 310,
}

type ThemeName = keyof typeof themeHueMap

// Values copied from frontend/src/app.css :root and [data-mode="light"]
// cancelled ALIASES --text-dim. The L/C values here duplicate --text-dim
// intentionally: the gate scans --cancelled as a status-color concern, but
// the true source of truth is --text-dim. If --text-dim drifts in app.css
// (it's a text-purpose token, not status-purpose), update these values here
// too or the gate silently tests stale data.
// startup-failure aliases --failed; stale/skipped alias --neutral
const DARK_TOKENS: Record<TokenName, OklchTriple | 'hue-following'> = {
  queued: { L: 72, C: 0.15, H: 250 },
  running: { L: 78, C: 0.16, H: 80 },
  success: { L: 72, C: 0.16, H: 155 },
  failed: { L: 72, C: 0.17, H: 25 },
  cancelled: { L: 72, C: 0.03, H: Number.NaN }, // alias: --text-dim; follows theme hue
  'timed-out': { L: 76, C: 0.18, H: 40 },
  'action-required': { L: 88, C: 0.2, H: 55 },
  neutral: { L: 60, C: 0.025, H: Number.NaN }, // follows theme hue
  'startup-failure': { L: 72, C: 0.17, H: 25 }, // alias of failed
  stale: { L: 60, C: 0.025, H: Number.NaN }, // alias of neutral
  skipped: { L: 60, C: 0.025, H: Number.NaN }, // alias of neutral
}

const LIGHT_TOKENS: Record<TokenName, OklchTriple | 'hue-following'> = {
  queued: { L: 45, C: 0.18, H: 250 },
  running: { L: 45, C: 0.15, H: 80 },
  success: { L: 42, C: 0.15, H: 155 },
  failed: { L: 48, C: 0.18, H: 25 },
  cancelled: { L: 40, C: 0.04, H: Number.NaN }, // alias: --text-dim; follows theme hue
  'timed-out': { L: 49, C: 0.18, H: 40 },
  'action-required': { L: 50, C: 0.18, H: 55 },
  neutral: { L: 45, C: 0.025, H: Number.NaN }, // follows theme hue
  'startup-failure': { L: 48, C: 0.18, H: 25 }, // alias of failed
  stale: { L: 45, C: 0.025, H: Number.NaN }, // alias of neutral
  skipped: { L: 45, C: 0.025, H: Number.NaN }, // alias of neutral
}

// --surface values from frontend/src/app.css
const DARK_SURFACE: OklchTriple = { L: 16, C: 0.063, H: Number.NaN } // follows theme hue
const LIGHT_SURFACE: OklchTriple = { L: 99, C: 0.013, H: Number.NaN } // follows theme hue

// Resolve a token to its concrete OKLCH triple, substituting theme hue where needed
function resolveToken(token: OklchTriple, theme: ThemeName): OklchTriple {
  return {
    L: token.L,
    C: token.C,
    H: Number.isNaN(token.H) ? themeHueMap[theme] : token.H,
  }
}

describe('WCAG contrast gate', () => {
  const modes = ['dark', 'light'] as const
  const themes: ThemeName[] = ['warm', 'radar', 'violet', 'pink']
  const tokenNames: TokenName[] = [
    'queued',
    'running',
    'success',
    'failed',
    'cancelled',
    'timed-out',
    'action-required',
    'neutral',
    'startup-failure',
    'stale',
    'skipped',
  ]

  for (const mode of modes) {
    for (const theme of themes) {
      it(`${mode} ${theme} - all tokens meet AA on --surface`, () => {
        const tokens = mode === 'dark' ? DARK_TOKENS : LIGHT_TOKENS
        const surface = mode === 'dark' ? DARK_SURFACE : LIGHT_SURFACE

        // Resolve surface to concrete triple (substitute theme hue)
        const surfaceResolved = resolveToken(surface, theme)

        for (const tokenName of tokenNames) {
          const tokenDef = tokens[tokenName]
          if (!tokenDef || tokenDef === 'hue-following') {
            throw new Error(`Token ${tokenName} not found in ${mode} tokens`)
          }

          const tokenResolved = resolveToken(tokenDef, theme)
          const ratio = contrast(tokenResolved, surfaceResolved)

          // AA (4.5:1) is the gate — must pass for all combinations
          expect(ratio).toBeGreaterThanOrEqual(4.5)

          // AAA (7:1) is aspirational — report misses as informational output
          if (ratio < 7) {
            // biome-ignore lint/suspicious/noConsole: informational output for maintainers
            console.info(`[AAA miss] ${mode}/${theme}/${tokenName} = ${ratio.toFixed(2)}:1`)
          }
        }
      })
    }
  }
})
