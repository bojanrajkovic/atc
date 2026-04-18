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
App.svelte
  ConnectionManager.svelte          (service: WS lifecycle)
  AppShell.svelte                   (connected: layout container)
    TopBar.svelte                   (connected: reads ConnectionStore, UIStore)
      Logo.svelte                   (pure)
      RunnerBar.svelte              (connected: reads RunnerStore)
        RunnerPool.svelte           (pure: dot + label + bar + count)
      ConnectionIndicator.svelte    (pure: live/stale/disconnected)
      ThemeControls.svelte          (connected: reads/writes UIStore)
    KanbanBoard.svelte              (connected: reads RunStore)
      KanbanColumn.svelte           (pure: receives filtered run array)
        ColumnHeader.svelte         (pure: title + count)
        RunCard.svelte              (pure: single run)
          StatusIcon.svelte         (pure)
          JobHeader.svelte          (pure)
          JobMeta.svelte            (pure)
          ProgressBar.svelte        (pure)
          RunnerLabel.svelte        (pure)
      EmptyState.svelte             (pure)
    DetailPanel.svelte              (connected: reads UIStore.selectedRun)
```

### Store Architecture (4 stores, no more)

```
stores/
  runs.svelte.ts        RunStore: Map<RunId, WorkflowRun>
                         $derived: queuedRuns, runningRuns, completedRuns
  runners.svelte.ts     RunnerStore: pool capacities, utilization
  connection.svelte.ts  ConnectionStore: ws status, last update, reconnect
  ui.svelte.ts          UIStore: theme, density, selectedRun, panelOpen
```

**Store boundary rule:** Stores are for cross-cutting concerns only. Parent-to-child data flow uses props. If a leaf component reads a store, it's either not pure or the store is too granular. Four stores is the ceiling — if you feel the need for a fifth, you're probably over-granularizing.

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

All animations respect `prefers-reduced-motion`. The halo is functional (visible across room on wall display), not decorative.

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

### Sub-Phase 5: Interactivity

**Goal:** Command palette, detail panel, per-card expansion, keyboard navigation.

- Copy shadcn-svelte Command component (Cmd+K search/filter)
- Sheet (slide-over detail panel) for run deep-dive: full job list, step list, log output
- Per-card click-to-expand (inline expansion with `transition:slide`)
- Keyboard navigation: arrow keys between cards, Enter to expand, Escape to close
- ARIA live regions for real-time status changes (screen reader announcements)
- **Tests:** Command palette: test search filtering, keyboard navigation (up/down/enter/escape). Sheet: test open/close, content rendering. Keyboard nav: test focus management, arrow key movement. E2E: Cmd+K opens palette, typing filters results, Enter selects. Click card opens detail panel.

### Sub-Phase 6: Polish + Responsive

**Goal:** Empty states, responsive breakpoints, reduced motion, final E2E coverage.

- EmptyState component ("No workflows running" with calm illustration)
- Responsive breakpoints: 3-column desktop (>1200px), condensed runner bar on tablet (768-1200px), tab-based single-column on mobile (<768px)
- `prefers-reduced-motion` audit: verify all animations degrade
- Scrollbar styling (6px WebKit, theme-colored thumb)
- Focus ring audit (2px solid accent, 2px offset on all interactive elements)
- Performance: verify 60fps during burst WebSocket updates with 50+ cards
- **Tests:** EmptyState rendering. Responsive: Playwright viewport tests at each breakpoint. Reduced motion: verify no animations when media query active. Performance: Playwright trace for frame budget under load. Full E2E regression suite covering all prior phases.
