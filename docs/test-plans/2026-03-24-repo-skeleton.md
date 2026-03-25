# Human Test Plan — Repo Skeleton

Generated: 2026-03-25
Implementation plan: `docs/implementation-plans/2026-03-24-repo-skeleton/`

## Prerequisites

- Development environment bootstrapped: `just setup` has completed successfully
- All automated operational checks pass (see Automated Verification table below)
- A modern browser (Chrome, Firefox, or Safari) available for visual testing
- Terminal available for git operations

## Phase 1: Frontend Visual Verification

| Step | Action | Expected |
|------|--------|----------|
| 1.1 | Run `cd frontend && pnpm dev` | Terminal shows Vite dev server started at `http://localhost:5173` |
| 1.2 | Open `http://localhost:5173` in a browser | Page loads. Heading "ATC — Actions Traffic Control" visible in accent color (amber/orange for default "warm" theme). Body uses light off-white background. Subtitle visible in secondary text color. |
| 1.3 | Verify four theme buttons are visible | Four buttons labeled "warm", "radar", "violet", "pink" in a horizontal row. "warm" highlighted. |
| 1.4 | Click "radar" button | Heading accent color changes to teal/green. "radar" button highlighted. |
| 1.5 | Click "violet" button | Heading accent color changes to purple. |
| 1.6 | Click "pink" button | Heading accent color changes to magenta/pink. |
| 1.7 | Click "warm" button | Heading accent color returns to amber/orange. Full 4-hue cycle demonstrated. |
| 1.8 | Click "Dark Mode" button | Background transitions (0.2s ease) to dark surface. Text becomes light. Button label changes to "Light Mode". Status color swatches (green, yellow, red, blue) remain visible. |
| 1.9 | In dark mode, click through all four theme buttons | Each theme changes accent hue. All 4 hues distinct in dark mode. |
| 1.10 | Click "Light Mode" button | Background transitions smoothly back to light. Transition is animated, not instant. |
| 1.11 | Stop Vite dev server (Ctrl+C) | Server stops cleanly |

## Phase 2: Git Hook Verification — Rust Files

| Step | Action | Expected |
|------|--------|----------|
| 2.1 | Add `// hook test` to end of `backend/crates/atc-core/src/lib.rs` | File saved |
| 2.2 | `git add backend/crates/atc-core/src/lib.rs` | File staged |
| 2.3 | `git commit -m "test: verify rs hooks"` | Lefthook output shows `clippy` and `rustfmt` hooks RAN (not "skipped"). Commit may succeed or fail — key is hooks fired. |
| 2.4 | Revert: `git reset HEAD~1` (if committed) then `git checkout -- backend/crates/atc-core/src/lib.rs` | Working tree clean |

## Phase 3: Git Hook Verification — TypeScript Files

| Step | Action | Expected |
|------|--------|----------|
| 3.1 | Add `// hook test` to end of `frontend/src/main.ts` | File saved |
| 3.2 | `git add frontend/src/main.ts` | File staged |
| 3.3 | `git commit -m "test: verify ts hooks"` | Lefthook output shows `biome` hook RAN (not "skipped"). |
| 3.4 | Revert: `git reset HEAD~1` (if committed) then `git checkout -- frontend/src/main.ts` | Working tree clean |

## Phase 4: Git Hook Verification — Svelte Files

| Step | Action | Expected |
|------|--------|----------|
| 4.1 | Add `// hook test` after `<script lang="ts">` in `frontend/src/App.svelte` | File saved |
| 4.2 | `git add frontend/src/App.svelte` | File staged |
| 4.3 | `git commit -m "test: verify svelte hooks"` | Lefthook output shows `eslint-svelte` hook RAN (not "skipped"). |
| 4.4 | Revert: `git reset HEAD~1` (if committed) then `git checkout -- frontend/src/App.svelte` | Working tree clean |

## Phase 5: Doc-Staleness Gate Verification

| Step | Action | Expected |
|------|--------|----------|
| 5.1 | `git checkout -b test-doc-staleness` | New branch created |
| 5.2 | Add `// staleness test` to end of `backend/crates/atc-server/src/main.rs` | File saved |
| 5.3 | `git add backend/crates/atc-server/src/main.rs && git commit -m "test: staleness check"` | Commit succeeds |
| 5.4 | `scripts/check-docs-lefthook.sh` | Exit code 1. Output mentions `backend-server.md` was not updated. |
| 5.5 | `git checkout main && git branch -D test-doc-staleness` | Cleaned up |

