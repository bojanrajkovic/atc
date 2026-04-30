# Human Test Plan — Sub-Phase 5: Interactivity

**Source:** `docs/design-plans/2026-04-25-interactivity.md` (47 acceptance criteria across 6 groups)
**Implementation plan:** `docs/implementation-plans/2026-04-25-interactivity/`
**Test requirements:** `docs/implementation-plans/2026-04-25-interactivity/test-requirements.md`
**Generated:** 2026-04-29
**Coverage status:** PASS — all 47 ACs covered by automated tests; 1 AC (AC3.1 touch-device gating) requires human verification.

> The full coverage matrix lives at the bottom of this document. This plan covers the manual scenarios that automated tests can't fully validate: visual comparisons against the playground, touch-device gating, screen-reader output, theme/mode visual review, and end-to-end user flows that span multiple ACs.

## Prerequisites

- Local dev server running: `cd frontend && pnpm dev`
- Playground reference open in a second tab: `docs/design-plans/playgrounds/2026-04-25-interactivity-explorer.html`
- Backend not required — the dashboard works against a stub WebSocket if one is provided, or you may seed state directly via `window.__stores` in DevTools.
- All automated suites green: `cd frontend && pnpm test --run` (547/547) and `pnpm test:e2e` (79/79).
- Browser: latest stable Chrome (primary). Firefox + Safari for the touch-device portion if available.

## Phase A — Smoke check (5 minutes)

| Step | Action | Expected |
|------|--------|----------|
| A1 | Load `/`. Wait for connection indicator → Connected. | TopBar visible: Logo, runner pools, connection dot, Settings cog. Three kanban columns rendered. |
| A2 | Toggle each of the four themes (warm/radar/violet/pink) via Settings. Toggle dark/light mode. | Status colors stay constant across themes; surfaces shift hue; halo color changes between dark and light. |
| A3 | Toggle density to compact. | Run cards collapse to header-only; hover/focus styles still legible. |

## Phase B — Command palette (15 minutes; covers AC1.x visual review)

The AC1.7 slide animation, AC1.11 three-state rendering, and the AC1.10 empty-state copy are flagged for human visual review per Rule 9.

| Step | Action | Expected |
|------|--------|----------|
| B1 | Press Cmd+K (Ctrl+K on Linux/Windows). | Palette opens centered with backdrop; search input focused; cursor in input. Compare layout & spacing to playground. |
| B2 | Press Cmd+K again. | Palette closes. State of any prior query/recents is preserved on next open. |
| B3 | Open palette. Type "build". | Sections re-order results internally but section headings remain in fixed source order: Recent → Runs → Jobs → Runner Pools → Commands. Sections with zero matches disappear. |
| B4 | Clear the input (Backspace). | All sections return to default ordering. |
| B5 | Type a guaranteed-no-match string like `zzz_no_match_zzz`. | Empty state appears with **exact** text `Nothing in flight matching "zzz_no_match_zzz".` Confirm the quotes are typographic curly quotes (U+201C and U+201D), not straight quotes. |
| B6 | Type a fragment that matches a runner pool's labels (e.g. `lin`). | Pool rows wrap (white-space normal) and each matched substring is wrapped in a `<mark>` highlight. Compare to playground's query-active state — AC1.11. |
| B7 | Without typing, arrow-down to focus a Pool row. | Focused pool row wraps regardless of query. The right-edge meta column (running/queued counts) stays at a stable ch-based gutter and does not jitter as the row wraps. AC1.11. |
| B8 | Click on a Run row. | selectedRunId set: detail panel slides in from the right. Palette closes. The clicked run appears at the top of the Recent section the next time the palette is opened. |
| B9 | Open palette again. Click a Job row. | Detail panel opens to the parent run AND scrolls smoothly to the matching JobBlock. |
| B10 | Open palette. Click a Pool row. | Pool filter pill appears in the TopBar with `Filtering by [labels] · ✕`. Matching RunnerPool indicator gets a 2px accent border + opacity boost (compare to playground). Non-matching runs disappear from columns. |
| B11 | Open palette. Click "Switch theme…". | Body slide-transitions to theme submenu (left-to-right slide; AC1.7 visual review). Search input remains anchored at top. Four theme options visible. |
| B12 | Press Escape inside the submenu. | Submenu collapses back to top-level palette. Dialog stays open. |
| B13 | Re-enter the submenu and click Violet. | Theme applies (page hue changes to violet). subMenu clears. Palette closes. |
| B14 | With pool filter still active and a run open, open palette. | "Clear pool filter" command appears under Commands. "Close detail panel" command also appears. |
| B15 | Close the panel and clear the filter. Open palette. | Both commands disappear from the Commands section (AC1.12, AC1.13). |
| B16 | Visually compare palette typography, spacing, status icons, and pool row layout against the playground. | Match. Differences should be intentional and documented. |

