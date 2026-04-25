# Sub-Phase 5: Interactivity (Cmd+K Palette and Detail Panel)

## Summary

Sub-Phase 5 adds two new interactive surfaces to the ATC dashboard — a Cmd+K command palette and a slide-over detail panel — plus an inline hover-peek popover on RunCard and a kanban pool-filter affordance. All four are frontend-only; no backend changes ship in this phase, and log fetching remains deferred.

The implementation is centered on two vendored shadcn-svelte primitives — `Sheet` (slide-over) and `Command` (palette) — both built on Bits UI's portal and focus-management infrastructure. Both dialogs mount at the `App.svelte` root so they stack correctly when opened simultaneously; dialog stacking is handled entirely through Bits UI's `defer-otherwise-close` escape and outside-interaction props rather than custom event wiring. A new fifth store, `PaletteStore`, owns high-frequency typing state and sessionStorage-persisted recent runs — deliberately separated from `UIStore`'s preference semantics, which revises the codebase's prior "four stores is the ceiling" principle. Pool filtering introduces the project's first branded TypeScript type, `PoolKey`, in a pure module that flows into `KanbanColumn` via props rather than store reads. State coordination between surfaces is entirely store-mediated: palette selections write to `UIStore` fields, and components react to those fields through `$derived`. Implementation ships as six sequential phases on a single branch and PR.

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

### `interactivity.AC1`: Cmd+K command palette

- **AC1.1 Success:** `Cmd/Ctrl+K` toggles `paletteStore.paletteOpen` via `paletteStore.toggle()`. When opening, the dialog renders centered, the search input is focused, and any previously typed query is preserved.
- **AC1.2 Success:** Typing into the palette input filters items via cmdk's command-score fuzzy match; sections with zero matches auto-hide.
- **AC1.3 Success:** Sections render in fixed source order — Recent → Runs → Jobs → Pools → Commands — regardless of match scores. Recent contains up to 10 frecency-tracked runs from `paletteStore.recentRunIds`.
- **AC1.4 Success:** Selecting a Run sets `uiStore.selectedRunId = run.id`, sets `paletteStore.paletteOpen = false`, and records the visit via `paletteStore.recordRunVisit(id)`.
- **AC1.5 Success:** Selecting a Job sets BOTH `uiStore.selectedRunId = job.runId` AND `uiStore.selectedJobId = job.id`, then closes the palette.
- **AC1.6 Success:** Selecting a Pool sets `uiStore.activePoolFilter = poolKey(pool.labels)` and closes the palette.
- **AC1.7 Success:** Selecting "Switch theme…" sets `paletteStore.subMenu = 'theme'`; palette body slide-transitions to a 4-item theme submenu; search input remains anchored.
- **AC1.8 Success:** Selecting a theme from the submenu sets `uiStore.theme` to the chosen value, clears `paletteStore.subMenu`, and closes the palette.
- **AC1.9 Success:** Pressing Esc inside the submenu returns to the top-level palette (clears `paletteStore.subMenu`) without closing the dialog.
- **AC1.10 Success:** A query with zero matches across all sections renders the empty state with the exact copy `Nothing in flight matching "{query}".` using typographic curly quotes.
- **AC1.11 Success:** Pool rows render in three states: browse (single-line truncated), query-active (wrap with `<mark>`-highlighted matched substrings), focused (wrap regardless of query). Right-edge meta column stays at a fixed `ch`-based gutter in all three states.
- **AC1.12 Failure:** "Clear pool filter" command does not appear in the Commands section when `uiStore.activePoolFilter === null`.
- **AC1.13 Failure:** "Close detail panel" command does not appear in the Commands section when `uiStore.selectedRunId === null`.
- **AC1.14 Failure:** Status icons in palette rows clear WCAG AA (≥ 4.5:1) against `--surface-raised` (the hovered/focused row surface) across all four theme hues × dark/light modes; AA failures break the build via the contrast-gate test extension.

### `interactivity.AC2`: Slide-over detail panel

- **AC2.1 Success:** When `uiStore.selectedRunId` becomes non-null and the corresponding `WorkflowRun` exists in `RunStore.runs`, the Sheet opens with the single-pane layout: header → metadata grid → flat list of job blocks.
- **AC2.2 Success:** Header includes a "Go to run" link with `target="_blank"`, `rel="noopener noreferrer"`, and `href` equal to `WorkflowRun.htmlUrl`.
- **AC2.3 Success:** Pressing Esc with panel open and palette closed sets `uiStore.selectedRunId = null`; sheet closes; focus returns to the triggering RunCard's inner button via `onCloseAutoFocus`.
- **AC2.4 Success:** Clicking the X button has the same effect as Esc (state mutation + focus restoration).
- **AC2.5 Success:** Clicking outside the sheet (with no nested dialog open) closes it.
- **AC2.6 Success:** While open, focus is trapped inside the sheet — Tab cycles only the sheet's focusable elements.
- **AC2.7 Success:** Setting `uiStore.selectedJobId` triggers `JobBlock.scrollIntoView({ block: 'start', behavior: 'smooth' })` on the matching block; `selectedJobId` is cleared after the scroll dispatches.
- **AC2.8 Success:** All 11 RunStatus fixtures (Queued, InProgress, Completed, Failed, TimedOut, Cancelled, ActionRequired, StartupFailure, Stale, Neutral, Skipped) render with the correct status icon glyph and `--status-color` in both panel header and step list.
- **AC2.9 Failure:** When `selectedRunId` references a run not present in `RunStore.runs`, the panel does not open and `selectedRunId` is cleared (graceful fallback rather than a broken empty panel).

