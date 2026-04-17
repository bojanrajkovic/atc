# Human Test Plan: Kanban Board (Sub-Phase 3)

Generated: 2026-04-16

## Prerequisites

- Environment bootstrapped: `just setup`
- All automated tests passing: `just test` (unit + browser), `pnpm --prefix frontend test:e2e` (E2E)
- Dev server running: `just dev` or `pnpm --prefix frontend dev`

## Phase 1: Grid Layout Verification (AC1.3)

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `http://localhost:5173/` in a browser with DevTools open | Page loads with the ATC app shell |
| 2 | Ensure the app is connected and has at least one workflow run visible | Three-column kanban board is visible |
| 3 | Right-click the three-column grid container and select "Inspect" | DevTools Elements panel opens |
| 4 | Verify the container has `display: grid` and `grid-template-columns: repeat(3, 1fr)` (Tailwind class `grid grid-cols-3`) | Computed styles show CSS Grid with three equal-width columns |
| 5 | Resize browser window from 1920px to 1024px width | All three columns remain visible and equal width; no horizontal overflow |
| 6 | Compare the live layout against `docs/ideation/playground.html` | Three-column layout matches the prototype |

## Phase 2: RunCard Scope Contract (AC4.3)

| Step | Action | Expected |
|------|--------|----------|
| 1 | Open `frontend/src/lib/components/RunCard.svelte` in an editor | Scope-contract comment block is visible at top |
| 2 | Search file for: `StatusIcon`, `ProgressBar`, `JobMeta`, `JobHeader`, `RunnerLabel` | Zero matches for all five forbidden imports |
| 3 | Search file for: `@keyframes` | Zero matches |
| 4 | Search file for: `setInterval` | Zero matches |
| 5 | Verify the scope-contract comment block is present and lists the forbidden patterns | Comment enumerates: forbidden imports, forbidden CSS, forbidden JS, forbidden content |

## Phase 3: Visual Verification of Status Indicators

| Step | Action | Expected |
|------|--------|----------|
| 1 | With dev server running, navigate to `http://localhost:5173/` | App loads |
| 2 | Inject or wait for runs in all three statuses (Queued, InProgress, Completed) | Cards appear in their respective columns |
| 3 | In the QUEUED column, verify the status indicator on each card | Hollow circle glyph, colored with the `--queued` token |
| 4 | In the IN PROGRESS column, verify the status indicator on each card | Filled triangle (play) glyph, colored with the `--running` token |
| 5 | In the COMPLETED column, verify the status indicator on each card | Filled circle glyph, colored with the `--text-dim` token |

## Phase 4: Animation and Motion Verification

| Step | Action | Expected |
|------|--------|----------|
| 1 | With the kanban board populated, trigger a run status change (Queued to InProgress) | Card smoothly animates out of source column and into destination column |
| 2 | Observe the card arriving in the destination column | Card flies in from below with subtle 20px settle-Y distance; ~250ms duration |
| 3 | Trigger a within-column reorder | Cards smoothly swap positions with a FLIP animation (~300ms) |
| 4 | Open DevTools Rendering tab, check "Emulate CSS media feature prefers-reduced-motion" | Reduced motion emulation activates |
| 5 | Repeat steps 1-3 | Cards appear instantly in new positions with no visible animation |

## Phase 5: Full Kanban Lifecycle

| Step | Action | Expected |
|------|--------|----------|
| 1 | Start dev server with `pnpm --prefix frontend dev` (no backend) | Vite dev server starts |
| 2 | Navigate to `http://localhost:5173/` | "Connecting..." placeholder visible |
| 3 | Start the backend server (`just dev` in a separate terminal) | Board transitions to either "No workflows yet." or three-column grid |
| 4 | Trigger a GitHub Actions webhook | Card appears in QUEUED column |
| 5 | Wait for workflow to start running (or send InProgress webhook) | Card moves from QUEUED to IN PROGRESS with animation |
| 6 | Wait for workflow to complete (or send Completed webhook) | Card moves from IN PROGRESS to COMPLETED with animation |
| 7 | Verify column header count badges update at each step | Counts increment/decrement correctly |

## Phase 6: Snapshot Reload Stability

| Step | Action | Expected |
|------|--------|----------|
| 1 | With the kanban board populated with multiple runs | Cards displayed in sorted order |
| 2 | Disconnect the network (DevTools "Offline" or stop backend) | Connection indicator changes; board remains as-is |
| 3 | Reconnect the network (or restart backend) | Connection re-establishes; snapshot loads |
| 4 | Observe the board after reconnection | Cards in same order — no reshuffling or flash |

## Traceability Matrix

| AC | Automated | Manual |
|----|-----------|--------|
| AC1.1-AC1.2 | Browser tests | -- |
| AC1.3 | -- | Phase 1 |
| AC2.1-AC2.2 | Unit tests | -- |
| AC3.1-AC3.7 | Unit tests | -- |
| AC4.1-AC4.2 | Unit tests | Phase 3 (visual) |
| AC4.3 | -- | Phase 2 |
| AC5.1-AC5.6 | Unit + browser tests | Phase 4 (visual) |
| AC6.1-AC6.4 | Unit + browser tests | Phase 4 steps 4-5 |
| AC7.1-AC7.6 | Browser tests | -- |
| AC8.1-AC8.3 | E2E tests | Phase 5 |