## Phase C — Run detail panel (10 minutes; AC2.x visual + behavior)

| Step | Action | Expected |
|------|--------|----------|
| C1 | Click a run card to open the panel. | Panel slides in from the right. Layout: PanelHeader → MetaGrid → flat list of JobBlocks (each with StepList). Compare layout to playground (AC2.1). |
| C2 | Click "Go to run" link in the header. | Opens GitHub run URL in a new tab. Original tab remains. (rel=noopener noreferrer attrs already verified by automation.) |
| C3 | With panel open, press Tab repeatedly. | Focus cycles only inside the dialog (close button → "Go to run" link → other interactive elements → wraps). |
| C4 | Press Esc. | Panel slides out. Focus returns to the originating run card's activator button (visible focus ring on the card you opened). |
| C5 | Click a different run card; once panel is open, click on the dim region outside the panel (e.g. left side of viewport). | Panel closes. |
| C6 | Open a panel. Open palette via Cmd+K. Click on a Job row in the palette. | Panel scrolls smoothly to the corresponding JobBlock. The job becomes visible — confirm scroll is RAF-smooth, not a jump. |
| C7 | Visually inspect panel for each of these run states: Queued, InProgress, Success, Failure, Cancelled, TimedOut, ActionRequired, StartupFailure, Stale, Neutral, Skipped. | Each panel header shows the correct status glyph and `--status-color`. Step list inside JobBlocks shows correct per-step glyph. Use webhook fixtures or seed via DevTools with `window.__stores!.runStore!.applyRunEvent(...)`. |

## Phase D — Hover-peek (10 minutes; AC3.1 covers human-only touch verification)

This phase is the canonical human-only block per `test-requirements.md` (Human Verification table).

| Step | Action | Expected |
|------|--------|----------|
| D1 | On a desktop browser with mouse, hover any run card in the leftmost (Queued) column. | After ~250ms, popover appears anchored to the **right edge** of the card. Compare positioning + content to playground. Popover shows: status, job count, "N of M steps complete", duration, runner. |
| D2 | Quickly mouse-leave (move within ~200ms). | Popover never appears (debounce gate). |
| D3 | Hover a card in the **rightmost** (Completed) column. | Popover appears on the **left** side of the card (Floating UI auto-flip). Verify it does not overflow the viewport. |
| D4 | Hover, wait for popover, then click the card. | Popover dismisses synchronously. Detail panel opens. |
| D5 | Hover, wait for popover, then move the mouse off the card. | Popover dismisses synchronously (no fade-out delay). |
| D6 | **Touch-device gating (canonical human verification):** Open Chrome DevTools → Device Toolbar → select "iPad" or "iPhone" responsive preset (forces Touch and `pointer: coarse`). Reload page. | Hovering (or simulating hover via mouse on the responsive overlay) does NOT trigger the popover. The `(hover: hover) and (pointer: fine)` media query evaluates false; the popover element should not even mount in the DOM. |
| D7 | While in touch emulation, tap a run card. | Detail panel opens directly with no intermediate popover. |
| D8 | (Optional, if a real device is available) Load the page on a physical iPad/iPhone/Android. Tap a card. | Same: panel opens directly; no popover ever shows. |