### `interactivity.AC3`: Inline hover-peek popover

- **AC3.1 Success:** Hovering a RunCard for 250 ms triggers `HoverPeekPopover` anchored to the right edge of the card; popover shows status, job count, "N of M steps complete", duration, and runner.
- **AC3.2 Success:** Mouse-leave on the card immediately clears the popover (no fade-out delay).
- **AC3.3 Success:** Click on a hovered card opens the slide-over panel; popover dismisses synchronously.
- **AC3.4 Failure:** Hover for less than 250 ms (mouse moves out before the debounce fires) does NOT show the popover.

### `interactivity.AC4`: RunCard activation

- **AC4.1 Success:** RunCard's `<article>` retains `role="article"`. An inner `<button class="run-card-activate">` with `aria-label` describing the run (title + status + repo·branch) handles activation.
- **AC4.2 Success:** Click on the inner button (or any of its non-interactive descendants via event bubbling) sets `uiStore.selectedRunId = run.id`.
- **AC4.3 Success:** Enter on the focused inner button sets `uiStore.selectedRunId`.
- **AC4.4 Success:** Space on the focused inner button sets `uiStore.selectedRunId`.
- **AC4.5 Success:** Tab cycles all RunCard inner buttons in DOM order (column → column → column, runs in column order); the article is not in the tab order.
- **AC4.6 Failure:** Pointer events on text inside the article (e.g., the run title text) do not break activation — clicks still bubble to the inner button.

### `interactivity.AC5`: Pool filter integration

- **AC5.1 Success:** When `uiStore.activePoolFilter !== null`, all three kanban columns filter to runs whose jobs include all the pool's labels (intersection check via `filterRunsByPool`).
- **AC5.2 Success:** When `activePoolFilter !== null`, the matching `RunnerPool` in TopBar (where `poolKey(pool.labels) === activePoolFilter`) renders with `isActiveFilter={true}` (2px `--accent` border + opacity boost).
- **AC5.3 Success:** When `activePoolFilter !== null`, KanbanBoard renders a `PoolFilterPill` with text "Filtering by [labels] · ✕"; clicking ✕ sets `activePoolFilter = null`.
- **AC5.4 Success:** "Clear pool filter" command in palette sets `activePoolFilter = null` and closes the palette.
- **AC5.5 Failure:** When `activePoolFilter === null`, no `PoolFilterPill` renders; no TopBar pool has `isActiveFilter={true}`; columns show all runs unfiltered.
- **AC5.6 Edge:** A pool filter referencing labels that no current job includes results in all three kanban columns rendering empty (no error; no flicker).

### `interactivity.AC6`: Cmd+K stacks above panel

- **AC6.1 Success:** With panel open (`selectedRunId !== null`), pressing Cmd+K opens the palette ON TOP of the panel; both dialogs are visible simultaneously; palette has focus.
- **AC6.2 Success:** Pressing Esc with both open closes the palette only; the panel remains open; focus returns to the panel's close button via `onCloseAutoFocus`.
- **AC6.3 Success:** Pressing Esc again closes the panel; focus returns to the triggering RunCard's inner button.
- **AC6.4 Success:** Clicking outside the palette but inside the panel area closes only the palette.
- **AC6.5 Success:** Only one backdrop overlay element is rendered to the DOM when both dialogs are open (`[data-nested] [data-overlay] { display: none }` suppresses the inner one).
- **AC6.6 Success:** Cmd+K while palette is already open closes it (toggle behavior); dialog state mutates correctly.

## Glossary

