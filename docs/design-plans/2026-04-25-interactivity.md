# Sub-Phase 5: Interactivity (Cmd+K Palette and Detail Panel)

## Summary

<!-- TO BE GENERATED after body is written -->

## Definition of Done

Sub-Phase 5: Interactivity is complete when:

1. **Cmd+K command palette** (vendored shadcn-svelte `Command` on Bits UI) opens via Cmd/Ctrl+K, fuzzy-matches across four sections — Runs, Jobs, Runner Pools, Commands — and selecting an item invokes its action: opening a run or job in the detail panel; filtering the kanban columns by the chosen pool's labels and highlighting that pool indicator in the TopBar; running a Command (theme switcher, mode toggle, density toggle, close panel, focus first run, etc.).
2. **Slide-over detail panel** (vendored shadcn-svelte `Sheet`) opens on RunCard activation, shows state-only deep-dive (full job list, steps, statuses, timestamps, runner — **no log fetching**), and dismisses via Esc, click-outside, or X button with standard Sheet semantics (focus trap on while open, focus restored to the triggering card on close).
3. **Inline preview on RunCard** complements the slide-over; concrete trigger (hover vs. click) and visual treatment for both surfaces are designed during brainstorming via the `impeccable` and `playground` skills before implementation.
4. **RunCard activation via inner button overlay** — the existing `<article>` retains its landmark role; an absolutely-positioned button inside it handles Enter, Space, and click. Tab cycles cards in DOM order using the browser's native focus order.
5. **Cmd+K stacks above an open detail panel** using Bits UI's `defer-otherwise-close` interaction-outside behavior; pressing Esc unwinds the palette first, then the panel.
6. **Per-component tests and Playwright E2E coverage ship in the same PR** as the implementation — no test debt deferred to a polish phase.
7. **Sub-Phase 6 carries forward** roving-tabindex keyboard navigation across cards (arrow keys, Home/End, Tab-leaves-group) and an ARIA live region for run state changes. The leaning preference for the live region is to announce every transition politely; Sub-Phase 6 re-evaluates whether terminal-only reads calmer on a wall display before settling.

**Out of scope for Sub-Phase 5:** roving-tabindex (deferred to Sub-Phase 6), ARIA live region (deferred to Sub-Phase 6), log fetching, virtual scrolling, mobile breakpoints, full reduced-motion audit, persisted selection state.

## Acceptance Criteria

<!-- TO BE GENERATED and validated before glossary -->

## Glossary

<!-- TO BE GENERATED after body is written -->