## Phase E — RunCard interactivity & accessibility (10 minutes)

| Step | Action | Expected |
|------|--------|----------|
| E1 | Tab from the address bar into the page until focus reaches the first run card. | Focus ring lands on the card's inner activator button. The article element itself is not in the tab order. |
| E2 | Continue Tab. | Focus advances column-by-column, top-to-bottom: Queued column cards in vertical order → InProgress column cards → Completed column cards. |
| E3 | Press Enter on a focused card. | Detail panel opens for that run. Esc to close. |
| E4 | Press Space on a focused card. | Detail panel opens for that run. |
| E5 | Move mouse pointer over the run title text inside a card and click. | Activation still fires (clicks bubble through the transparent overlay button). Detail panel opens. |
| E6 | Run a screen reader (VoiceOver on macOS via Cmd+F5, or NVDA/JAWS on Windows). Tab to a card. | Reader announces `article, button, "<title>, <status>, <repo>·<branch>"`. Confirm the form. |
| E7 | If a card has a null branch in its data, screen-reader output omits the trailing `·` and just reads `<title>, <status>, <repo>`. | Confirmed. |

## Phase F — Pool filter integration (5 minutes; AC5.x visual + flow)

| Step | Action | Expected |
|------|--------|----------|
| F1 | With no filter active, confirm: no PoolFilterPill in the TopBar; no RunnerPool has the active-filter accent border; all runs visible across columns. | Default state (AC5.5). |
| F2 | Open palette. Click a pool row. | PoolFilterPill renders with `Filtering by [sorted-labels-dot-separated] · ✕`. Matching RunnerPool indicator gets the accent border + opacity boost (compare to playground — AC5.2 visual review). Non-matching runs hide across all three columns. |
| F3 | Click the ✕ on the pill. | Filter clears. State returns to F1. |
| F4 | Reapply a filter. Open palette. Click "Clear pool filter". | Filter clears. Pill disappears. Palette closes. |
| F5 | Use DevTools console to set a deliberately non-matching filter: `window.__stores.uiStore.activePoolFilter = window.__stores.poolKey(['nonexistent-label'])` | All three columns become empty. Pill remains visible reading `Filtering by nonexistent-label · ✕`. No JS errors. |

## Phase G — Sheet + Command stacking (10 minutes; AC6.x — primary for Phase 6)

| Step | Action | Expected |
|------|--------|----------|
| G1 | Click a run card to open the panel. | Panel open, `selectedRunId` set. |
| G2 | Press Cmd+K. | Palette opens **on top** of the panel. Both dialogs visible simultaneously. Palette has focus (search input cursor). Backdrop appears continuous (no double-darkening — only one overlay visible per AC6.5; the second is suppressed by CSS sibling combinator). |
| G3 | Press Cmd+K again. | Palette closes. Panel remains open. |
| G4 | With both open again (Cmd+K), press Esc once. | Palette closes only. Panel remains open. Focus returns to the panel's "Close detail panel" button (visible focus ring on the X). |
| G5 | Press Esc again. | Panel closes. Focus returns to the originating run card's activator button. |
| G6 | Repeat G1–G2. With both open, click on a visible region of the panel header (e.g., the run title) — outside the palette content box. | Palette closes only. Panel stays open. (Reconciled wording per d9e4258: defer-otherwise-close keeps the panel open.) |
| G7 | Repeat G1–G2. Click outside both dialogs, on the backdrop. | Per dismissable-layer order, the topmost (palette) closes first; second click would close the panel. Confirm one-click-per-layer behavior. |

## Phase H — End-to-end scenarios (15 minutes)

**Purpose:** Validate that the entire interactivity surface composes correctly through realistic user flows that span multiple ACs.

### H1 — "Find and inspect" flow

1. Open the page. Wait for connection.
2. Press Cmd+K.
3. Type a partial workflow name; verify filtered results.
4. Click the matching Run row.
5. Detail panel opens. Verify metadata grid populated.
6. Press Cmd+K with the panel still open.
7. Type a job name. Click the matching Job row.
8. Verify panel scrolls smoothly to the JobBlock for that job.
9. Press Esc → palette closes, focus on panel close button.
10. Press Esc → panel closes, focus on originating card.