- **Bits UI**: A headless Svelte component library providing low-level primitives (Dialog, Popover, Tooltip, etc.) with built-in portal rendering, focus trapping, and ARIA attribute management. shadcn-svelte's Sheet and Command are built on top of it.
- **shadcn-svelte**: A component collection strategy where UI component source is vendored directly into the project under `frontend/src/lib/components/ui/` rather than consumed as an npm package dependency, making each component fully customizable.
- **cmdk / command-score**: The fuzzy-matching engine that powers the command palette's search. `command-score` is its scoring algorithm — items are ranked by weighted substring match; sections with zero-scoring items auto-hide.
- **frecency**: A hybrid ranking signal combining recency (how recently) and frequency (how often) of access. Used in the palette's Recent section to order the up-to-10 recent run IDs stored in `PaletteStore.recentRunIds`.
- **defer-otherwise-close**: A Bits UI interaction-outside/escape-keydown behavior value. When set on a nested dialog, it defers the event to any enclosing dialog first (e.g., Esc closes the palette before the panel), and only closes the dialog if no outer handler consumed it.
- **`onCloseAutoFocus`**: A Bits UI Dialog/Sheet callback that fires when a dialog closes and determines where focus is restored. This phase uses it to return focus to the triggering RunCard's inner button after the panel closes, or to the panel's close button after the palette closes.
- **branded type**: A TypeScript compile-time pattern that makes a structural alias (like `string`) nominally distinct by intersecting it with a phantom property — e.g., `type PoolKey = string & { readonly __brand: 'PoolKey' }`. Assignment of a plain `string` to `PoolKey` is a type error without calling the constructor function.
- **`@ts-expect-error` brand assertion**: A test technique that places a deliberate type-incorrect assignment under a `@ts-expect-error` directive, then verifies the directive is actually needed (TypeScript errors if it is not). Used in `pool.test.ts` to prove the `PoolKey` brand is enforced by the compiler.
- **PoolKey**: The project's first branded TypeScript type — a `string` carrying a `__brand` phantom property. It represents the canonical identifier for a runner pool, derived by sorting and joining a pool's label array. A frontend-only type; ts-rs-generated IDs remain plain `bigint` aliases.
- **ts-rs**: A Rust library that generates TypeScript type definitions from Rust structs at build time (via `just types`). Domain types like `RunId`, `JobId`, and `WorkflowRun` are generated this way. `PoolKey` is out of ts-rs scope because it is a frontend-only derived identifier.
- **OKLCH**: A perceptually uniform CSS color space (`oklch(L C H / alpha)`) used throughout the ATC design system. Its hue axis allows the four palette themes (Warm, Radar, Violet, Pink) to be expressed as a single `--hue` token that drives all derived color tokens.
- **sessionStorage-persisted**: State that is saved to the browser's `sessionStorage` (tab-scoped, cleared when the tab closes) rather than `localStorage` (persistent across sessions). `PaletteStore.recentRunIds` is sessionStorage-persisted under the key `atc.palette.recent`.
- **slide-over panel**: A UI pattern where a content panel slides in from the side of the screen, overlaying the main content with a backdrop. Implemented here as a shadcn-svelte `Sheet`, it displays the single-pane RunDetailPanel without navigating away from the kanban board.
- **hover peek (popover)**: A lightweight read-only popover that appears after a debounce delay when the user hovers a RunCard, surfacing quick metadata without opening the full detail panel. Complements rather than replaces the click-to-open panel.
- **single-pane layout**: The RunDetailPanel's presentation structure — one scrollable column containing header, metadata grid, and job blocks — as opposed to a master-detail or tabbed layout. All run and job content is visible without switching views.
- **three-state pool row**: The `PalettePoolItem` rendering contract — a pool row in the palette takes one of three visual forms: browse (single-line, truncated), query-active (wraps text, highlights matched substrings with `<mark>`), focused (wraps text regardless of query presence).
- **`<mark>` highlight**: The HTML `<mark>` element used to visually highlight fuzzy-matched substrings within palette pool rows and run names. Styled via the `--mark-bg` / `--mark-underline` design tokens added in this phase.
- **jobsByRunId**: A new `$derived` field on `RunStore` — a `ReadonlyMap<RunId, Job[]>` aggregating each run's jobs by run ID. Required for pool filtering, which needs the raw job label arrays rather than the summarized runner strings that `jobStatsByRun` provides.
- **kanban column**: The vertical layout unit in the KanbanBoard showing runs grouped by lifecycle state (Queued / In Progress / Completed). In this phase, `KanbanColumn` gains `activePoolFilter` and `jobsByRunId` props that determine which run cards are rendered.

## Architecture

The two visible surfaces of Sub-Phase 5 are the slide-over **`RunDetailPanel`** (a shadcn-svelte `<Sheet>` that shows read-only state for one run) and the **`CommandPalette`** (a shadcn-svelte `<Command.Dialog>` with sections for Recent runs, Runs, Jobs, Pools, and Commands). A third surface — the **inline hover-peek popover** — is local to `RunCard` and never lives standalone. Both Sheet and Command mount via Bits UI portals at the `App.svelte` root so they stack correctly when both are open simultaneously, with a single global Cmd+K listener at the same level toggling palette state.

State is owned by a 5-store layout — the existing 4 (`RunStore`, `RunnerStore`, `ConnectionStore`, `UIStore`) keep their roles, and a new `PaletteStore` is introduced for palette-specific state with high-frequency mutation and its own recent-items lifecycle. `UIStore` gains two new fields, `activePoolFilter` (session-only `PoolKey | null`) and `selectedJobId` (transient `JobId | null` cleared after the panel scrolls). `RunStore` exposes a new `jobsByRunId: $derived<ReadonlyMap<RunId, Job[]>>` so consumers can access raw job labels — today's `jobStatsByRun` only carries summarized runner strings, not the structured labels needed for pool filtering.

Pool filtering is implemented as a pure module at `frontend/src/lib/filters/pool.ts` with the project's first branded TypeScript type, `PoolKey`. `KanbanBoard` reads `uiStore.activePoolFilter` and `runStore.jobsByRunId` once and threads them as props to each `KanbanColumn`; each column invokes `filterRunsByPool` internally on its own `runs` prop. This keeps `RunStore` pure of UI concerns and keeps `KanbanColumn` pure of store reads — the filter flows in as data, not as a store dependency.

