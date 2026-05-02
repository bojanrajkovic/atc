# Kanban Keyboard Navigation — Test Plan

**Implementation plan:** `docs/implementation-plans/2026-05-01-kanban-keyboard-nav/`
**Design plan:** `docs/design-plans/2026-05-01-kanban-keyboard-nav.md`
**Test requirements:** `docs/implementation-plans/2026-05-01-kanban-keyboard-nav/test-requirements.md`
**Generated:** 2026-05-01

## Coverage summary

44/44 ACs covered by automated tests; 0 ACs require human-only verification. The eight manual scenarios (A1-A8) are confirmatory smoke checks that are duplicative of automated coverage — no AC is gated exclusively on them.

## AC Traceability

| Acceptance criterion | Automated test (primary) | Manual step |
|---|---|---|
| kanban-keyboard-nav.AC1.1 | `RunCard.test.ts` (`AC1.1 / AC1.3: focused card gets tabindex=0, other card gets tabindex=-1`) + `KanbanColumn.tabindex.browser.test.ts` (`AC1.1: initial render with all three columns populated — exactly one tabindex=0 on first queued card`) | A1 |
| kanban-keyboard-nav.AC1.2 | `run-card-interactivity.test.ts` (`interactivity.AC4.5 Tab from outside kanban lands on the single tabindex=0 card, second Tab exits`) | A1 |
| kanban-keyboard-nav.AC1.3 | `RunCard.test.ts` (`AC1.1 / AC1.3: focused card gets tabindex=0, other card gets tabindex=-1`) | A2 |
| kanban-keyboard-nav.AC1.4 | `KanbanColumn.tabindex.browser.test.ts` (`AC1.4: all three columns empty — no card has tabindex=0, currentFocusRunId===null`) | — |
| kanban-keyboard-nav.AC1.5 | `RunCard.test.ts` (`AC1.5: zero-to-one transition — new first card receives tabindex=0 reactively`) | — |
| kanban-keyboard-nav.AC1.6 | `KanbanColumn.tabindex.browser.test.ts` (`AC1.6: mid-reorder — never two cards simultaneously have tabindex=0`) | — |
| kanban-keyboard-nav.AC1.7 | `RunCard.test.ts` (`AC1.7: no role="grid" or role="gridcell" in rendered DOM`) + `KanbanColumn.tabindex.browser.test.ts` (`AC1.7: no role="grid" or role="gridcell" exists in the kanban subtree`) | — |
| kanban-keyboard-nav.AC2.1 | `geometry.test.ts` (`AC2.1: moves to row+1 in the same column (non-edge)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.1 ArrowDown moves focus to next card in same column`) | A3 |
| kanban-keyboard-nav.AC2.2 | `geometry.test.ts` (`AC2.2: moves to row-1 in the same column (non-edge)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.2 ArrowUp moves focus to previous card in same column`) | A3 |
| kanban-keyboard-nav.AC2.3 | `geometry.test.ts` (`AC2.3: moves to adjacent non-empty column with matching row`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.3 ArrowRight moves focus to corresponding row in next non-empty column`) | A3 |
| kanban-keyboard-nav.AC2.4 | `geometry.test.ts` (`AC2.4: moves to adjacent non-empty column with matching row`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.4 ArrowLeft moves focus to corresponding row in previous non-empty column`) | A3 |
| kanban-keyboard-nav.AC2.5 | `geometry.test.ts` (`AC2.5: moves to row 0 of the same column`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.5 Home moves focus to the first card in the current column`) | A3 |
| kanban-keyboard-nav.AC2.6 | `geometry.test.ts` (`AC2.6: moves to last row of the same column`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.6 End moves focus to the last card in the current column`) | A3 |
| kanban-keyboard-nav.AC2.7 | `action.test.ts` (`calls setFocus and calls preventDefault when ArrowDown resolves to a different card (AC2.7)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC2.7 ArrowDown calls event.preventDefault() — page does not scroll`) | A4 |
| kanban-keyboard-nav.AC3.1 | `geometry.test.ts` (`AC3.1: last row of column is a no-op — returns same id`) + `action.test.ts` (`calls preventDefault but NOT setFocus when ArrowDown is a no-op (last row of column) (AC2.7)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.1 ArrowDown at last card is a no-op`) | A4 |
| kanban-keyboard-nav.AC3.2 | `geometry.test.ts` (`AC3.2: first row (row 0) is a no-op — returns same id`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.2 ArrowUp at first card is a no-op`) | A4 |
| kanban-keyboard-nav.AC3.3 | `geometry.test.ts` (`AC3.3: rightmost non-empty column is a no-op — returns same id`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.3 ArrowRight in rightmost non-empty column is a no-op`) | A4 |
| kanban-keyboard-nav.AC3.4 | `geometry.test.ts` (`AC3.4: leftmost non-empty column is a no-op — returns same id`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.4 ArrowLeft in leftmost non-empty column is a no-op`) | A4 |
| kanban-keyboard-nav.AC3.5 | `geometry.test.ts` (`AC3.5: skips empty middle column when moving right`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.5 ArrowRight skips empty inProgress column and lands in completed`) | A3 |
| kanban-keyboard-nav.AC3.6 | `geometry.test.ts` (`AC3.6: skips empty middle column when moving left`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.6 ArrowLeft skips empty inProgress column symmetrically`) | A3 |
| kanban-keyboard-nav.AC3.7 | `geometry.test.ts` (`AC3.7: ArrowRight clamps row when target column is shorter`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.7 asymmetric clamp: focus in row 5 of 10-card queued, ArrowRight to 3-card inProgress clamps to last row`) | A3 |
| kanban-keyboard-nav.AC3.8 | `geometry.test.ts` (`AC3.8: empty middle col with no further col is a no-op — returns same id`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC3.8 ArrowRight skips multiple empty columns to find furthest non-empty`) | A3 |
| kanban-keyboard-nav.AC4.1 | `action.test.ts` (`returns immediately without preventDefault when metaKey is true (AC4.1)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.1 Cmd+K opens command palette while card focused`) | A5 |
| kanban-keyboard-nav.AC4.2 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.2 Cmd+D toggles dark mode while card focused and does not move kanban focus`) | A5 |
| kanban-keyboard-nav.AC4.3 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.3 Cmd+\\ toggles compact density while card focused`) | A5 |
| kanban-keyboard-nav.AC4.4 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.4 Cmd+ArrowDown returns early — kanban does not move focus`) | A5 |
| kanban-keyboard-nav.AC4.5 | `action.test.ts` (`returns immediately without preventDefault when shiftKey is true (AC4.1)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.5 Shift+ArrowDown returns early — kanban does not move focus`) | A5 |
| kanban-keyboard-nav.AC4.6 | `action.test.ts` (`returns immediately without preventDefault when altKey is true (AC4.1)`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.6 Alt+ArrowDown returns early — kanban does not move focus`) | A5 |
| kanban-keyboard-nav.AC4.7 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC4.7 bare ArrowDown is claimed by kanban and does not open palette`) | — |
| kanban-keyboard-nav.AC5.1 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC5.1 palette open: ArrowDown does not move kanban focus; Esc returns to card`) | A6 |
| kanban-keyboard-nav.AC5.2 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC5.2 panel open: ArrowDown does not move kanban focus`) | A6 |
| kanban-keyboard-nav.AC5.3 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC5.3 both palette and panel stacked: ArrowDown affects neither`) | A6 |
| kanban-keyboard-nav.AC5.4 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC5.4 after panel closes, ArrowDown resumes kanban navigation`) | A6 |
| kanban-keyboard-nav.AC6.1 | `RunCard.cross-column.browser.test.ts` (`AC6.1: in-column reorder preserves focus on same DOM node`) | — |
| kanban-keyboard-nav.AC6.2 | `RunCard.cross-column.browser.test.ts` (`AC6.2: cross-column move lands focus on new DOM node in destination column`) | A7 |
| kanban-keyboard-nav.AC6.3 | `RunCard.cross-column.browser.test.ts` (`AC6.3: old DOM node loses focus after cross-column move`) | — |
| kanban-keyboard-nav.AC6.4 | `RunCard.cross-column.browser.test.ts` (`AC6.4: kanbanHasFocus===false prevents focus migration on cross-column move`) | — |
| kanban-keyboard-nav.AC6.5 | `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC6.5 burst events with held ArrowDown — card-stable across reorder`) | A7 |
| kanban-keyboard-nav.AC7.1 | `RunDetailPanel.browser.test.ts` (`AC7.1 happy path: focus returns to trigger card when it is still present`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC5.4 after panel closes, ArrowDown resumes kanban navigation`) | A8 |
| kanban-keyboard-nav.AC7.2 | `RunDetailPanel.browser.test.ts` (`AC7.2 evicted-source: focus lands on first queued card when trigger card is gone`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC7.2 panel close with evicted trigger card — focus lands on initial card, NOT body`) | A8 |
| kanban-keyboard-nav.AC7.3 | `RovingFocusProvider.browser.test.ts` (`eviction $effect triggers restoreFocusToInitial when focused run is deleted from store`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC7.3 eviction during keyboard nav restores focus to initial card`) | A8 |
| kanban-keyboard-nav.AC7.4 | `RunDetailPanel.browser.test.ts` (`AC7.2 evicted-source: focus lands on first queued card when trigger card is gone`) + `kanban-keyboard-nav.test.ts` (`kanban-keyboard-nav.AC7.3 eviction during keyboard nav restores focus to initial card`) — cross-test invariant: both paths call `ctx.restoreFocusToInitial()` and land on the same `[data-run-id] .run-card-activate` under identical preconditions | — |
| kanban-keyboard-nav.AC7.5 | `RunDetailPanel.browser.test.ts` (`AC7.5 no trigger recorded: no run-card-activate receives focus when lastTriggerRunId is null`) | — |
| kanban-keyboard-nav.AC7.6 | `RovingFocusProvider.browser.test.ts` (`restoreFocusToInitial does not throw and does not focus body when all columns empty`) | — |

## Manual verification scenarios

Each scenario maps to one or more AC families and takes 5-15 minutes. All ACs have automated coverage; these scenarios are confirmatory smoke checks for observable behavior that tests verify programmatically.

### A1 — Initial focus and tabindex

| Step | Action | Expected |
|------|--------|----------|
| A1.1 | Open the app, focus the URL bar, press Tab repeatedly until focus is on a kanban card. | Focus lands on the first card of the first non-empty column. The page does not stop at any other interactive element inside the kanban grid before reaching that card. |
| A1.2 | Open DevTools, inspect the focused button. | Exactly one `.run-card-activate` element has `tabindex="0"`; all others have `tabindex="-1"`. |
| A1.3 | Press Tab once more. | Focus exits the kanban entirely. No second-card or column-2 stop occurs before exiting. |

_Covers: AC1.1, AC1.2._

### A2 — Click moves the rove

| Step | Action | Expected |
|------|--------|----------|
| A2.1 | Click any card that is not the first card. | The clicked card now has `tabindex="0"`; the previously-active card has `tabindex="-1"`. Confirm in DevTools. |

_Covers: AC1.3._

### A3 — 2D arrow nav within and between columns

| Step | Action | Expected |
|------|--------|----------|
| A3.1 | Focus a middle card via click. Press ArrowDown. | Focus moves to the next card in the same column. The page does NOT scroll. |
| A3.2 | Press ArrowUp. | Focus returns to the previous card. |
| A3.3 | Press ArrowLeft (from a column that has a non-empty column to the left). | Focus moves to the corresponding row in the previous non-empty column. Empty columns are skipped. |
| A3.4 | Press ArrowRight (from a column that has a non-empty column to the right). | Focus moves to the corresponding row in the next non-empty column. Empty columns are skipped. |
| A3.5 | Press Home. | Focus jumps to the first card in the current column. |
| A3.6 | Press End. | Focus jumps to the last card in the current column. |
| A3.7 | With asymmetric columns (e.g., Queued has 8 cards, InProgress has 3): focus row 5 of Queued, press ArrowRight. | Focus lands on InProgress's last (3rd) card — not a phantom row-5. The asymmetric clamp applies. |

_Covers: AC2.1–2.6, AC3.5–3.8._

### A4 — Edge no-wrap

| Step | Action | Expected |
|------|--------|----------|
| A4.1 | Focus the last card of a column, press ArrowDown. | Focus does not move. No wrap-around to the first card. |
| A4.2 | Focus the first card of a column, press ArrowUp. | Focus does not move. |
| A4.3 | Focus a card in the rightmost non-empty column, press ArrowRight. | Focus does not move. |
| A4.4 | Focus a card in the leftmost non-empty column, press ArrowLeft. | Focus does not move. |

_Covers: AC2.7 (preventDefault / no-scroll observable at edge), AC3.1–3.4._

### A5 — Modifier delegation

| Step | Action | Expected |
|------|--------|----------|
| A5.1 | With focus on a card, press Cmd+K (Ctrl+K on Linux). | The command palette opens. The card's focus ring disappears (focus moved to palette input). |
| A5.2 | Close the palette, then press Cmd+D. | `<html data-mode>` toggles between `light` and `dark`. Kanban focus does not move. |
| A5.3 | Press Cmd+\\ (backslash). | `<html data-density>` toggles. Kanban focus does not move. |
| A5.4 | Press Cmd+ArrowDown (or Cmd+ArrowUp). | Kanban focus does NOT move. No card selection change. |
| A5.5 | Press Shift+ArrowDown, then Alt+ArrowDown. | Kanban focus does NOT move in either case. |

_Covers: AC4.1–4.7._

### A6 — Suspension while dialogs open

| Step | Action | Expected |
|------|--------|----------|
| A6.1 | Open the command palette via Cmd+K. Press ArrowDown. | Kanban focus does NOT move while the palette is open. The arrow key either moves the palette's internal list cursor or does nothing. |
| A6.2 | Close the palette (Esc). Press ArrowDown. | Kanban focus now moves normally. |
| A6.3 | Open the detail panel by clicking a card. Press ArrowDown. | Kanban focus does NOT move while the panel is open. |
| A6.4 | Close the panel via Esc. Press ArrowDown. | Kanban focus resumes moving. |

_Covers: AC5.1–5.4._

### A7 — Card-stable through reorder bursts

| Step | Action | Expected |
|------|--------|----------|
| A7.1 | With the kanban populated, focus a Queued card that is not the first card. | Focus ring is on the selected card. Note its run title. |
| A7.2 | In DevTools console, send a burst of WS events that reorders the queued column: `window.__stores.runStore.applyRunEvent(...)`. | Focus follows the run identity through the reorder: `document.activeElement` is still the button inside the article with the same `data-run-id`, even if the card moved rows. |
| A7.3 | Send a WS event that transitions the focused card from Queued to InProgress. | After the crossfade animation settles, `document.activeElement` is the corresponding button in the InProgress column (same `data-run-id`). Focus migrated to the new DOM node. |

_Covers: AC6.2, AC6.5._

### A8 — Lost-trigger restoration

| Step | Action | Expected |
|------|--------|----------|
| A8.1 | Focus a card, then click its `.run-card-activate` button to open the detail panel. The detail panel opens. | `lastTriggerRunId` is set internally. |
| A8.2 | In DevTools console: `window.__stores.runStore.runs.delete(BIGINT_ID)` where `BIGINT_ID` is the BigInt ID of the card you opened from (e.g., `1n`). | The card disappears from the column. |
| A8.3 | Close the panel via Esc. Wait for the close animation to complete. | Focus lands on the first card of the first non-empty column — NOT on `<body>`. The bug-fixed behavior. |
| A8.4 | Repeat A8.1 without the eviction step. Open the panel from a card, then close it via Esc. | Focus returns to the trigger card's `.run-card-activate` button (happy path preserved). |

_Covers: AC7.1, AC7.2._

## Notes

- All 44 ACs have automated coverage (unit, browser-mode, or E2E).
- **AC1.2 location note:** `kanban-keyboard-nav.test.ts` header documents that Tab-into-kanban (AC1.2-style) is covered by `run-card-interactivity.test.ts` — specifically `interactivity.AC4.5 Tab from outside kanban lands on the single tabindex=0 card, second Tab exits`. The test-requirements.md entry for AC1.2 cites `kanban-keyboard-nav.test.ts` which is stale; the source code is authoritative.
- **AC7.4 cross-test invariant:** No single test asserts both restoration paths land on an identical DOM node back-to-back. The invariant is structural — both `RunDetailPanel.onCloseAutoFocus` (panel-close path) and `RovingFocusProvider`'s eviction `$effect` (keyboard-nav eviction path) call the same `ctx.restoreFocusToInitial()` function with a single deterministic target (`initialFocusRunId`). Both tests independently assert `[data-run-id="<initial>"] .run-card-activate` under identical preconditions.
- **AC4.1 action.test.ts note:** The action.test.ts tests for `ctrlKey`, `altKey`, and `shiftKey` all carry the `(AC4.1)` label in their test descriptions; these map to AC4.1's modifier guard for the first two and AC4.5/AC4.6 for shift/alt respectively.
- Test counts (measured 2026-05-01): 676 unit/browser-mode tests across all files (PASS), 119 E2E tests across all files (PASS). Per `frontend/CLAUDE.md` Status section.