Expected: All six dialogs and focus transitions complete without jank, console errors, or focus loss.

### H2 — "Filter and clear" flow

1. From the palette, click a runner pool row.
2. Confirm pool filter pill appears in the TopBar AND the matching pool indicator highlights AND non-matching runs are hidden.
3. Click a card in a now-filtered column.
4. Detail panel opens for that run. Verify run.id matches a known label-set.
5. Close the panel via Esc.
6. Open palette. Click "Clear pool filter".
7. All filters clear. The complete dataset returns to columns.

Expected: filtering does not break run-card activation, hover-peek, or panel opening on any of the matching runs.

### H3 — "Theme switch under panel" flow

1. Open a run detail panel.
2. Open the palette. Click "Switch theme…".
3. Confirm slide-transition to theme submenu, search anchored.
4. Click a different theme.
5. Theme applies, palette closes, panel remains open and now uses the new theme's hue.
6. All status colors remain constant across the theme change (status-token contract).

Expected: theme submenu, panel re-render, and dialog stacking interactions all coexist without re-mounting the panel or losing scroll position.

### H4 — "Reduced motion" flow

1. Enable `prefers-reduced-motion: reduce` (via OS settings or DevTools rendering emulation).
2. Reload page.
3. Trigger an InProgress card halo: it should be static (no pulse animation).
4. Open palette and switch theme: slide animation is reduced/disabled.
5. Open detail panel: slide-in transition is reduced/disabled.

Expected: All motion-driven UI continues to function; only the animation amplitude/duration is suppressed.

## Visual regression checks

Per Rule 9, mandatory side-by-side comparison against `docs/design-plans/playgrounds/2026-04-25-interactivity-explorer.html`:

| Surface | AC | What to look for |
|---|---|---|
| Theme submenu slide animation | AC1.7 | Slide direction, duration, easing |
| Pool row three-state rendering | AC1.11 | Browse vs query-active vs focused; ch-based right-edge gutter stability |
| Run detail panel layout | AC2.1 | Header/meta-grid/JobBlock spacing; PanelActions placement; close-button hit area |
| Hover-peek popover | AC3.1 | 250ms feel, content row alignment, anchor offset, auto-flip cleanliness |
| RunnerPool active-filter treatment | AC5.2 | 2px accent border, opacity boost amount |
| PoolFilterPill chip | AC5.3 | Pill background, label spacing, clear button hit area |

## Human Verification Required (canonical)

| AC | Why manual | Steps |
|----|-----------|-------|
| AC3.1 (touch-device gating) | Playwright runs on desktop Chromium; the popover suppression on `pointer: coarse` cannot be exercised by the harness. | Phase D, steps D6–D8. |

## AC Traceability

