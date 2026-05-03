# Phase 10: Frontend Dashboard — UI Decomposition

Pre-code research and design principles for the ATC frontend dashboard. This is ideation material that feeds into a formal design plan, not an implementation plan.

## Research Documents

| Document | Contents |
|----------|----------|
| [playground-analysis.md](playground-analysis.md) | Component inventory, layout map, state/interaction map, design tokens from the HTML prototype |
| [component-patterns.md](component-patterns.md) | Dashboard layout patterns, kanban decomposition, card anatomy, real-time update strategies, Carbon design system analysis |
| [framework-evaluation.md](framework-evaluation.md) | shadcn-svelte vs Skeleton vs Melt UI vs Bits UI evaluation matrix, Svelte 5 state patterns, component shopping list |

## Framework Decision

**shadcn-svelte** (copy-paste model on Bits UI primitives) + **Bits UI** for custom headless components.

| Option | Verdict | Reason |
|--------|---------|--------|
| shadcn-svelte | Adopt | Copy-paste ownership, OKLCH-compatible, largest dashboard catalog, Svelte 5 + TW4 native |
| Bits UI | Use underneath | Headless a11y primitives for custom components where shadcn-svelte doesn't fit |
| Skeleton | Reject | Competing OKLCH token namespace conflicts with ATC's `--hue`-derived design system |
| Melt UI | Reject | Incomplete Svelte 5 migration, declining community momentum |
| Carbon | Reference only | 169 components but hex/RGB tokens, Svelte 3/4 syntax — useful as pattern reference |

## Component Architecture

### Purity Classification

Every component is classified into one of three categories. This drives testing strategy and determines where state boundaries live.

**Pure components** (props in, DOM out, no stores, no effects):
Trivially testable with `@testing-library/svelte`. Given the same props, they render the same output. These should be the majority.

| Component | Props | Renders |
|-----------|-------|---------|
| StatusIcon | `status` | Colored symbol (queued=blue circle, running=amber play, etc.) |
| Badge | `label`, `variant?` | Pill-shaped count/label |
| CapacityBar | `used`, `total`, `status?` | Horizontal progress bar with scaleX fill |
| ProgressBar | `current`, `total`, `label?` | Step progress with bar and label |
| MonoText | `children` | Tabular-nums monospace text |
| RunnerLabel | `hostname` | Grid icon + truncated mono hostname |
| ColumnHeader | `title`, `count` | Uppercase label + count badge |
| JobHeader | `name`, `status`, `duration` | Icon + name + duration row |
| JobMeta | `repo`, `branch` | Repo + branch secondary text |
| RunCard | `run`, `compact` | Full card composing header/meta/progress/runner |
| RunnerPool | `label`, `used`, `total`, `queued`, `elastic` | Pool indicator (dot + label + bar + count) |
| EmptyState | `message` | Calm idle display |

**Connected components** (read stores, pass data down as props to pure children):
These need store mocking in tests. Keep them thin — their job is to read a store and pass data as props.

| Component | Reads Store | Passes To |
|-----------|-------------|-----------|
| KanbanBoard | RunStore (derived column arrays) | KanbanColumn |
| KanbanColumn | — (receives filtered array as prop) | RunCard |
| RunnerBar | RunnerStore | RunnerPool |
| TopBar | ConnectionStore, UIStore | ConnectionIndicator, ThemeControls |
| AppShell | — | TopBar, KanbanBoard, DetailPanel |

**Service components** (side effects, no visible DOM):
Test the store mutations they produce, not DOM output.

| Component | Side Effects |
|-----------|-------------|
| ConnectionManager | WebSocket lifecycle → writes RunStore, RunnerStore, ConnectionStore |

### Component Tree