Sheet + Command stacking is configured via Bits UI Dialog props: `escapeKeydownBehavior: 'defer-otherwise-close'` and `interactOutsideBehavior: 'defer-otherwise-close'` on both dialogs. The palette's `onCloseAutoFocus` callback returns focus to the panel's close button when both were open; the panel's `onCloseAutoFocus` returns focus to the triggering RunCard's inner button. A CSS rule `[data-nested] [data-overlay] { display: none }` suppresses the inner backdrop so only one dimming layer is visible when both dialogs are open.

The contract between the palette and the rest of the dashboard is small and store-mediated. Selecting a Run sets `uiStore.selectedRunId`. Selecting a Job sets both `selectedRunId` and `selectedJobId`. Selecting a Pool sets `uiStore.activePoolFilter`. Running a Command invokes the corresponding mutator. No component-to-component event wiring; consumers simply react to store changes via `$derived`.

```typescript
// frontend/src/lib/filters/pool.ts — contract for the pool-filter module

export type PoolKey = string & { readonly __brand: 'PoolKey' };

export function poolKey(labels: readonly string[]): PoolKey;
export function jobMatchesPool(jobLabels: readonly string[], poolLabels: readonly string[]): boolean;
export function filterRunsByPool(
  runs: readonly WorkflowRun[],
  jobsByRunId: ReadonlyMap<bigint, readonly Job[]>,
  poolFilter: PoolKey | null,
): readonly WorkflowRun[];
```

```typescript
// frontend/src/lib/stores/palette.svelte.ts — PaletteStore contract

export class PaletteStore {
  paletteOpen: boolean;          // $state
  paletteQuery: string;          // $state
  recentRunIds: bigint[];        // $state, LRU cap 10, sessionStorage-persisted
  subMenu: 'theme' | null;       // $state

  open(): void;
  close(): void;
  toggle(): void;
  setQuery(q: string): void;
  recordRunVisit(id: bigint): void;
  enterSubmenu(name: 'theme'): void;
  exitSubmenu(): void;
}
```

```typescript
// frontend/src/lib/stores/ui.svelte.ts — UIStore additions

class UIStore {
  // existing fields unchanged
  selectedRunId: bigint | null;
  theme: 'warm' | 'radar' | 'violet' | 'pink';
  mode: 'dark' | 'light';
  density: 'comfortable' | 'compact';
  nowMs: number;

  // NEW
  activePoolFilter: PoolKey | null;   // session-only, no localStorage
  selectedJobId: bigint | null;       // transient, cleared after scroll
}
```