| Acceptance criterion | Automated test (primary) | Manual step |
|---|---|---|
| AC1.1 | palette.test.ts (e2e) + stacking.test.ts | B1, B2 |
| AC1.2 | palette.test.ts AC1.2 | B3 |
| AC1.3 | palette.test.ts AC1.3 + palette.test.ts (unit) | B3, B4 |
| AC1.4 | palette.test.ts AC1.4 | B8 |
| AC1.5 | palette.test.ts AC1.5 | B9 |
| AC1.6 | palette.test.ts AC1.6 + pool-filter.test.ts | B10 |
| AC1.7 | palette.test.ts AC1.7 | B11 (visual review) |
| AC1.8 | palette.test.ts AC1.8 | B13 |
| AC1.9 | palette.test.ts AC1.9 | B12 |
| AC1.10 | palette.test.ts AC1.10 | B5 |
| AC1.11 | PalettePoolItem.browser.test.ts + palette.test.ts AC1.11 | B6, B7 (visual review) |
| AC1.12 | palette.test.ts AC1.12 | B14, B15 |
| AC1.13 | palette.test.ts AC1.13 | B14, B15 |
| AC1.14 | design-tokens.test.ts | A2 (theme/mode toggling spot-check) |
| AC2.1 | RunDetailPanel.test.ts + run-detail-panel.test.ts AC2.1 + leaf tests | C1 (visual review) |
| AC2.2 | PanelActions.test.ts + run-detail-panel.test.ts AC2.2 | C2 |
| AC2.3 | RunDetailPanel.test.ts + stacking.test.ts AC6.3 | C4, G5 |
| AC2.4 | PanelActions.test.ts + run-detail-panel.test.ts AC2.4 | C4 (close button variant) |
| AC2.5 | run-detail-panel.test.ts AC2.5 | C5 |
| AC2.6 | run-detail-panel.test.ts AC2.6 | C3 |
| AC2.7 | JobBlock.browser.test.ts + run-detail-panel.test.ts AC2.7 | C6 |
| AC2.8 | PanelHeader/StepItem/RunDetailPanel/run-detail-panel.test.ts (parameterized) | C7 |
| AC2.9 | RunDetailPanel.test.ts + run-detail-panel.test.ts AC2.9 | spot-check via DevTools `selectedRunId = 99999n` |
| AC3.1 | HoverPeekPopover/RunCard.browser/run-card-interactivity AC3.1 | D1, D3, **D6 (touch — primary human)** |
| AC3.2 | RunCard.browser AC3.2 + run-card-interactivity AC3.2 | D5 |
| AC3.3 | RunCard.browser AC3.3 + run-card-interactivity AC3.3 | D4 |
| AC3.4 | RunCard.browser AC3.4 | D2 |
| AC3.5 | HoverPeekPopover AC3.5 + RunCard.browser AC3.5 | D1 (visual: portal + positioning) |
| AC4.1 | RunCard.test.ts AC4.1+AC4.7 | E6 |
| AC4.2 | RunCard.test.ts + run-card-interactivity AC4.2 | E5 |
| AC4.3 | RunCard.test.ts + run-card-interactivity AC4.3 | E3 |
| AC4.4 | RunCard.test.ts + run-card-interactivity AC4.4 | E4 |
| AC4.5 | run-card-interactivity AC4.5 | E2 |
| AC4.6 | RunCard.test.ts + run-card-interactivity AC4.6 | E5 |
| AC4.7 | RunCard.test.ts AC4.1+AC4.7 | E6, E7 |
| AC5.1 | pool.test.ts + KanbanColumn.test.ts + pool-filter.test.ts | F2 |
| AC5.2 | RunnerPool.test.ts + RunnerBar.test.ts + pool-filter.test.ts | F2 (visual review) |
| AC5.3 | PoolFilterPill.test.ts + pool-filter.test.ts | F2, F3 (visual review) |
| AC5.4 | pool-filter.test.ts AC5.4 | F4 |
| AC5.5 | KanbanColumn.test.ts + RunnerBar.test.ts + pool-filter.test.ts | F1 |
| AC5.6 | KanbanColumn.test.ts + pool-filter.test.ts | F5 |
| AC6.1 | stacking.test.ts | G2 |
| AC6.2 | stacking.test.ts | G4 |
| AC6.3 | stacking.test.ts | G5 |
| AC6.4 | stacking.test.ts AC6.4 | G6 |
| AC6.5 | BackdropSuppression.browser.test.ts + stacking.test.ts | G2 (visual: single overlay) |
| AC6.6 | palette.test.ts (unit) + stacking.test.ts | G3 |

## Notes

- All 47 ACs are covered by automated tests that verify the documented behaviors.
- The test-requirements.md document references three test files that do not exist as named: `frontend/src/lib/components/CommandPalette.test.ts`, `frontend/src/lib/connection.test.ts`, and unit-jsdom variants of the palette leaf tests (which exist as browser-mode `.browser.test.ts` instead). The behaviors described for those files are still covered — these are documentation drift, not coverage gaps.
- Test counts (measured 2026-04-29): 547 unit/browser-mode tests across 64 files (PASS), 79 E2E tests across 10 files (PASS).
