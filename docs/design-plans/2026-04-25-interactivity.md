# Sub-Phase 5: Interactivity (Cmd+K Palette and Detail Panel)

## Summary

<!-- TO BE GENERATED after body is written -->

## Definition of Done

Sub-Phase 5: Interactivity is complete when:

1. **Cmd+K command palette** (vendored shadcn-svelte `Command` on Bits UI) opens via Cmd/Ctrl+K, fuzzy-matches across four sections — Runs, Jobs, Runner Pools, Commands — and selecting an item invokes its action: opening a run or job in the detail panel; filtering the kanban columns by the chosen pool's labels and highlighting that pool indicator in the TopBar; running a Command (theme switcher, mode toggle, density toggle, close panel, focus first run, etc.).
2. **Slide-over detail panel** (vendored shadcn-svelte `Sheet`) opens on RunCard activation in a **single-pane layout**: header (status eyebrow + run title), 2-column metadata grid (commit / event / triggered-by / started / duration / runner), then a flat list of job blocks each containing the job header (status icon + name + duration) and its step list with status icon + name + duration per step. State-only deep-dive — **no log fetching**. The header includes a **"Go to run"** external link (next to the close button) that opens `WorkflowRun.htmlUrl` in a new tab via `target="_blank" rel="noopener noreferrer"`. Dismisses via Esc, click-outside, or X button with standard Sheet semantics (focus trap on while open, focus restored to the triggering card on close).
3. **Inline preview on RunCard** uses the **hover peek + click panel** model. After a 250 ms hover debounce, a small popover anchored to the right of the card surfaces quick metadata (status, job count, "N of M steps complete", duration, runner). The popover dismisses immediately on mouse-leave. **Click** the card opens the slide-over panel — the two surfaces coexist as complementary peek (popover) and deep-dive (panel) layers. The hover popover is read-only context that does not interfere with focus or selection state. Visual exploration captured in `docs/design-plans/playgrounds/2026-04-25-interactivity-explorer.html`.
4. **RunCard activation via inner button overlay** — the existing `<article>` retains its landmark role; an absolutely-positioned button inside it handles Enter, Space, and click. Tab cycles cards in DOM order using the browser's native focus order.
5. **Cmd+K stacks above an open detail panel** using Bits UI's `defer-otherwise-close` interaction-outside behavior; pressing Esc unwinds the palette first, then the panel.
6. **Per-component tests and Playwright E2E coverage ship in the same PR** as the implementation — no test debt deferred to a polish phase.
7. **Sub-Phase 6 carries forward** roving-tabindex keyboard navigation across cards (arrow keys, Home/End, Tab-leaves-group) and an ARIA live region for run state changes. The leaning preference for the live region is to announce every transition politely; Sub-Phase 6 re-evaluates whether terminal-only reads calmer on a wall display before settling.

**Out of scope for Sub-Phase 5:**

- Roving-tabindex keyboard navigation — deferred to Sub-Phase 6 (Polish + Responsive).
- ARIA live region for run state changes — deferred to Sub-Phase 6.
- Log fetching for the detail panel — tracked as #36.
- Virtual scrolling / list windowing for the kanban columns — tracked as #37.
- URL-based deep linking for the selected run and open panel (in lieu of localStorage-backed persistence) — tracked as #38.
- Mobile responsive breakpoints — already part of Sub-Phase 6.
- Full `prefers-reduced-motion` audit — already part of Sub-Phase 6 (individual Phase 5 components still respect the media query).

## Acceptance Criteria

<!-- TO BE GENERATED and validated before glossary -->

## Glossary

<!-- TO BE GENERATED after body is written -->