System boundaries: all new code lives within `frontend/`. No backend changes. No new GitHub API endpoints (logs deferred to issue #36). ts-rs-generated TypeScript types are unchanged; `PoolKey` is a frontend-only branded type.

## Existing Patterns

The design follows established codebase patterns:

- **4-store rune-class architecture** under `frontend/src/lib/stores/` — `RunStore`, `RunnerStore`, `ConnectionStore`, `UIStore`. The new `PaletteStore` follows the same pattern: a class with `$state` fields and explicit mutator methods.
- **shadcn-svelte vendoring** under `frontend/src/lib/components/ui/` — Phase 1 vendors Sheet + Command via the same `pnpm dlx shadcn-svelte@latest add <name>` invocation that produced the existing Popover, Tooltip, Card, Progress, Separator, Toggle, ToggleGroup, and Badge components.
- **Pure-leaf decomposition** established in Sub-Phase 4 (`StatusIcon`, `JobHeader`, `JobMeta`, `ProgressBar`, `RunnerLabel`). New leaves under `RunDetailPanel` and `CommandPalette` follow the same shape: exported `interface Props`, props in / DOM out, no store reads.
- **`.svelte` + `.test.ts` per-component sibling tests** — every new component gets a sibling test file. Established in Sub-Phases 2–4.
- **Vitest projects split** (`vitest.config.unit.ts` for jsdom, `vitest.config.browser.ts` for Playwright Chromium) — existing pattern; new tests use the appropriate project per concern.
- **`window.__stores` E2E bridge** at `e2e/lib/ws-mock.ts` — Sub-Phase 3 introduced `makeRunEvent` / `makeJobSeqEvent` / `sendWS` to drive store state from Playwright. New E2E tests use the same harness; tests for palette / panel / pool filter set store state via the bridge rather than relying on UI navigation chains across phases.
- **`--status-color` inline custom property + `data-status` PascalCase attribute** on RunCard — Sub-Phase 4 pattern. The inner button overlay added in Phase 4 inherits `--status-color` from the article root.
- **OKLCH design tokens** in `frontend/src/app.css` — new tokens (`--mark-bg`, `--mark-underline`, `--kbd-bg`, `--kbd-border`, `--text-quiet`) follow the existing `--hue`-derived neutral ramp + fixed-hue status color convention. Light-mode variants live in the existing `[data-mode="light"]` block.
- **Conventional Commits + lefthook three-tier hooks** — Phase commits follow `feat(frontend):` / `test(frontend):` / `docs(design):` prefixes per `.commitlintrc.mjs`.

The design diverges from existing patterns in three documented ways:

1. **Store ceiling principle revision.** The README's principle "Four stores is the ceiling — if you feel the need for a fifth, you're probably over-granularizing" is revised to "Five stores is the ceiling, with rationale: PaletteStore separates high-frequency typing state and recent-items lifecycle from UIStore's preference-state semantics." Trying to shoehorn palette state into UIStore would be worse for net complexity (mixed concerns: theme/density preferences vs. ephemeral typing state vs. session-scoped recent runs). The principle update is captured in the **Documents to Update** table below.
2. **First branded TypeScript type.** ts-rs-generated IDs (`RunId`, `JobId`, `StepId`) remain plain `bigint` aliases — branding them would require ts-rs configuration or post-processing and is out of scope. `PoolKey` is the first frontend-only branded type and sets a precedent for future TS-only domain types. An optional ADR captures this decision (see Documents to Update).
3. **First hover-driven affordance.** The hover-peek popover is the first hover-only UX in the codebase. Mitigation: full keyboard equivalence is preserved (Enter on the focused inner button opens the panel directly, no peek required). Mouse users get an extra affordance; keyboard users lose nothing. Touch-screen users get the same treatment as keyboard users — first tap on the activator button opens the panel directly, with no intermediate hover state.

## Implementation Phases

Sub-Phase 5 implements as six sequential phases on the `feat/interactivity` branch, all merged in one PR. Each phase ends with a green local test run and a commit; later phases depend on earlier phases as documented below.

<!-- START_PHASE_1 -->
### Phase 1: Infrastructure

**Goal:** Vendor Sheet + Command, set up new stores, expose raw job labels from `RunStore`, and ship the pure pool-filter module with its branded type. No user-visible changes — purely the data and primitive layer for everything that follows.

**Components:**

- `frontend/src/lib/components/ui/sheet/` — vendored via `pnpm dlx shadcn-svelte@latest add sheet`
- `frontend/src/lib/components/ui/command/` — vendored via `pnpm dlx shadcn-svelte@latest add command`
- `frontend/src/lib/stores/palette.svelte.ts` — new `PaletteStore` class with `paletteOpen`, `paletteQuery`, `recentRunIds`, `subMenu` fields and mutators (`open`, `close`, `toggle`, `setQuery`, `recordRunVisit`, `enterSubmenu`, `exitSubmenu`); `recentRunIds` LRU cap 10, sessionStorage-persisted under key `atc.palette.recent`
- `frontend/src/lib/stores/palette.test.ts` — PaletteStore mutator tests with sessionStorage stub
- `frontend/src/lib/stores/ui.svelte.ts` — adds `activePoolFilter: PoolKey | null` and `selectedJobId: bigint | null`; both default to `null`; neither persisted to localStorage
- `frontend/src/lib/stores/runs.svelte.ts` — adds `jobsByRunId: $derived<ReadonlyMap<bigint, Job[]>>` aggregating jobs from current state into per-run buckets
- `frontend/src/lib/filters/pool.ts` — pure module with branded `PoolKey` type and `poolKey`, `jobMatchesPool`, `filterRunsByPool` functions
- `frontend/src/lib/filters/pool.test.ts` — behavior tests (intersection, sort/join roundtrip, null-filter passthrough) AND a `@ts-expect-error` block proving raw `string` cannot be assigned to `PoolKey`
- Updates to existing tests for new store fields

**Dependencies:** None (first phase).

**Done when:** `pnpm test` passes including the new test files; `just types` produces no diff (no Rust types changed); the `@ts-expect-error` block correctly fails type-checking when removed (proving the brand is enforced); both vendored shadcn components are committed under `frontend/src/lib/components/ui/`. No ACs map to this phase — verification is operational.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: CommandPalette

**Goal:** Render the Cmd+K palette as a shadcn-svelte `Command.Dialog` with the five sections (Recent / Runs / Jobs / Pools / Commands), fuzzy match via cmdk's command-score weighting, the theme submenu, and the v1 commands list with conditional visibility.

**Components:**

- `frontend/src/lib/components/CommandPalette.svelte` — connected: reads `paletteStore` + `runStore` + `runnerStore`, dispatches to `paletteStore` and `uiStore`. Renders `<Command.Dialog>` bound to `paletteStore.paletteOpen`. Section order: Recent → Runs → Jobs → Pools → Commands (declared in source order so cmdk renders them in that sequence regardless of match score)
- `frontend/src/lib/components/PaletteSection.svelte` — pure: wraps a labeled `<Command.Group>`
- `frontend/src/lib/components/PaletteRunItem.svelte` — pure: status icon + run title + repo·branch meta
- `frontend/src/lib/components/PaletteJobItem.svelte` — pure: smaller status icon + job name + parent run reference suffix
- `frontend/src/lib/components/PalettePoolItem.svelte` — pure: ⊞ icon + plain-text labels (dot-separated) + N running·M queued meta. Implements three-state behavior: browse (truncate), query-active (wrap with `<mark>` highlights), focused (wrap regardless)
- `frontend/src/lib/components/PaletteCommandItem.svelte` — pure: command icon + label + optional `<kbd>` shortcut chips
- New tokens added to `frontend/src/app.css`: `--text-quiet`, `--kbd-bg`, `--kbd-border`, `--mark-bg`, `--mark-underline` (with light-mode variants). Tokens follow the existing `--hue`-derived convention
- v1 Commands list: Switch theme… (submenu trigger), Toggle dark mode (`⌘D`), Toggle compact density (`⌘\\`), Clear pool filter (conditional on `activePoolFilter !== null`), Close detail panel (conditional on `selectedRunId !== null`, shortcut `Esc`), Reconnect
- Theme submenu: when `paletteStore.subMenu === 'theme'`, palette body content slide-transitions to a 4-item theme list (Warm / Radar / Violet / Pink); search input stays anchored
- Per-component test files (5 jsdom + 1 browser-mode for the pool 3-state computed styles)
- `frontend/src/lib/components/CommandPalette.test.ts` — connected component test with mocked stores
- `frontend/e2e/palette.test.ts` — Playwright E2E covering open via `paletteStore.paletteOpen = true`, type filtering, section ordering, empty state copy, pool 3-state behavior, Esc closes, theme submenu navigation, run/job/pool/command selection mutates the correct stores

**Dependencies:** Phase 1 (PaletteStore + Command primitive must exist).

**Done when:** Tests pass for `interactivity.AC1.*` (palette ACs except stacking which lands in Phase 6); per-component tests pass; E2E test file ships green.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: RunDetailPanel

**Goal:** Render the slide-over panel as a shadcn-svelte `Sheet` with the single-pane layout: header (status eyebrow + title + actions), 2-column metadata grid, flat list of job blocks each containing a step list with a timeline gutter.

**Components:**

- `frontend/src/lib/components/RunDetailPanel.svelte` — connected: reads `uiStore.selectedRunId` and `selectedJobId`, `runStore.runs.get(id)`, `runStore.jobsByRunId.get(id)`. Renders `<Sheet>` bound to `selectedRunId !== null` for `bind:open`
- `frontend/src/lib/components/PanelHeader.svelte` — pure: status eyebrow (status name + dot in `--status-color`) + run title
- `frontend/src/lib/components/PanelActions.svelte` — pure: "Go to run" external link + close button row. Link has `target="_blank"`, `rel="noopener noreferrer"`, `href={WorkflowRun.htmlUrl}`
- `frontend/src/lib/components/MetaGrid.svelte` — pure: 2-column key-value grid (commit / event / triggered-by / started / duration / runner)
- `frontend/src/lib/components/MetaCell.svelte` — pure: label + value pair
- `frontend/src/lib/components/JobBlock.svelte` — pure: job header (status icon + name + duration) + step list. Element id `job-${job.id}` matches `selectedJobId` for `scrollIntoView`. Includes `$effect` that triggers scroll when `selectedJobId === job.id`, then dispatches a clear via callback prop
- `frontend/src/lib/components/StepList.svelte` — pure: `<ol>` container with timeline gutter via `::before` pseudo-element
- `frontend/src/lib/components/StepItem.svelte` — pure: status icon + name + duration row
- `App.svelte` — modified: mounts `<RunDetailPanel />` at root level (alongside existing `<KanbanBoard />`)
- Per-component test files (7 leaves jsdom; 1 browser-mode for scroll-into-view behavior)
- `frontend/src/lib/components/RunDetailPanel.test.ts` — connected component test with mocked stores
- `frontend/e2e/run-detail-panel.test.ts` — E2E covering panel-opens-when-selectedRunId-set, X close, Esc close, focus trap, focus restoration to RunCard's button, "Go to run" anchor attributes (verified via DOM inspection — link is `target="_blank"` with correct `rel`), `selectedJobId` scroll behavior with all 11 RunStatus fixtures

**Dependencies:** Phase 1 (UIStore additions, `RunStore.jobsByRunId`, vendored Sheet).

**Done when:** Tests pass for `interactivity.AC2.*` (panel ACs); per-component tests pass; E2E ships green.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: RunCard interactivity

**Goal:** Make `RunCard` activatable via mouse + keyboard, and add the hover-peek popover that complements the slide-over panel.

**Components:**

- `frontend/src/lib/components/RunCard.svelte` — modified: add absolutely-positioned inner `<button class="run-card-activate">` with `aria-label` derived from run title + status + repo·branch. Article retains `role="article"`. Button click + `keydown` (Enter/Space) handlers set `uiStore.selectedRunId = run.id`. Hover state managed via local `$state` timer (250 ms debounce); mouse-leave clears immediately
- `frontend/src/lib/components/HoverPeekPopover.svelte` — pure: receives `WorkflowRun` and `JobStats` props. Renders peek content (status, job count, "N of M steps complete", duration, runner). Anchored to the right of the card via `position: absolute` relative to RunCard's positioning context; uses Bits UI Tooltip primitive for portal + positioning if a single-direction anchor is insufficient — otherwise inline absolute positioning suffices
- `frontend/src/lib/components/RunCard.test.ts` — modified: add tests for inner-button rendering with correct `aria-label`; click + keyboard activation; article role preservation; Tab order (button is focus target, not article)
- `frontend/src/lib/components/RunCard.browser.test.ts` — modified or new: hover-debounce timing using fake timers; mouse-leave clears popover; click on a child element (e.g., status icon) bubbles to the button correctly
- `frontend/src/lib/components/HoverPeekPopover.test.ts` — pure leaf tests
- `frontend/e2e/run-card-interactivity.test.ts` — E2E covering click → panel opens; Enter on focused button → panel opens; Space activates; Tab cycles cards in DOM order; hover after 250 ms shows popover; mouse-leave clears immediately

**Dependencies:** Phase 3 (RunDetailPanel must exist for click activation to be testable end-to-end).

**Done when:** Tests pass for `interactivity.AC3.*` (inline preview) and `interactivity.AC4.*` (RunCard activation); per-component tests pass; E2E ships green.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Pool filter integration

**Goal:** Wire palette pool selection through to filtered kanban columns + a TopBar pool-indicator highlight + a clear-filter affordance.

**Components:**

- `frontend/src/lib/components/KanbanBoard.svelte` — modified: read `uiStore.activePoolFilter` and `runStore.jobsByRunId`; thread both as new props (`activePoolFilter`, `jobsByRunId`) to each `KanbanColumn`. Conditionally render `<PoolFilterPill>` when `activePoolFilter !== null`
- `frontend/src/lib/components/KanbanColumn.svelte` — modified: receive `activePoolFilter` + `jobsByRunId` props; call `filterRunsByPool(this.runs, jobsByRunId, activePoolFilter)` internally before rendering. Pure (props in / DOM out) is preserved — the filter flows in as data
- `frontend/src/lib/components/RunnerPool.svelte` — modified: accept new `isActiveFilter: boolean` prop. When `true`, render with 2px `--accent` border + slight opacity boost on the indicator surface
- `frontend/src/lib/components/TopBar.svelte` — modified: derive `isActiveFilter` per pool by computing `poolKey(pool.labels) === activePoolFilter`; thread as prop to `<RunnerPool>`
- `frontend/src/lib/components/PoolFilterPill.svelte` — new pure: small "Filtering by [labels] · ✕" pill in the kanban-board header area. Click on ✕ dispatches `uiStore.activePoolFilter = null`
- Tests for the new prop threading; tests for `PoolFilterPill` rendering and click; tests for `RunnerPool.isActiveFilter` styling
- `frontend/e2e/pool-filter.test.ts` — E2E covering: select pool from palette → all three columns filter; TopBar pool gets accent border; PoolFilterPill renders; clicking ✕ clears filter; "Clear pool filter" command in palette also clears it; filter referencing labels with no matching jobs results in empty columns (graceful fallback)

**Dependencies:** Phase 1 (filter module + UIStore field) and Phase 2 (palette must dispatch pool selection).

**Done when:** Tests pass for `interactivity.AC5.*` (pool filter); per-component tests pass; E2E ships green.
<!-- END_PHASE_5 -->

<!-- START_PHASE_6 -->
### Phase 6: Sheet + Command stacking

**Goal:** Configure nested-dialog focus management, the global Cmd+K listener, and the backdrop-suppression CSS so palette and panel coexist correctly.

**Components:**

- `frontend/src/lib/components/CommandPalette.svelte` — modified: configure `escapeKeydownBehavior: 'defer-otherwise-close'` and `interactOutsideBehavior: 'defer-otherwise-close'` on the Command dialog. Set `onCloseAutoFocus` callback that returns focus to the panel's close button when `uiStore.selectedRunId !== null` at close time, otherwise restores to body default
- `frontend/src/lib/components/RunDetailPanel.svelte` — modified: same dismissal behavior props on the Sheet. `onCloseAutoFocus` returns focus to the triggering RunCard's inner button using a stored reference captured at open time
- `frontend/src/app.css` — add `[data-nested] [data-overlay] { display: none }` rule to suppress double-darkening when both dialogs are open
- `frontend/src/App.svelte` — modified: add a single `keydown` listener on mount that fires `paletteStore.toggle()` on `(e.metaKey || e.ctrlKey) && e.key === 'k'`. Removed on destroy. No separate Esc handler — Bits UI's `escapeKeydownBehavior` on the dialogs handles dismissal
- Browser-mode tests: nested focus traps don't conflict, focus restoration order, only one backdrop element rendered when both dialogs open
- `frontend/e2e/stacking.test.ts` — E2E for stacking-specific scenarios only: open panel via test harness → open palette via Cmd+K → both visible → Esc closes palette only → focus on panel close button → second Esc closes panel → focus on triggering RunCard's button. Click outside palette but inside panel area closes palette only

**Dependencies:** Phases 2 + 3 (both dialogs must exist).

**Done when:** Tests pass for `interactivity.AC6.*` (stacking ACs); browser-mode tests confirm focus restoration order; single backdrop element rendered when both dialogs are open; E2E ships green.
<!-- END_PHASE_6 -->

## Documents to Update

Per `.ed3d/design-plan-guidance.md` rule 6, every design plan must list the documents that change alongside implementation:

| Document | What changes |
|----------|--------------|
| `docs/architecture/frontend-app.md` | Component tree update (CommandPalette, RunDetailPanel, all pure leaves, HoverPeekPopover, PoolFilterPill); document new stores (PaletteStore + UIStore additions); document Sheet+Command stacking pattern with `defer-otherwise-close` semantics; **revise store-ceiling principle from 4 to 5 with rationale**; bump "Last verified" date |
| `frontend/CLAUDE.md` | Add new components to Key Files table; PaletteStore to store list; `lib/filters/pool.ts` module reference noting first branded type in project; new design tokens (`--mark-bg`, `--mark-underline`, `--kbd-bg`, `--kbd-border`, `--text-quiet`); status section update on Sub-Phase 5 completion |
| `docs/ideation/ui-decomposition/README.md` | After merge: mark Sub-Phase 5 ✅ COMPLETE with PR# and "What was built:" section in established pattern; note the store-ceiling principle revision |
| `.impeccable.md` | New tokens for `<mark>` highlight bg (`--mark-bg`) plus underline color, palette focus ring color, hover popover surface elevation. Document the contrast extension to `--surface-raised` for palette rows |
| `docs/test-plans/2026-04-25-interactivity.md` | New file with full AC traceability matrix (Sub-Phase 4 pattern). Posted as first PR comment per project convention; never committed inside the PR description |
| `scripts/doc-mapping.sh` | New source paths → architecture doc mappings: `frontend/src/lib/stores/palette.svelte.ts`, `frontend/src/lib/components/CommandPalette.svelte`, `frontend/src/lib/components/RunDetailPanel.svelte`, `frontend/src/lib/filters/pool.ts` all map to `docs/architecture/frontend-app.md` |
| `docs/architecture-decisions/00NN-first-branded-type.md` | New ADR documenting the rationale for `PoolKey` as the first branded TypeScript type (and why ts-rs IDs remain plain `bigint` aliases for now). Sets precedent for future TS-only domain types |

## Additional Considerations

**Contrast gate extension.** The existing `frontend/src/lib/design-tokens.test.ts` enforces WCAG AA contrast for status tokens against `--surface` only. Palette rows render on `--surface-raised` when hovered or focused, which is brighter in dark mode and lighter in light mode. The build-gate test should extend to cover status icons against `--surface-raised` so AA is enforced in palette context too. AAA failures remain informational. The extension is part of Phase 2's deliverables (the phase that introduces `--surface-raised` palette usage).

**Sub-Phase 6 deferral notes.** The design plan body and the ideation README's Sub-Phase 6 section both carry forward two items that the Definition of Done requires:

- **Roving-tabindex implementation strategy.** Bits UI provides no `RovingFocusGroup` primitive (verified 2026-04). Library survey: `svelte-roving-ux` is Svelte-4-era and unmaintained for runes; `jakelazaroff/roving-tabindex` is a framework-agnostic web component but adds Vite/Tailwind v4 integration risk. Default plan in Sub-Phase 6: a custom Svelte 5 context provider with explicit `tabindex` swap on a single index signal, plus one key handler attached at the kanban-board root. Suspend while detail panel is open; restore focus to triggering card on panel close.
- **ARIA live region leaning preference.** Lean toward announcing every transition (Queued → InProgress → Completed, plus terminal Failed/TimedOut/Cancelled) via `aria-live="polite"`. The dashboard is intended for wall display, so frequency matters; re-evaluate during Sub-Phase 6 whether terminal-only reads calmer in practice before settling. A single live-region element near the kanban root receives short messages constructed from `Run` events as they flow through `EventDispatcher`.

These items are recorded in `docs/ideation/ui-decomposition/README.md` Sub-Phase 6 section so the implementation plan for that phase can pick them up directly.

**The 5-store decision is permanent, not provisional.** PaletteStore exists because palette state has fundamentally different lifecycle properties from UIStore preferences: high-frequency mutation per keystroke, ephemeral session-scoped recent-items tracking, and submenu state that doesn't survive logical navigation. Trying to consolidate would require either splitting UIStore semantically (worse — same problem with different boundaries) or accepting mixed concerns in a single store (worst — the principle was meant to prevent exactly this). The README principle update reflects the design's empirical finding rather than a one-time exception.

**Hover-peek + click-panel does not require touch-screen handling beyond the click affordance.** On touch devices, the hover state never fires; users get the click-to-open behavior directly. No additional code paths needed. The two design plan playgrounds verified this works visually; the inner-button overlay also handles touch correctly because mobile browsers synthesize click events from touch.

**Reduced motion.** The palette's scale-up entry animation, the Sheet's slide-in, and the theme submenu slide all respect `prefers-reduced-motion` via the existing `@media (prefers-reduced-motion: reduce)` block in `frontend/src/app.css`. The hover-peek popover's transitions degrade to instant. The InProgress halo on palette rows is static (no motion); the existing pulsating halo on RunCard is already covered by the existing reduced-motion override.