```
App.svelte                          (owns global Cmd+K / Cmd+D / Cmd+\ keydown listener)
  ConnectionManager.svelte          (service: WS lifecycle)
  AppShell.svelte                   (connected: layout container)
    TopBar.svelte                   (connected: reads ConnectionStore, UIStore, PaletteStore)
      Logo.svelte                   (pure)
      RunnerBar.svelte              (connected: reads RunnerStore + UIStore.activePoolFilter)
        RunnerPool.svelte           (pure: dot + label + bar + count + isActiveFilter)
      PoolFilterPill.svelte         (pure: "Filtering by [labels] · ✕")
      ConnectionIndicator.svelte    (pure: live/stale/disconnected)
      SettingsPopover.svelte        (connected: reads/writes UIStore)
    KanbanBoard.svelte              (connected: reads RunStore + UIStore.activePoolFilter)
      KanbanColumn.svelte           (pure: receives filtered run array + jobsByRunId)
        ColumnHeader.svelte         (pure: title + count)
        RunCard.svelte              (composition: status-color, density, halo, inner activator button)
          StatusIcon.svelte         (pure)
          JobHeader.svelte          (pure)
          JobMeta.svelte            (pure)
          ProgressBar.svelte        (pure)
          RunnerLabel.svelte        (pure)
          HoverPeekPopover.svelte   (pure: 250ms-debounced peek; touch-suppressed)
      EmptyState.svelte             (pure)
  CommandPalette.svelte             (connected: portal-mounted Cmd+K dialog)
    PaletteSection.svelte           (pure: section header + group wrapper)
    PaletteRunItem.svelte           (pure)
    PaletteJobItem.svelte           (pure)
    PalettePoolItem.svelte          (pure: 3-state browse/query-active/focused)
    PaletteCommandItem.svelte       (pure: label + optional <kbd> chord)
  RunDetailPanel.svelte             (connected: portal-mounted slide-over Sheet)
    PanelHeader.svelte              (pure: status eyebrow + title)
    PanelActions.svelte             (pure: Go-to-run + Close)
    MetaGrid.svelte                 (pure: 2-col definition list)
      MetaCell.svelte               (pure)
    JobBlock.svelte                 (pure: header + scroll-into-view on selectedJobId)
      StepList.svelte               (pure)
        StepItem.svelte             (pure)
```

CommandPalette and RunDetailPanel are App-level siblings (not nested under AppShell) so their Bits UI portals mount at the document root and stack predictably when both are open.

### Store Architecture (5 stores, no more)

```
stores/
  runs.svelte.ts        RunStore: Map<RunId, WorkflowRun>
                         $derived: queuedRuns, runningRuns, completedRuns,
                                   jobStatsByRun, jobsByRunId
  runners.svelte.ts     RunnerStore: pool capacities, utilization
  connection.svelte.ts  ConnectionStore: ws status, last update, reconnect
  ui.svelte.ts          UIStore: theme, density, mode, nowMs, selectedRunId,
                                 selectedJobId, activePoolFilter, lastTriggerRunId
  palette.svelte.ts     PaletteStore: paletteOpen, paletteQuery, subMenu,
                                      recentRunIds (LRU 10, sessionStorage)
```

**Store boundary rule:** Stores are for cross-cutting concerns only. Parent-to-child data flow uses props. If a leaf component reads a store, it's either not pure or the store is too granular. Five stores is the ceiling — if you feel the need for a sixth, you're probably over-granularizing.

**Why a fifth store (revised in Sub-Phase 5):** The original "four stores is the ceiling" principle was revised when PaletteStore landed. Palette state has fundamentally different lifecycle properties from UIStore preferences — high-frequency mutation per keystroke, ephemeral session-scoped recent-items tracking, and submenu state that doesn't survive logical navigation. Consolidating into UIStore would either force a semantic split (same problem with different boundaries) or accept mixed concerns in a single store (the principle was meant to prevent exactly this). The store ceiling moves up because the new dimension is real, not because the discipline is loosening.

**Prop drilling guidance:** If a prop passes through a component that doesn't use it, that's either a missing store (for cross-cutting data like theme) or a sign the tree is too deep. With ~20 components and max 4 levels of nesting, passthrough props should not occur. If they do, reconsider the tree.

## Design and Implementation Principles

These principles are mandatory for Phase 10 design and implementation. They go beyond the existing `.impeccable.md` design system (which covers visual design) to cover component engineering.

### 1. Pure by default

Every new component starts as pure (props → DOM). Adding a store dependency is a conscious escalation that must be justified. The test: "can I render this component with just props and assert on the output?" If yes, it's pure and must stay pure.

**Why:** Pure components are trivially testable, reusable, and predictable. Connected components require mocking infrastructure. The more pure components in the tree, the faster the test suite and the easier the refactors.

### 2. Test-per-component, no exceptions

Every `.svelte` file gets a `.test.ts` sibling. No component is "too simple to test." A StatusIcon that renders the wrong symbol for a status is a P0 accessibility bug.