## End-to-End: Full Build and Serve

| Step | Action | Expected |
|------|--------|----------|
| E2E.1 | `just build` | Frontend builds first (dist/ created), then backend release binary compiled. |
| E2E.2 | `./backend/target/release/atc-server` | "listening on http://0.0.0.0:8080" |
| E2E.3 | `curl http://localhost:8080/health` | HTTP 200, `{"status":"ok"}` |
| E2E.4 | Open `http://localhost:8080/` in browser | Full Svelte app loads with theme buttons. |
| E2E.5 | Navigate to `http://localhost:8080/dashboard` | Svelte app loads (SPA fallback), not 404. |
| E2E.6 | Navigate to `http://localhost:8080/some/deep/path` | Same — SPA fallback works for arbitrary paths. |
| E2E.7 | Stop server (Ctrl+C) | Clean stop |

## End-to-End: Empty Frontend Dist

| Step | Action | Expected |
|------|--------|----------|
| EF.1 | `rm -rf frontend/dist && mkdir -p frontend/dist` | Empty dist/ |
| EF.2 | `cd backend && cargo build --release -p atc-server` | Build succeeds |
| EF.3 | `./backend/target/release/atc-server` | Server starts |
| EF.4 | `curl -s -o /dev/null -w "%{http_code}" http://localhost:8080/` | HTTP 404 |
| EF.5 | `curl -s http://localhost:8080/` | "frontend not embedded" |
| EF.6 | Stop server, run `just build` to restore | Normal state restored |

## End-to-End: Dev Mode Proxy

| Step | Action | Expected |
|------|--------|----------|
| DP.1 | `just dev` | Both servers start: Vite :5173, Axum :8080 |
| DP.2 | `curl http://localhost:8080/health` | HTTP 200, `{"status":"ok"}` — served by Axum |
| DP.3 | Open `http://localhost:8080/` in browser | Svelte app loads via proxy. Content matches :5173. |
| DP.4 | Edit `App.svelte` heading text and save | HMR triggers — browser at :8080/ reflects change without refresh. |
| DP.5 | Stop `just dev` (Ctrl+C) | Both servers stop |
| DP.6 | `git checkout -- frontend/src/App.svelte` | File restored |

## Human Verification Required

| Criterion | Why Manual | Test Phase |
|-----------|-----------|------------|
| AC2.3 — OKLCH styling renders | Automated tests can't verify perceptual color correctness | Phase 1, Steps 1.2-1.3 |
| AC2.4 — 4 themes switch | Color differences require visual comparison | Phase 1, Steps 1.4-1.7 |
| AC2.5 — Dark/light mode | Background/text contrast and transition animation | Phase 1, Steps 1.8-1.10 |
| AC4.1 — .rs triggers hooks | Must observe hooks fire during real git commit | Phase 2 |
| AC4.2 — .ts triggers hooks | Must observe hooks fire during real git commit | Phase 3 |
| AC4.3 — .svelte triggers hooks | Must observe hooks fire during real git commit | Phase 4 |
| AC5.5 — Doc-staleness gate | Requires throwaway branch + commit | Phase 5 |

## Traceability

| AC | Automated | Manual |
|----|-----------|--------|
| AC1.1-AC1.8 | `cargo check/test`, `git ls-files`, curl | E2E.2-E2E.6 |
| AC2.1-AC2.2 | `pnpm dev/build` | Phase 1 Step 1.1, E2E.1 |
| AC2.3-AC2.5 | — | Phase 1 Steps 1.2-1.10 |
| AC2.6-AC2.8 | `biome check`, `eslint`, `prettier` | — |
| AC3.1-AC3.8 | `just` recipes | E2E, EF |
| AC4.1-AC4.3 | — | Phases 2-4 |
| AC4.4-AC4.5 | `lefthook run pre-push` | Phase 5 |
| AC5.1-AC5.4 | grep + file checks | — |
| AC5.5 | — | Phase 5 |
