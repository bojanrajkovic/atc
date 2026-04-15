# Human Test Plan: Frontend Foundation

## Prerequisites

- Run `just dev` to start both backend and frontend dev servers
- Open browser to `http://localhost:5173`

## AC1: Design System Tokens

### AC1.1: Dark mode default with rich OKLCH chroma

1. Open the app at `/` with no prior localStorage state (clear `atc-theme` and `atc-mode` keys)
2. Verify surfaces have visible color tinting — backgrounds should have a subtle teal/green hue (radar theme default), not neutral gray
3. Compare against the old scaffold's near-black flat surfaces — the new tokens use chroma 0.063, producing perceptible hue in dark surfaces

### AC1.2: Light mode override

1. Click the mode toggle to switch to light mode
2. Verify surfaces lighten (near-white backgrounds) and text darkens while maintaining the theme's color tinting
3. Inspect `app.css` — the `[data-mode="light"]` block should only contain tokens that change, not a full redefinition of all tokens

### AC1.4: Status colors constant across themes

1. For each theme (warm, radar, violet, pink), click the theme button
2. Observe the status color swatches — queued (blue), running (amber), success (green), failed (red) should appear visually identical across all four themes
3. **Note:** `--cancelled` intentionally uses `var(--text-dim)` which shifts with the theme hue. This is by design — cancelled items blend with the theme's text palette rather than using a fixed status color

### AC1.5: shadcn components via CSS alias layer

1. shadcn components (Card, Badge, Toggle, Progress) are installed but not yet rendered in the current scaffold
2. When components are added to the UI, verify they use ATC design tokens (not shadcn default gray palette)
3. Inspect a Card element — its background should match `var(--surface)`, not a hardcoded value

### AC1.6: prefers-reduced-motion

1. Open browser DevTools > Rendering > Emulate CSS media feature `prefers-reduced-motion: reduce`
2. Verify no visible transitions or animations play when switching themes or toggling modes
3. Changes should apply instantly without any motion

## AC2: Generated TypeScript Types

### AC2.1-AC2.4: Type generation verification

1. Run `just types` — verify it completes without errors
2. Spot-check `frontend/src/lib/types/generated/WorkflowRun.ts` — fields should be camelCase (`headSha`, `htmlUrl`, `createdAt`)
3. Spot-check `frontend/src/lib/types/generated/WebhookEvent.ts` — should be `{ type: "Run", data: ... } | { type: "Job", data: ... }`
4. Spot-check `frontend/src/lib/types/generated/RunStatus.ts` — should be `"Queued" | "InProgress" | "Completed"`

## Automated Test Summary

All remaining acceptance criteria (AC3, AC4, AC5) are fully covered by automated tests:

- **AC3 (Stores):** 48 Vitest unit tests covering all 9 sub-criteria
- **AC4 (ConnectionManager):** 16 Vitest integration tests covering all 8 sub-criteria
- **AC5 (E2E):** 14 Playwright E2E tests covering rendering, theme switching, mode toggle, reduced motion, and attribute independence