```
src/components/
  StatusIcon.svelte
  StatusIcon.test.ts
  RunCard.svelte
  RunCard.test.ts
```

Pure components: test with `@testing-library/svelte` + jsdom. Assert on rendered output given various prop combinations.

Connected components: test with store mocks. Verify that the right props are passed to children.

Service components: test the store mutations. Mock the WebSocket, fire events, assert store state.

### 3. Accessibility-first test selectors

Use `getByRole`, `getByLabelText`, `getByText` as primary selectors. **Ban `getByTestId` as the primary way to find elements.** This forces accessible markup as a side effect of testing — if a test can't find an element by role, the element is missing ARIA attributes.

```typescript
// Good: forces the component to have role="progressbar"
screen.getByRole('progressbar', { name: 'Step 3 of 8' })

// Bad: works even if the element is a div with no semantics
screen.getByTestId('step-progress')
```

`getByTestId` is acceptable as a last resort for layout containers with no semantic role, but never for interactive or status-indicating elements.

### 4. Props as exported interfaces

Every component exports a TypeScript interface for its props. Tests import the interface and verify the contract. No inline prop types, no `any`.

```typescript
// RunCard.svelte
export interface RunCardProps {
  run: WorkflowRun;
  compact: boolean;
}

let { run, compact }: RunCardProps = $props();
```

**Why:** The prop interface IS the component's API contract. Exporting it makes the contract explicit, testable, and refactor-safe. If a prop changes, TypeScript catches every callsite.

### 5. E2E tests per phase, not deferred

Each sub-phase adds Playwright E2E coverage for the user-visible flows it introduces. Don't accumulate a "test debt" to be paid off in a polish phase.

- Phase 1 (Foundation): E2E that the app renders, theme switching works
- Phase 2 (Shell): E2E that runner bar renders with mock data
- Phase 3 (Kanban): E2E that columns render with mock data, cards appear in correct columns
- Phase 4 (Cards): E2E that card expansion works, status icons render correctly
- Phase 5 (Interactivity): E2E for Cmd+K, detail panel, keyboard navigation
- Phase 6 (Polish): E2E for responsive breakpoints, reduced motion

### 6. Read-only monitoring, not interactive board

This is NOT a drag-and-drop kanban. Cards move between columns via server-pushed WebSocket events with FLIP animations (`animate:flip` + `crossfade`). No drag-and-drop library. No user-initiated card movement. The only card interaction is selection (click to view detail in the slide-over panel).

**Why this matters for implementation:** It eliminates an entire class of complexity (drag handles, drop zones, reorder logic, optimistic updates, conflict resolution). The card list is a pure projection of server state.

### 7. Animation budget

| Update Type | Duration | Easing | Technique |
|-------------|----------|--------|-----------|
| Value changes (duration tick) | 200-400ms | ease-out-expo | `svelte/motion` tweened |
| Card column transition | <300ms | ease-out-expo | `crossfade` send/receive |
| Card reorder within column | <300ms | ease-out-expo | `animate:flip` |
| Card expansion (compact→expanded) | 250ms | ease-out-expo | `transition:slide` |
| New card arrival | 250ms | ease-out-expo | `transition:fly` |
| Card removal | <200ms | ease-out-expo | `transition:fade` |
| Running card halo | 2s infinite | ease-in-out | CSS `@keyframes` box-shadow |

All animations respect `prefers-reduced-motion`. The halo is functional (high-contrast motion signal for glance-readability at the operator's workstation), not decorative.

### 8. OKLCH token remapping first

shadcn-svelte's default tokens (`--primary`, `--background`, `--foreground`) must be remapped to ATC's OKLCH tokens (`--color-accent`, `--color-surface-base`, `--color-text-primary`) before any component is built. This is Phase 1 work. If a component is built against wrong tokens, every test is written against wrong expectations.

### 9. Derived state, never duplicated state

Column contents are `$derived` from the run store, not maintained as separate arrays. Runner utilization is `$derived` from the runner store, not a separate counter.

```typescript
const queuedRuns = $derived(runStore.runs.filter(r => r.status === 'queued'));
```

**Why:** Single source of truth prevents sync bugs. When a WebSocket event changes a run's status, only the affected columns re-render because their `$derived` dependencies changed. Other columns are untouched.

### 10. Batch WebSocket updates

For burst messages, accumulate updates in a plain array and flush to `$state` on `requestAnimationFrame`:

```typescript
let pending: Update[] = [];
let rafId = 0;
ws.onmessage = (event) => {
  pending.push(JSON.parse(event.data));
  if (!rafId) {
    rafId = requestAnimationFrame(() => {
      for (const update of pending) {
        runStore.applyEvent(update);
      }
      pending = [];
      rafId = 0;
    });
  }
};
```

**Why:** Prevents N re-renders for N messages arriving in the same frame. Common during CI burst activity (a workflow triggers 10 jobs simultaneously).

## Sub-Phases

Phase 10 decomposes into 6 sub-phases. Each is independently shippable and testable.

### Sub-Phase 1: Foundation ✅ COMPLETE

**Implemented in:** PR #22 (`feat/fe-foundation-design` branch)

**What was built:**
- ts-rs TypeScript type generation from Rust structs (`just types`, CI freshness check)
- OKLCH design system with CSS alias layer for shadcn-svelte (Card, Badge, Toggle, Progress)
- 4 Svelte 5 rune-class stores (RunStore, RunnerStore, ConnectionStore, UIStore with localStorage persistence)
- ConnectionManager with WS-first connect protocol, seq-based reconciliation, exponential backoff, AbortController teardown
- EventDispatcher with RAF batching
- Vitest (65 unit tests, 94% coverage) + Playwright (13 E2E tests) fully gated in CI

### Sub-Phase 2: Shell + Runner Bar ✅ COMPLETE

**Implemented in:** PR #23 (`feat/app-shell-design` branch)

**What was built:**
- Backend: `RunnerPoolStats` extended with `is_elastic: bool` and `total: Option<u32>`, ts-rs type regeneration
- AppShell (100dvh flex column layout with TopBar pinned, scrollable content slot)
- TopBar (connected component reading ConnectionStore + RunnerStore, deriving IndicatorState + RunnerPoolDisplay[])
- ConnectionManager.svelte (service component: connect on mount, destroy on unmount, no DOM)
- RunnerBar + RunnerPool with 3 variants (known-capacity with CapacityBar, unknown-capacity, elastic)
- CapacityBar with color thresholds (green <70%, amber 70-99%, red 100%)
- ConnectionIndicator with 4 states (live/stale/connecting/disconnected) + tooltip
- SettingsPopover (theme picker with 4 OKLCH dots, dark/light toggle, density toggle)
- Logo (monospace "ATC" text mark)
- Vitest split into unit (jsdom) + browser (Playwright) projects for portal-dependent components
- shadcn-svelte: Separator, Tooltip, Popover, ToggleGroup installed
- 111 unit/browser tests + 19 E2E tests (307 total with backend), architecture docs updated
- Fixed SIGPIPE false positive in doc-staleness pre-push hook

### Sub-Phase 3: Kanban Board ✅ COMPLETE

**Implemented in:** PR #25 (`feat/kanban-board` branch)

**What was built:**
- RunStore sort refactor: deterministic per-column sort strategies on three `$derived` arrays (queued ascending by createdAt, inProgress descending by runStartedAt with null fallback, completed descending by updatedAt), bigint tie-breakers, direct lexical ISO-8601 comparison, source-level no-localeCompare assertion
- KanbanBoard connected component with tri-state rendering (hydration placeholder gated on totalRuns, empty state, CSS Grid 3-column layout), wired into App.svelte
- KanbanColumn pure component composing ColumnHeader + RunCard with `animate:flip` and `crossfade` send/receive keyed on bigint run.id, semantic ARIA structure (section/aria-labelledby/role=list/role=listitem)
- ColumnHeader pure component (uppercase label + plain-text count badge, no role="status" to avoid screen-reader churn)
- RunCard skeleton with displayTitle + three-cue accessible status indicator (OKLCH color + glyph + sr-only text), `Record<RunStatus, ...>` for type-safe exhaustiveness, scope-contract comment for Sub-Phase 4 boundaries
- Shared animation module (`kanban-transitions.ts`): crossfade pair with fly/fade fallback, DURATION_MOVE/DURATION_ARRIVE/DURATION_REMOVE/FLY_SETTLE_Y constants, single-source `prefersReducedMotion` check zeroing all durations
- ConnectionManager fix: `eventDispatcher.flush()` before setting 'connected' to eliminate buffered-event race
- Dev-mode store bridge (`window.__stores`) for E2E testing
- Shared test factories (`createMockRun`, `createMockRunEvent`) in `src/lib/test-utils/factories.ts`
- 165 unit/browser tests (26 files) + 22 E2E tests, architecture docs updated

### Sub-Phase 4: Cards ✅ COMPLETE

**Implemented in:** PR #30 (`feat/run-cards` branch)

**What was built:**

- Three new OKLCH design tokens (`--timed-out`, `--action-required`, `--neutral`) in both dark and light modes, plus `--halo-color` with mode-aware amber override (dark 0.25 alpha, light 0.5 alpha)
- Automated WCAG contrast-gate test (`design-tokens.test.ts`) covering all 11 status tokens × 4 theme hues × 2 modes against `--surface`; AA failures block the build, AAA misses emit `console.info`
- Five new pure leaf components: `StatusIcon` (exhaustive 11-key glyph lookup with inherited `--status-color`), `JobHeader` (StatusIcon + title + duration row), `JobMeta` (repo · branch with null-branch elision), `ProgressBar` (role=progressbar with scaleX fill and empty-state aria-valuetext), `RunnerLabel` (⊞ summary with null-summary elision)
- Four new pure format utilities: `format/duration.ts` (discriminated `{kind: 'static' | 'live'}` API, MM:SS and H:MM:SS ranges), `format/duration-text.ts` (state-aware formula extracted as pure function so unit tests cover AC12.1-AC12.6 without fake-timer choreography), `format/runners.ts` (single-name / N runners / null branches), `format/status-key.ts` (11-value StatusKey union + `resolveStatusKey(run)` boundary normalization)
- `uiStore.nowMs` shared wall-clock signal (single 1s `setInterval` feeds every live-duration derivation, replacing per-card timers) + `runStore.jobStatsByRun` total-map aggregate (every runId resolves to a JobStats entry, no silent fallbacks)
- `RunCard` evolved into full composition: root `<article>` with `data-run-id` and `data-status` PascalCase, inline `style="--status-color: ..."`, scoped `<style>` with 3px `::before` accent bar, state-aware `$derived.by` duration (static-Completed branch short-circuits before reading `nowMs` so those cards do not subscribe to the tick — AC12.7 reactivity proof via spy on `computeDurationText`)
- `KanbanColumn` threads `jobStatsByRun: ReadonlyMap<bigint, JobStats>` to each RunCard via throwing `requireJobStats` guard (total-map invariant enforced, no silent zero-fallback); wrapper element changed from `<article role="listitem">` to `<div role="listitem">` so RunCard owns the single `<article data-run-id>` root
- `KanbanBoard` reads `runStore.jobStatsByRun` and threads it to all three columns
- Browser-mode tests cover the accent bar computed style, halo keyframe inspection, light-mode halo variable divergence, density `display: none` flipping, and DOM identity preservation across density toggles
- Playwright E2E scenarios (`run-cards.test.ts`) cover all 11 StatusKey fixtures, Queued→InProgress WS transition, density toggle full-cycle, and live duration update via `page.clock.fastForward(1000)` — all using the shared `e2e/lib/ws-mock` harness
- `just test-e2e` recipe added to `justfile`
- Revised accessibility target formalised in `.impeccable.md`: WCAG AA (≥4.5:1) as build gate, AAA (≥7:1) as aspirational
- 302 unit/browser tests (40 files) + 26 Playwright E2E tests, all passing; architecture docs updated

### Sub-Phase 5: Interactivity ✅ COMPLETE

**Implemented in:** PR #41 (`feat/interactivity` branch), per design plan [`docs/design-plans/2026-04-25-interactivity.md`](../../design-plans/2026-04-25-interactivity.md).

**What was built:**

- **Cmd+K command palette** (`CommandPalette` + 5 pure leaves: `PaletteSection`, `PaletteRunItem`, `PaletteJobItem`, `PalettePoolItem`, `PaletteCommandItem`) on vendored shadcn-svelte `Command` (Bits UI). Five sections in fixed source order — Recent / Runs / Jobs / Runner Pools / Commands — with cmdk command-score fuzzy matching, theme submenu with sliding transition, three-state pool row rendering (browse / query-active with `<mark>` highlights / focused), and typographic-curly-quote empty state. Browse mode caps each data section at 5 entries; typing reveals all matches. Run/Job selections await `tick()` so the Sheet mounts before the palette closes.
- **Slide-over detail panel** (`RunDetailPanel` + 7 pure leaves: `PanelHeader`, `PanelActions`, `MetaGrid`, `MetaCell`, `JobBlock`, `StepList`, `StepItem`) on vendored shadcn-svelte `Sheet`. Single-pane layout: status eyebrow + title, 2-column metadata grid, flat list of job blocks each containing its step list. "Go to run" external link opens `WorkflowRun.htmlUrl` in a new tab. Focus restoration to the originating RunCard via `uiStore.lastTriggerRunId` + `onCloseAutoFocus`. **Log fetching deferred** (issue #36).
- **Hover-peek popover on RunCard** (`HoverPeekPopover`) — 250 ms hover debounce, anchored to the right edge of the card with auto-flip. Touch-device gating via `(hover: hover) and (pointer: fine)` media queries. RunCard wraps an absolutely-positioned inner button (`.run-card-activate`) for keyboard activation, preserving the `<article>` landmark.
- **Pool filter** — `PoolFilterPill` + `RunnerPool.isActiveFilter` + filter-aware kanban derivation via the new pure `lib/filters/pool.ts` module. `PoolKey` is the codebase's first branded TypeScript type (a pure string proven at compile time to come from `poolKey(labels)`); `RunStore.jobsByRunId` derived map exposes raw labels for filtering. ADR `docs/architecture-decisions/0001-pool-key-branded-type.md`.
- **Dialog stacking** — Bits UI sibling-dialog mechanics: panel uses `escapeKeydownBehavior="defer-otherwise-close"` and `interactOutsideBehavior="defer-otherwise-close"` so a topmost palette absorbs Esc/click-outside first; first Esc closes palette, second Esc unwinds panel. A `[data-dialog-overlay] ~ [data-dialog-overlay] { display: none }` rule prevents double-darkening when both backdrops mount under `<body>`.
- **Global keyboard chords** in `App.svelte` — single `window` keydown listener dispatches Cmd/Ctrl+K (palette toggle), Cmd/Ctrl+D (dark mode toggle, `preventDefault`s the browser bookmark default), and Cmd/Ctrl+\ (density toggle). All three share an allow-from-palette-input / block-from-other-editables guard.
- **PaletteStore** — fifth Svelte 5 rune-class store (`palette.svelte.ts`), separated from `UIStore` because typing state mutates per-keystroke and shouldn't drag preference consumers along. `recentRunIds` is an LRU of size 10, sessionStorage-backed under key `atc.palette.recent`. **The 4-store ceiling principle in this README is revised to 5** — see Store Architecture above.
- **TypeScript bridge expansion** — dev-mode `window.__stores` extended with `uiStore`, `paletteStore`, and `poolKey` so Playwright E2E tests can drive store state and construct `PoolKey` values directly.
- **App.css bridge** — `@theme inline { --color-*: var(--*) }` block bridges shadcn-svelte color aliases (`bg-popover`, `bg-card`, `bg-muted`, `text-muted-foreground`, etc.) into Tailwind v4. Without it, ~30 utilities silently dropped at compile time and the panel rendered with a transparent background.
- **47 acceptance criteria, 47/47 covered by automated tests** (1 — touch-device gating — flagged for manual verification per `docs/test-plans/2026-04-25-interactivity.md`). Per-component tests (jsdom + browser-mode), full Playwright E2E test files for palette, panel, run-card interactivity, pool filter, and dialog stacking. Architecture docs (`docs/architecture/frontend-app.md`) and `frontend/CLAUDE.md` updated alongside.

**Divergences from prior ideation** (the items below from the original Sub-Phase 5 plan did NOT ship in this form):

- Per-card click-to-expand (inline `transition:slide`) — replaced by the hover-peek + click-to-open-panel model.
- ARIA live regions for run state changes — deferred to Sub-Phase 6.
- Roving-tabindex keyboard navigation across the kanban — deferred to Sub-Phase 6 (Tab cycles cards in DOM order via the inner activator buttons in this phase).
- Log fetching for the detail panel — tracked as issue #36.

### Sub-Phase 6a: Kanban Keyboard Navigation ✅ COMPLETE

**Implemented in:** PR #43 (`feat/kanban-keyboard-nav` branch), per design plan [`docs/design-plans/2026-05-01-kanban-keyboard-nav.md`](../../design-plans/2026-05-01-kanban-keyboard-nav.md).

**What was built:**

- 2D arrow-key navigation across the kanban grid: ArrowUp/ArrowDown within a column, ArrowLeft/ArrowRight between non-empty columns (skipping empties), Home/End to column ends, no-wrap at edges.
- Tab leaves the kanban as a single group; initial focus on first card of first non-empty column.
- Roving state via Svelte 5 context (`<RovingFocusProvider>` wrapping AppShell + CommandPalette + RunDetailPanel) — first production use of `setContext`/`getContext` in this codebase.
- Modifier-key delegation: Cmd+K / Cmd+D / Cmd+\\ continue to fire window-level handlers; Cmd/Shift/Alt+Arrow delegate to browser default.
- Suspension via natural focus scoping: when the palette or detail panel opens, focus moves into Bits UI's portaled DOM (outside the kanban grid), so the action's keydown listener silences without explicit coordination.
- Card-stable focus through FLIP / crossfade animations: focus follows run identity (`data-run-id`), not column position.
- `RunDetailPanel.onCloseAutoFocus` bug fix: when the trigger card has been TTL-evicted while the panel is open, focus now restores to the first card of the first non-empty column instead of stranding on `<body>`.
- New module `lib/components/roving/`: `context.ts`, `geometry.ts`, `action.ts`, `RovingFocusProvider.svelte` plus sibling tests; first production use of a Svelte 5 action.
- Playground at `docs/design-plans/playgrounds/2026-05-01-kanban-keyboard-nav-explorer.html`.
- AC traceability matrix at `docs/test-plans/2026-05-01-kanban-keyboard-nav.md`.

### Sub-Phase 6b: Polish + Responsive ✅ COMPLETE

**Design plan:** [`docs/design-plans/2026-05-02-frontend-1-0-polish.md`](../../design-plans/2026-05-02-frontend-1-0-polish.md)

**Goal:** Empty states, responsive breakpoints, reduced motion, ARIA live regions for run state changes, final E2E coverage.

**Responsive contract (locked):**

| Viewport | Kanban | TopBar |
|---|---|---|
| `≥1280px` (xl) | 3 columns | Single row |
| `768–1279px` | 2 columns | Single row |
| `640–767px` | 2 columns | Wrapped (2 rows) |
| `<640px` | 1 column stack | Wrapped |

Breakpoints: Tailwind v4 `sm:` (≥640px) and `xl:` (≥1280px). No container queries (deferred). No horizontal scroll at any viewport width ≥320px. Mobile-targeted tab navigation between Queued/Running/Completed is **not** part of the 1.0 contract.

**Deliverables:**

- EmptyState component (`EmptyState.svelte`) — pure, schematic-preview treatment (three faint dashed column groups + "Watching for runs." caption); replaces inline "No workflows yet." string in `KanbanBoard.svelte`
- Responsive kanban grid: `grid-cols-1 sm:grid-cols-2 xl:grid-cols-3` on `KanbanBoard.svelte`; TopBar markup rewrite for `<md` wrap + separator hide + `order-*` reflow; RunnerPool label truncation at `<md`
- `prefers-reduced-motion` audit: gate the one remaining ungated animation (`CommandPalette.svelte` theme submenu slide); strengthen existing reduced-motion tests
- Scrollbar styling: global `.atc-scrollbar` class in `app.css` using `var(--border)` thumb; applied to `KanbanColumn` and `RunDetailPanel` body
- Focus ring audit: four custom interactive elements (`command-item`, `PoolFilterPill` clear, `PanelActions` close, `PanelActions` Go-to-run) gain explicit `:focus-visible` rules
- **ARIA live region for run state changes** (deferred from Sub-Phase 5): `LiveRegion` rune-class store in `lib/aria/live-region.svelte.ts`; announces `RunEvent::Requested` (queued) and `RunEvent::Completed` events per-run below the burst threshold (≤3 per flush) and in summary form above it; single `role="status" aria-live="polite" aria-atomic="true"` element at `App.svelte` level; `EventDispatcher` gains a `setOnFlush(cb)` post-flush callback hook; `ConnectionManager` defers wiring until after snapshot+buffered-drain completes so reconnect sequences are silent
- **Performance verification:** Tier 1 — vitest browser-mode deterministic RAF-coalescing test (1000 events, N controlled ticks, hard CI gate); Tier 2 — Playwright frame-budget trace artifact (informational)
- **Tests:** EmptyState rendering. Responsive: Playwright viewport tests at 1280/900/480px. Reduced motion: all animations in inventory matrix have at least one asserting test. Performance: Tier 1 hard gate + Tier 2 artifact. Live region: per-run and summary announcement forms, burst threshold, debounce, error containment. Full E2E regression suite covering all prior phases.

**Implemented in:** PR on `feat/frontend-1-0-polish` branch, per design plan [`docs/design-plans/2026-05-02-frontend-1-0-polish.md`](../../design-plans/2026-05-02-frontend-1-0-polish.md).

**What was built:**

- **Documentation cleanup (Phase 1):** "Wall display" framing removed from 7 structural occurrences across `.impeccable.md`, `docs/ideation/ui-decomposition/component-patterns.md`, `docs/ideation/ui-decomposition/README.md`, `docs/design-plans/2026-04-25-interactivity.md`, and `docs/ideation/design-research.md` (Concourse competitive-research mentions preserved). SP6b responsive contract locked: Tailwind v4 `sm:` (≥640px) and `xl:` (≥1280px) cascade, no mobile tab navigation, no container queries (deferred).
- **EmptyState component (Phase 2):** `EmptyState.svelte` — pure, schematic-preview treatment with three faint dashed column groups (Queued/Running/Completed) and a configurable caption defaulting to "Watching for runs.". `KanbanBoard.svelte` inline placeholder string replaced. Tri-state predicate (connecting / connected-empty / populated) preserved. Responsive kanban grid (`grid-cols-1 sm:grid-cols-2 xl:grid-cols-3` on `KanbanBoard.svelte`); TopBar markup rewritten for `<md` wrap + separator hide + `order-*` reflow; RunnerPool label truncation at `<md`.
- **Polish audits (Phase 3):** `CommandPalette.svelte` theme submenu slide gated on `prefersReducedMotion`; animation inventory matrix completed (all entries in `docs/architecture/frontend-app.md` have ≥1 reduced-motion test). Global `.atc-scrollbar` CSS class in `app.css` applied to `KanbanColumn` and `RunDetailPanel` body. `:focus-visible` outline rules added to `command-item`, `PoolFilterPill` clear button, `PanelActions` close button, and the Go-to-run link.
- **ARIA live region (Phase 4):** New `lib/aria/` module: `transition-kinds.ts` (`TransitionKind` union, `classifyEvent`, `VERB_BY_CONCLUSION` compile-time exhaustiveness dictionary), `format-run-transition.ts` (pure `formatRunTransition` message builder), `live-region.svelte.ts` (`LiveRegion` rune-class store with `BurstAccumulator` 200 ms debounce, per-run announcements below threshold, summary form above). `AriaLiveRegion.svelte` mounts a single `role="status" aria-live="polite" aria-atomic="true" aria-busy` element at `App.svelte` level. `EventDispatcher` gains `setOnFlush(cb)` post-flush callback hook (Phase 4) and `bufferLength` getter; `ConnectionManager` defers wiring until after snapshot+buffered-replay drain. E2E tests cover: ARIA attribute audit, per-run messages, conclusion verbs, multi-transition join, null-branch elision, burst `aria-busy` flip, summary accumulation across flushes.
- **Performance verification (Phase 5):** `dispatcher.perf.browser.test.ts` — Tier 1 deterministic CI gate: 1000 events dispatched in 10 batches of 100 each via manually-driven RAF queue (no wall-clock dependency); asserts `flushCount === 10`, `runStore.runs.size === 1000`, `totalEventsReceived === 1000`. `frame-budget.test.ts` — Tier 2 informational Playwright test: CDP trace with `devtools.timeline,rendering` categories, BeginFrame delta parsing, p50/p95/p99 summary logged to stdout, trace saved as `test-results/frame-budget-trace.json` CI artifact; test always passes. `justfile` gains `test-perf` recipe running both tiers in parallel.
- **Completed E2E regression:** All 147 Playwright E2E tests passing (146 prior + 1 new frame-budget test). All 756 vitest unit/browser tests passing (755 prior + 1 new perf test). Architecture docs (`docs/architecture/frontend-app.md`) updated with EmptyState, responsive breakpoint contract, animation inventory matrix, AriaLiveRegion module + `setOnFlush` contract, and performance verification methodology. `frontend/CLAUDE.md` and root `CLAUDE.md` updated to reflect 1.0-ready status.
