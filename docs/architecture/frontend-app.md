# Frontend App — Architecture

Last verified: 2026-05-02

## Purpose

The frontend app is a standalone Svelte 5 single-page application built with Vite. It provides:

- The user interface for the ATC dashboard
- A complete OKLCH-based design system with four themes (warm, radar, violet, pink) and dark/light mode
- Real-time WebSocket client with event buffering, batching via RAF, and exponential backoff reconnect
- State management via Svelte 5 rune-class stores (runs, runners, connection, ui, palette)
- Comprehensive test coverage with Vitest (unit tests) and Playwright (E2E tests)
- A static build output (`frontend/dist/`) that the backend embeds into its release binary via rust-embed

Complete through Sub-Phase 5 + Sub-Phase 6a (kanban keyboard navigation). Sub-Phase 5 adds: Cmd+K command palette (five sections — Recent/Runs/Jobs/Pools/Commands; theme submenu), slide-over detail panel (Sheet with header, action row, metadata grid, job blocks with step timeline), RunCard activation (inner button overlay + hover-peek popover gated by `(hover: hover) and (pointer: fine)`), pool filter integration (palette → `activePoolFilter` → kanban columns + RunnerPool accent + PoolFilterPill), nested-dialog stacking with global Cmd+K listener, and the first branded TypeScript type (`PoolKey`). Sub-Phase 6a (kanban keyboard navigation, see `docs/design-plans/2026-05-01-kanban-keyboard-nav.md`) adds the roving tabindex system: `RovingFocusProvider` context wrapper (wraps App subtree excluding ConnectionManager), `use:roving` action on KanbanBoard's grid `<div>`, per-card tabindex derivation in RunCard (tabindex=0 on the current-focus card, tabindex=-1 on all others), imperative `.focus()` via a `$effect` in RunCard (with `!popoverOpen` re-entrancy guard), 2D arrow + Home/End navigation with no-wrap and empty-column-skip, modifier-key delegation back to the App-level window handler, structural suspension via natural focus scoping when dialogs open, card-stable focus through FLIP/crossfade transitions, and lost-trigger restoration on panel close (existing `RunDetailPanel.onCloseAutoFocus` bug fixed). Initial focus falls back to the first card of the first non-empty column (Queued > InProgress > Completed priority). Test coverage spans unit (jsdom), browser-mode (Playwright chromium), and E2E tiers. 676 unit/browser tests + 119 E2E tests passing.

## Key Decisions

**Decision:** Standalone Svelte 5, not SvelteKit
**Alternatives considered:** SvelteKit, Next.js, plain Vite + React
**Rationale:** ATC is a dashboard SPA with no server-side rendering needs. SvelteKit adds file-based routing, SSR, and a Node server — none of which are needed when the Rust backend serves the app. Standalone Svelte 5 with Vite produces a static bundle that rust-embed can embed directly.

**Decision:** Tailwind v4 via @tailwindcss/vite plugin (no PostCSS)
**Alternatives considered:** Tailwind v3 with PostCSS, vanilla CSS, CSS modules
**Rationale:** Tailwind v4's Vite plugin is faster and simpler than the PostCSS approach. The `@theme` block syntax allows design tokens to be defined directly in CSS, which is more natural for an OKLCH-based system where all colors derive from a single hue variable.

**Decision:** OKLCH color model with single-hue theme switching
**Alternatives considered:** HSL, hex colors, separate color palettes per theme
**Rationale:** OKLCH is perceptually uniform — equal changes in lightness/chroma look equal across different hues. This means a single `--hue` variable can drive an entire theme: set hue to 70 for warm amber, 155 for radar teal, 280 for violet, 310 for pink. All semantic tokens (surfaces, text, borders, accents) derive from this one value with fixed lightness/chroma combinations.

**Decision:** Biome for .ts/.js, eslint-plugin-svelte + prettier-plugin-svelte for .svelte
**Alternatives considered:** ESLint + Prettier for everything, Biome for everything
**Rationale:** Biome is significantly faster than ESLint/Prettier for TypeScript/JavaScript but does not yet support Svelte file syntax. The split approach uses each tool where it's strongest: Biome for .ts/.js (fast, zero-config), eslint-plugin-svelte for .svelte linting (understands Svelte template syntax), prettier-plugin-svelte for .svelte formatting (handles script/style/markup ordering).

**Decision:** Vendored `Command.Dialog` wrapper extended (not theming-modified) to forward content-level dismissal props
**Alternatives considered:** Raw `Dialog.Root` + `Dialog.Portal` + `Dialog.Overlay` + `Dialog.Content` + `Command.Root` composition inlined in `CommandPalette.svelte` (no vendor change at all)
**Rationale:** The shadcn-svelte 1.2.7 `command-dialog.svelte` wrapper types its `restProps` as `DialogPrimitive.RootProps & CommandPrimitive.RootProps`, which deliberately excludes content-level dismissal props — `escapeKeydownBehavior`, `interactOutsideBehavior`, `onCloseAutoFocus`, `onOpenAutoFocus` — because they live on `Dialog.Content`. Sub-Phase 5's nested-dialog stacking (palette over slide-over panel) needs all four on the inner `<Dialog.Content>`. We extend the vendored wrapper with explicit pass-through fields for the four props rather than inlining ~30 lines of raw Dialog composition in `CommandPalette.svelte`. Project guidance Rule 7 (no vendor modification) is for *theming* changes; API-surface extensions are permitted, with an in-source comment citing the rationale. The four props are extracted from destructuring so they do NOT bleed into `restProps` (which spreads onto `<Dialog.Root>` and `<Command>`, neither of which accepts content-level props). A `strip<T>()` helper removes undefined keys before spreading onto `<Dialog.Content>`, satisfying `exactOptionalPropertyTypes` (EOPT): Bits UI types optional props as absent-or-present-with-value, not present-with-undefined. Patch lives at `frontend/src/lib/components/ui/command/command-dialog.svelte`. **Re-running `pnpm dlx shadcn-svelte@latest add command` will clobber the patch — re-apply after any future re-vendoring.**

## Boundaries

**Owns:** UI rendering, design tokens (OKLCH system), theme switching, Tailwind configuration, Svelte component structure, frontend build output, WebSocket client, state stores, event dispatching, testing infrastructure
**Does not own:** Responsive breakpoints, advanced analytics, routing (future phase), backend serving logic
**Prohibitions:** Do not import backend code. Do not add SvelteKit. Do not use PostCSS for Tailwind (use @tailwindcss/vite). Do not let Biome process .svelte files (use eslint/prettier for those). Do not hand-edit types in `src/lib/types/generated/` (these are generated by ts-rs).

## App Shell

### Component Tree

```
App.svelte                          (global Cmd/Ctrl+K listener via onMount; toggles PaletteStore)
  ConnectionManager.svelte          (service: onMount → connect, onDestroy → destroy; outside provider)
  RovingFocusProvider.svelte        (context-only wrapper: no DOM; owns focusedRunId + kanbanHasFocus state)
    AppShell.svelte                   (layout: 100dvh flex column)
      TopBar.svelte                   (connected: reads ConnectionStore, RunnerStore, UIStore)
        Logo.svelte                   (pure: "ATC" monospace text mark)
        Separator                     (shadcn: vertical divider)
        RunnerBar.svelte              (pure: receives pools[] as prop)
          RunnerPool.svelte           (pure: single pool indicator; isActiveFilter prop adds accent border)
            CapacityBar.svelte        (pure: horizontal fill bar with color thresholds)
        PoolFilterPill.svelte         (pure: shown when activePoolFilter non-null; clear button)
        Separator                     (shadcn: vertical divider)
        ConnectionIndicator.svelte    (pure: colored dot + Tooltip)
        SettingsPopover.svelte        (connected: reads/writes UIStore)
      <slot />                        (content area: KanbanBoard mounted here)
    CommandPalette.svelte             (connected: reads PaletteStore + stores; portals to body)
      PaletteSection.svelte           (pure: group wrapper with heading)
      PaletteRunItem.svelte           (pure: run row with status icon + highlight)
      PaletteJobItem.svelte           (pure: job row with parent run label + highlight)
      PalettePoolItem.svelte          (pure: pool row with label highlight)
      PaletteCommandItem.svelte       (pure: command row with optional keyboard shortcut badge)
    RunDetailPanel.svelte             (connected: reads UIStore + RunStore; Sheet portals to body)
      PanelHeader.svelte              (pure: status icon + run title)
      PanelActions.svelte             (pure: open-in-GitHub link + close button aria-label="Close detail panel")
      MetaGrid.svelte                 (pure: two-column CSS grid wrapper)
        MetaCell.svelte × N           (pure: label/value pair; null value → em-dash)
      JobBlock.svelte × N             (connected: reads uiStore.selectedJobId; scrolls on $effect)
        StepList.svelte               (pure: ordered list of steps)
          StepItem.svelte × N         (pure: step row with status icon + name + duration)
```

**RunCard tree (within KanbanColumn):**

```
RunCard.svelte                      (almost-pure: reads uiStore.nowMs + runStore.jobsByRunId + RovingFocusContext)
  JobHeader.svelte                  (pure: StatusIcon + displayTitle + durationText row)
    StatusIcon.svelte               (pure: 11-StatusKey exhaustive glyph)
  JobMeta.svelte                    (pure: repo · branch secondary line)
  ProgressBar.svelte                (pure: role=progressbar with scaleX fill)
  RunnerLabel.svelte                (pure: ⊞ summary line)
  HoverPeekPopover.svelte           (pure: Popover with status/steps/runner summary; gated by (hover: hover) and (pointer: fine))
  <button class="run-card-activate"> (inner overlay button; tabindex=0/−1 from roving context; imperative .focus() via $effect)
```

### Data Flow

- **ConnectionStore → TopBar → ConnectionIndicator** — TopBar reads `ConnectionStore` connection status and derives `IndicatorState` (live/stale/connecting/disconnected) to pass to ConnectionIndicator
- **RunnerStore → TopBar → RunnerBar → RunnerPool → CapacityBar** — TopBar reads `RunnerStore` to compute pool statistics, passes pools[] array to RunnerBar, which renders RunnerPool items. Each RunnerPool passes used/total to CapacityBar for rendering fill percentage. RunnerPool receives `isActiveFilter` prop from RunnerBar (derived from `uiStore.activePoolFilter`) and shows accent border when true.
- **UIStore ↔ SettingsPopover** — SettingsPopover reads/writes `UIStore` to persist theme and dark/light mode selections directly
- **UIStore.activePoolFilter → TopBar → PoolFilterPill** — PoolFilterPill is shown when `activePoolFilter` is non-null; its clear button sets `uiStore.activePoolFilter = null`
- **PaletteStore ↔ CommandPalette** — CommandPalette reads/writes `PaletteStore` for open state, query text, recent run IDs, and submenu. Cmd+K from App.svelte calls `paletteStore.toggle()`.
- **UIStore.selectedRunId → RunDetailPanel** — Panel is rendered when `selectedRunId !== null`. Setting `selectedRunId = null` (via handleOpenChange or PanelActions) unmounts the Sheet.
- **RunCard → UIStore** — `handleActivate` sets both `uiStore.lastTriggerRunId` and `uiStore.selectedRunId` at click time to enable focus restoration after panel close.

### Component Contracts

**ConnectionIndicatorProps**
- `state: IndicatorState` — Derived state: `'live' | 'stale' | 'connecting' | 'disconnected'`
- `detail: string` — Detail message for tooltip and aria-label (e.g., "Connected", "Reconnecting (attempt 3)...")

**RunnerBarProps**
- `pools: RunnerPoolDisplay[]` — Array of pool display objects

**RunnerPoolDisplay**
- `label: string` — Pool display name (derived from `groupName ?? labels.join(', ')` in TopBar)
- `running: number` — Count of running jobs in pool
- `queued: number` — Count of queued jobs in pool
- `total: number | null` — Total capacity (null until operator sets it)
- `isElastic: boolean` — Whether pool auto-scales (derived from runner group_id == Some(0))

**CapacityBarProps**
- `used: number` — Jobs currently running
- `total: number` — Pool capacity (parent only renders CapacityBar when total is known)

## Kanban Board

### Component Hierarchy

```
KanbanBoard (connected: reads RunStore + ConnectionStore; use:roving={ctx} on grid div)
  KanbanColumn (pure: receives sorted array + column state)
    ColumnHeader (pure: label + badge count)
    RunCard × N (almost-pure: reads roving context; tabindex 0/−1 derived from currentFocusRunId)
```

KanbanBoard has three-state rendering:
1. **Loading** — While `connectionStore.status` is not `'connected'` and no runs loaded yet
2. **Empty** — All three columns are empty
3. **Populated** — Standard kanban grid with one or more cards

### Data Flow

- **Startup:** KanbanBoard reads `$runsStore.queuedRuns`, `$runsStore.inProgressRuns`, `$runsStore.completedRuns`
- **Updates:** WebSocket events → EventDispatcher → RunStore → sorted derived arrays (automatically re-sorted)
- **Rendering:** Sorted arrays pass to KanbanColumn, which renders ColumnHeader + animated RunCard list
- **Animation:** Cards use shared crossfade instance from `kanban-transitions.ts` to move between columns; within-column reorder uses `animate:flip`

### Animation Model

All animations defined in `src/lib/animations/kanban-transitions.ts`:

- **Within-column reorder:** `animate:flip` (FLIP animation)
- **Cross-column movement:** `crossfade` send/receive (fade out of source, fade in at destination)
- **Arrival (first load):** `fly` transition (20px below to current position)
- **Removal:** `fade` transition
- **Motion constants:** `DURATION_MOVE` (300ms), `DURATION_ARRIVE` (250ms), `DURATION_REMOVE` (200ms), `FLY_SETTLE_Y` (20px)
- **Reduced motion:** All durations zeroed when `prefers-reduced-motion` is active
- **Shared instance:** One crossfade pair used across all KanbanColumn instances to ensure visual continuity

### Sort Strategies

All sorting is direct lexical ISO-8601 string comparison (no Date parsing, no millisecond precision loss):

- **Queued column:** Ascending by `createdAt`, then by `run.id` (tiebreaker)
- **In-Progress column:** Descending by `runStartedAt`, then by `run.id`
- **Completed column:** Descending by `updatedAt`, then by `run.id`

### Testing Approach

Tests split across three Vitest projects and E2E tier:

1. **Unit (jsdom, `*.test.ts`):** Store logic, sort function correctness, DOM structure, component lifecycle
2. **Browser (Playwright chromium, `*.browser.test.ts`):** Animation behavior, FLIP transitions, crossfade send/receive, store reactivity (derived array updates), reduced-motion support
3. **E2E (Playwright, `e2e/*.test.ts`):** Full lifecycle (connect → load runs → card renders → animate between columns), real WebSocket event handling, user interactions

## Run Cards

See `## App Shell` for the top-down tree from `App` down to `KanbanColumn` and `## Kanban Board` for the column-to-card handoff. This section documents `RunCard`, its leaf children, and the supporting stores and CSS mechanics.

### Component Tree (RunCard-scoped)

```
RunCard (almost-pure: reads uiStore.nowMs + runStore.jobsByRunId + RovingFocusContext)
  JobHeader (pure: StatusIcon + displayTitle + durationText row)
    StatusIcon (pure: 11-StatusKey exhaustive glyph)
  JobMeta (pure: repo · branch, null-branch elision)
  ProgressBar (pure: role=progressbar with scaleX fill)
  RunnerLabel (pure: ⊞ summary line, null-summary elision)
  HoverPeekPopover (pure: status + step progress + runner summary; anchored to article element)
  <button class="run-card-activate"> (inner overlay button; tabindex=0/−1 from roving context; imperative .focus() via $effect)
```

Leaf components are pure (props in, DOM out, no store reads). `RunCard` is the orchestrator — its `$derived.by` for `durationText` reads `uiStore.nowMs` (but only in live branches; completed non-ActionRequired cards never subscribe to the tick), and it reads `runStore.jobsByRunId` for step aggregation. Sub-Phase 5 added the hover-peek popover (gated by `(hover: hover) and (pointer: fine)` media query) and the inner activation button (opens RunDetailPanel). Kanban keyboard nav Phase 2 added `getRovingContext()` to RunCard for tabindex derivation (`isFocused = ctx.currentFocusRunId === run.id`) and an imperative focus `$effect` that calls `buttonEl.focus()` when `isFocused && ctx.kanbanHasFocus`.

### Store Additions

**`uiStore.nowMs` — shared wall-clock signal** (`frontend/src/lib/stores/ui.svelte.ts`)
- `$state(Date.now())` initialised at module load; refreshed every 1000ms by a constructor-owned `setInterval`.
- Single timer feeds every live-duration derivation across the board. Every card reads the same signal instead of each spawning its own timer.
- `uiStore.destroy()` clears the interval **and** runs the captured `$effect.root()` cleanup so the DOM-sync effects stop firing. Used by fake-timer tests to prevent leaks; production never calls it. `paletteStore.destroy()` mirrors this for the sessionStorage persistence effect — both stores capture the cleanup returned by `$effect.root()` rather than discarding it, otherwise prior store instances keep ticking under `vitest --isolate=false` and clobber later tests' storage state.

**`uiStore.lastTriggerRunId` — activation ref for focus restoration** (`frontend/src/lib/stores/ui.svelte.ts`)
- Set by RunCard's `handleActivate` to the clicked run's id at click time.
- Consumed by RunDetailPanel's `onCloseAutoFocus` to focus the originating card's `.run-card-activate` button after the panel closes.
- Cleared after use. See `## Sheet + Command Dialog Stacking` for the full focus-restoration chain.

**`runStore.jobStatsByRun` — total-map aggregate** (`frontend/src/lib/stores/runs.svelte.ts`)
- `$derived.by<ReadonlyMap<bigint, JobStats>>` that iterates `this.runs.keys()` (not `this.jobsByRun.keys()`) so every known run resolves to a `JobStats` entry, even runs with zero jobs (`{ completed: 0, total: 0, runnerSummary: null }`).
- Consumes `summarizeRunners` from `frontend/src/lib/format/runners.ts` to compute the runner summary string (`null`, single-runner name, or `N runners`).
- Exported `JobStats` interface gives `KanbanColumn` a named type for the prop.

### State-Aware Duration Rules

| Run state | Label format | Base timestamp |
|-----------|-------------|-----------------|
| `Queued` | `waiting MM:SS` (live) | `createdAt` |
| `InProgress` | `MM:SS` (live) | `runStartedAt` (falls back to `createdAt` if null) |
| `Completed` + `conclusion = ActionRequired` | `awaiting action MM:SS` (live) | `updatedAt` |
| `Completed` + any other conclusion | `MM:SS` (static) | `updatedAt − runStartedAt` |

Format: `MM:SS` under 1h, `H:MM:SS` at or above. All durations use `font-variant-numeric: tabular-nums` to prevent layout jitter.

The duration formula is extracted to a pure `computeDurationText(run, nowMs): string` in `frontend/src/lib/format/duration-text.ts`. `RunCard`'s `$derived.by` short-circuits on `Completed + non-ActionRequired` and calls the pure function without reading `uiStore.nowMs`. Svelte 5's fine-grained dependency tracking means those cards never register `nowMs` as a dependency — the derivation does not re-evaluate on tick.

### CSS Mechanics

**Status-color propagation.** `RunCard` sets `style="--status-color: var(...)"` on its root `<article>`; children (`::before` accent bar, `StatusIcon`, `ProgressBar` fill) all read `var(--status-color)` via inherited or explicit CSS reference. The status-to-color map lives in one local `resolveStatusColorVar(key: StatusKey): string` switch inside `RunCard.svelte` — single source of truth, exhaustive over the 11-value union.

**Halo animation.** Declared globally in `app.css`: `.run-card[data-status="InProgress"] { animation: pulse-border 2s ease-in-out infinite; }` with `@keyframes pulse-border` fading `box-shadow` from transparent at 0%/100% to `var(--halo-color)` (8px blur, 2px spread) at 50%. The halo is always amber (H=80) regardless of theme; `--halo-color` has a per-mode override — dark `oklch(78% 0.16 80 / 0.25)`, light `oklch(50% 0.15 80 / 0.5)` — so it stays visible on both surface contrasts. An explicit `@media (prefers-reduced-motion: reduce) { .run-card[data-status="InProgress"] { animation: none; } }` halts the animation cleanly alongside the global reduced-motion reset.

**Accent bar.** `.run-card::before` in `RunCard`'s scoped `<style>` — 3px wide, `left: 0`, `top: 0`, `bottom: 0`, `background: var(--status-color)`. This is the first scoped style block in the kanban components; the halo and density rules stay in `app.css` because they need ancestor selectors (`html[data-density]`, `[data-mode="light"]`) that Svelte's scoped selectors can't cross.

**Density attribute.** `UIStore`'s `$effect.root` block writes `data-density="compact"` (or removes it) on `<html>` when the setting changes. CSS selectors in `app.css` key off `[data-density="compact"]` to `display: none` the `.run-card-meta`, `.run-card-progress`, `.run-card-runner` children and shrink `.run-card` padding + `.run-card-name` font-size. Class names stay global (not Svelte-scoped) so the top-level selector still matches the compiled DOM.

### Design Tokens

Sub-Phase 4 added three OKLCH status tokens in both dark and light modes: `--timed-out` (H=40, amber-red), `--action-required` (H=55, warning-amber), `--neutral` (low-chroma, hue-following). A fourth token `--halo-color` is used by the halo animation; it lives in the mode-level token group, not the status group, because it's always amber.

Sub-Phase 5 added five tokens in `app.css` for the command palette and keyboard shortcut badges:

| Token | Dark | Light | Use |
|-------|------|-------|-----|
| `--text-quiet` | `oklch(55% 0.02 var(--hue))` | `oklch(55% 0.02 var(--hue))` | Tertiary text (placeholder, section labels) |
| `--kbd-bg` | `oklch(18% 0.015 var(--hue))` | `oklch(95% 0.01 var(--hue))` | Keyboard shortcut badge background |
| `--kbd-border` | `oklch(30% 0.02 var(--hue))` | `oklch(85% 0.02 var(--hue))` | Keyboard shortcut badge border |
| `--mark-bg` | `oklch(40% 0.2 80)` | `oklch(85% 0.22 80)` | Search match highlight background |
| `--mark-underline` | `oklch(65% 0.22 80)` | `oklch(55% 0.3 80)` | Search match underline accent |

All five are hue-following for the surface tokens (`--kbd-bg`, `--kbd-border`, `--text-quiet`) and fixed to H=80 (amber) for the match highlight pair (`--mark-bg`, `--mark-underline`). Light-mode overrides live in the `[data-mode="light"]` block.

Accessibility target formalised in `.impeccable.md`: **WCAG AA (≥ 4.5:1) gates the build** via `frontend/src/lib/design-tokens.test.ts` (all 11 status tokens × 4 theme hues × 2 modes against `--surface`); **AAA (≥ 7:1) is aspirational** — misses emit `console.info` but do not fail the test.

### Testing Approach

`RunCard` uses a three-file test split driven by what each environment can observe:

1. **`RunCard.test.ts`** (jsdom, static imports, real timers) — composition, status-color mapping, data-status PascalCase, five-leaf presence, `RunCardProps` type shape.
2. **`RunCard.duration.test.ts`** (jsdom, static imports + direct `uiStore.nowMs` assignment) — AC12.7 reactivity proof: spy on `computeDurationText`, assert zero re-invocations when nowMs changes on a static-Completed card; contrast test on an InProgress card confirms the spy mechanism itself works.
3. **`RunCard.browser.test.ts`** (Vitest browser project, Playwright chromium) — computed-style assertions: `::before` accent width/position/color, `animation-name: pulse-border` gating, keyframe inspection via `CSSKeyframesRule`, `--halo-color` dark-vs-light divergence, density-attribute `display: none` flipping, DOM identity preservation across density toggle.

AC12.1–AC12.6 are covered by `frontend/src/lib/format/duration-text.test.ts` as input→output tests on the pure function. Extracting the formula eliminated the need for `vi.resetModules()` + fake-timer + dynamic-import choreography (which would break `@testing-library/svelte`'s shared Svelte runtime).

Playwright E2E coverage lives in `frontend/e2e/run-cards.test.ts` — four scenarios using `page.clock.install` / `page.clock.fastForward` for deterministic wall-clock control, driven by the shared WS-mock harness in `frontend/e2e/lib/ws-mock.ts`.

## Store Architecture

The frontend uses Svelte 5 rune-class stores as module-level singletons. All stores are defined in `src/lib/stores/` and are initialized on app mount.

**Store-ceiling principle:** Five stores is the ceiling. `PaletteStore` exists as a fifth store (alongside ConnectionStore, RunsStore, RunnerStore, UIStore) because it separates high-frequency typing state (`paletteQuery` changes on every keystroke) and session-scoped recent-items lifecycle (sessionStorage, not localStorage) from UIStore's low-frequency preference-state semantics. Introducing a sixth store requires justification at the same level of design specificity.

**ConnectionStore** (`src/lib/stores/connection.svelte.ts`)
- Tracks connection status (`disconnected`, `connecting`, `connected`, `reconnecting`)
- Tracks reconnect attempt count and last event timestamp
- Does NOT manage WebSocket lifecycle directly — that's `ConnectionManager` in `connection.ts`

**RunsStore** (`src/lib/stores/runs.svelte.ts`)
- Holds a map of `WorkflowRun` objects indexed by `RunId` (bigint)
- Receives and applies `RunEvent` mutations from the WebSocket
- Derives: three sorted arrays (`queuedRuns` ascending by createdAt, `inProgressRuns` descending by runStartedAt, `completedRuns` descending by updatedAt), each with run.id tie-breaker; direct lexical ISO-8601 comparison, no Date parsing
- Uses `SvelteMap<bigint, WorkflowRun>` and `SvelteMap<bigint, Job[]>` from `svelte/reactivity` (not plain `$state<Map>`). `SvelteMap` tracks reads per-key and per-iteration: `.get(key)` / `.set(key, v)` invalidates only consumers of that key; `.values()` / `.keys()` / `.size` invalidate iterating consumers on any structural change. Plain class fields (no `$state` wrapper) — reassignment is intentionally *not* supported since it would replace the reactive instance and silently drop subscribers; mutations go through `.set()` / `.delete()` / `.clear()`. `loadSnapshot` and `clear` call `.clear()` then re-populate, preserving the reactive instance's identity.

**RunnerStore** (`src/lib/stores/runners.svelte.ts`)
- Holds `pools: RunnerPoolStats[]` as a single `$state`-backed array (single consumer — `TopBar`'s pool indicator `$derived` reads the whole collection).
- Exposes `loadPools(pools)` (wholesale replace) and `clear()`. No per-pool update path — intentional: see `feedback_cow_semantics.md` for why `RunnerStore` uses wholesale replace while `RunStore` uses `SvelteMap` per-key reactivity.
- Seeded at WS-connect from `StateSnapshot.poolStats` via `runnerStore.loadPools(snapshot.poolStats)` in `ConnectionManager`.
- Updated live by the `SeqEvent.poolStatsAfter` sidecar: `EventDispatcher.routeEvent` calls `runnerStore.loadPools(seqEvent.poolStatsAfter)` whenever it is non-null. Null sidecars (Run events) are not applied. Derivation stays on the backend; the frontend never recomputes `RunnerPoolStats`.

**UIStore** (`src/lib/stores/ui.svelte.ts`)
- Transient UI state: theme, dark/light mode, density, and selections
- Does not persist to WebSocket (local-only state)
- Sub-Phase 5 additions:
  - `selectedRunId: bigint | null` — which run's detail panel is open (null = panel closed)
  - `selectedJobId: bigint | null` — set by the palette when opening a run via a job row; consumed by JobBlock to scroll the job into view, then cleared
  - `lastTriggerRunId: bigint | null` — set by RunCard's `handleActivate` at click time; consumed by RunDetailPanel's `onCloseAutoFocus` to restore focus to the triggering card's inner button via `document.querySelector('.run-card[data-run-id="${id}"] .run-card-activate')`; then cleared
  - `activePoolFilter: PoolKey | null` — the active pool filter (null = no filter); `PoolKey` is a branded type (see ADR 0001 at `docs/architecture-decisions/0001-pool-key-branded-type.md`)

**PaletteStore** (`src/lib/stores/palette.svelte.ts`)
- High-frequency palette state, separated from UIStore to keep keystroke-rate mutations from co-locating with low-frequency preference state
- `paletteOpen: boolean` — controlled by `open()`, `close()`, `toggle()` methods; `toggle()` is called by the global Cmd/Ctrl+K listener in App.svelte
- `paletteQuery: string` — live search string, updated on every keystroke via `setQuery()`
- `recentRunIds: bigint[]` — LRU list of the last 10 visited run IDs, persisted to sessionStorage under `"atc.palette.recent"` (dot-separated key namespace, distinct from UIStore's dash-separated localStorage keys)
- `subMenu: 'theme' | null` — active palette submenu; only `'theme'` exists in v1
- Methods: `open()`, `close()`, `toggle()`, `setQuery(q)`, `recordRunVisit(id)`, `enterSubmenu(name)`, `exitSubmenu()`

**Derived State**
- Each store exports derived stores (via `$derived`) for filtered views, counts, and computed properties
- RunsStore exports three derived arrays: `queuedRuns`, `inProgressRuns`, `completedRuns` (pre-sorted)
- KanbanBoard applies `filterRunsByPool` (from `src/lib/filters/pool.ts`) to each sorted array when `uiStore.activePoolFilter` is non-null before threading them to KanbanColumn

## Sheet + Command Dialog Stacking

CommandPalette (a `Command.Dialog`) and RunDetailPanel (a `Sheet`, which is itself a Bits UI Dialog) can be open simultaneously. Both portal their overlay and content to `document.body`. Getting Esc-unwind, click-outside, and backdrop suppression right requires understanding two independent stacking mechanisms Bits UI provides.

### Two independent stacking mechanisms

**1. `DialogRootContext` (lexical Svelte `getContext`/`setContext`) — drives `data-nested`**

When a Bits UI Dialog is rendered inside another Dialog's component tree, the inner dialog's `DialogRootState` sees a non-null parent (set via Svelte context). Bits UI then sets `data-nested` on the inner overlay element. This drives the `--bits-dialog-depth` CSS variable and related depth tracking.

**This mechanism does NOT fire for siblings.** CommandPalette and RunDetailPanel are siblings in App.svelte — they are both children of `<body>` after portal, but not children of each other in Svelte's component tree. No `DialogRootContext` parent is ever established. As a result, `data-nested` never appears on either overlay when both are open — the planned `[data-nested][data-dialog-overlay] { display: none }` rule would have silently done nothing for this use case.

**2. `globalThis.bitsEscapeLayers` + `bitsDismissableLayers` (global mount-order Maps) — drive Esc and interact-outside behavior**

Bits UI maintains global insertion-order Maps of registered dialog layers regardless of component nesting. Each dialog pushes itself into these maps on mount and removes itself on unmount. The behavior logic uses `findLast(closeOrIgnore) || layersArr[0]` — the topmost registered layer handles the event first.

This mechanism is what actually enables sibling palette + panel stacking:

- **CommandPalette** — leaves `escapeKeydownBehavior` and `interactOutsideBehavior` at their default (`"close"`). After the palette is opened on top of the panel, it is the last-registered layer; `findLast` finds it first. First Esc closes the palette only.
- **RunDetailPanel** — uses `escapeKeydownBehavior="defer-otherwise-close"` and `interactOutsideBehavior="defer-otherwise-close"`. With only the panel open (palette unregistered), `findLast` finds only the panel; the `defer-otherwise-close` policy falls through to `layersArr[0]` (the panel itself) and closes it. Second Esc closes the panel.

### What "outside" means for dismissable-layer

bits-ui's dismissable-layer fires `onInteractOutside` when the click target is outside the dialog's own content ref (checked via `isOrContainsTarget`), regardless of overlay z-order or CSS stacking context. With the palette as the topmost close-layer in the global stack and the panel set to `defer-otherwise-close`, any click whose target is outside the palette's content ref — including the panel's modal scrim, the visible kanban behind both dialogs, or any other DOM region not inside the palette's content box — fires the palette's `onInteractOutside` and closes the palette only.

The one case that does NOT close the palette: a click directly on the panel content box itself. bits-ui's `isOrContainsTarget` check sees that target as "inside the panel's content ref"; the palette's `onInteractOutside` does not fire for that click. This distinction is why AC6.4's E2E test clicks the QUEUED column (far-left kanban, outside both content boxes) rather than inside the panel slide-over: the QUEUED region is outside both content refs and reliably triggers the observable behavior — palette closes, panel stays open via `defer-otherwise-close`. See `frontend/e2e/stacking.test.ts` AC6.4 test for the verified interaction path.

### Backdrop suppression

Both portal overlays are appended to `document.body` in mount order: panel's overlay first, then palette's overlay. The CSS rule in `app.css` uses the general sibling combinator to hide every overlay after the first:

```css
[data-dialog-overlay] ~ [data-dialog-overlay] {
  display: none;
}
```

This prevents double-darkening of the kanban behind a stacked palette + panel. Because `data-nested` is absent (sibling architecture), `[data-nested][data-dialog-overlay]` would not have matched — the sibling combinator is the correct selector for this topology.

### Focus restoration

Focus restoration after each dialog closes uses an id-then-querySelector pattern rather than stored element refs (RunCard instances unmount/remount when runs change columns, making element refs dangle):

- **Palette closes (panel still open):** `onCloseAutoFocus` queries `button[aria-label="Close detail panel"]` (stable aria-label set by PanelActions.svelte) and focuses it.
- **Panel closes:** `onCloseAutoFocus` reads `uiStore.lastTriggerRunId`, queries `.run-card[data-run-id="${lastTriggerRunId}"] .run-card-activate` (set by RunCard as `data-run-id` on the `<article>`), focuses the inner button, and clears `lastTriggerRunId`. The `data-run-id` attribute survives remounts; the element ref would not.

See Phase 6 plan Note 4 (`docs/implementation-plans/2026-04-25-interactivity/phase_06.md`) for the rationale comparing alternative approaches (element-ref-on-store, default Bits UI focus-scope).

### Roving Focus

The kanban grid implements 2D arrow-key navigation via roving tabindex without adopting the WAI-ARIA `grid` or `listbox` widget contract. Focus management is layered on externally — the existing `<section>` / `<div role="list">` / `<div role="listitem">` structure is preserved.

**Architecture:**

- `<RovingFocusProvider>` is a context-only wrapper component (no DOM element) that wraps `<AppShell>`, `<CommandPalette>`, and `<RunDetailPanel>` in `App.svelte`. It owns two `$state` cells (`focusedRunId: bigint | null`, `kanbanHasFocus: boolean`), a `$derived<Columns> visibleColumns` that applies `filterRunsByPool(runs, runStore.jobsByRunId, uiStore.activePoolFilter)` to all three store arrays (this is the single source of truth for what the kanban DOM actually renders — geometry resolution, `initialFocusRunId`, and the eviction `$effect` all consume `visibleColumns` so the roving logic and the rendered DOM cannot diverge under a pool filter), three additional `$derived` values (`initialFocusRunId`, `currentFocusRunId`, an eviction-watcher `$effect`), and the `restoreFocusToInitial()` function. `visibleColumns` is exposed through `ctx.getVisibleColumns()` so the action and any other consumer can read filtered columns without re-computing the derivation. The provider renders `{@render children()}` with no surrounding DOM, so context propagates by component tree (which means Bits UI portals from `Command.Dialog` and `Sheet` do NOT break `getRovingContext()` access in their consumers).
- `roving/context.ts` exposes `RovingFocusContext` interface, `ROVING_CONTEXT_KEY` symbol, and `setRovingContext` / `getRovingContext` accessors. The getter throws if the context is missing — fast failure rather than silent `undefined`. The interface includes `getVisibleColumns(): Columns` so callers don't need to thread the pool filter or re-compute the derivation.
- `roving/geometry.ts` is pure functions over a `Columns = readonly [WorkflowRun[], WorkflowRun[], WorkflowRun[]]` tuple. The `Columns` tuple is fed from `visibleColumns` (see provider above) — never from raw `runStore.*Runs` directly. Resolves arrow-key + Home/End navigation with no-wrap at edges, empty-column-skipping, and asymmetric-column row-clamping. O(n) per keypress where n is total visible runs (~<100 at dashboard scale).
- `roving/action.ts` is a Svelte 5 action `(node: HTMLElement, ctx: RovingFocusContext) => { destroy }` attached to `KanbanBoard.svelte`'s grid `<div>`. Three listeners: `focusin` (sets `kanbanHasFocus` + syncs `focusedRunId` from the event target's `[data-run-id]` ancestor), `focusout` (clears `kanbanHasFocus` if focus exits the grid), `keydown` (modifier-guard-first, then reads `ctx.getVisibleColumns()` for geometry resolution, calls `ctx.setFocus(targetId)` plus `event.preventDefault()`). Imperative `.focus()` is NOT called from the action — RunCard's `$effect` handles it.
- `RunCard.svelte` derives `isFocused = ctx.currentFocusRunId === run.id`, applies `tabindex={isFocused ? 0 : -1}` on the inner `<button class="run-card-activate">`, and runs a `$effect` that calls `buttonEl.focus()` when `isFocused && ctx.kanbanHasFocus`. The `$effect` covers both user-initiated arrow nav AND cross-column re-focus across crossfade (a fresh DOM node remounts with `isFocused === true`, the effect runs, and `document.activeElement` lands on the new node).

**Suspension is structural:** When `Command.Dialog` (palette) or `Sheet` (panel) opens, Bits UI moves focus into its portaled DOM (outside the kanban grid). The action's keydown listener is scoped via bubble-phase to the grid `<div>`, so it silences naturally — no explicit coordination flag with `paletteStore.paletteOpen` or `uiStore.selectedRunId`.

**Lost-trigger restoration is centralized:** `ctx.restoreFocusToInitial()` is the single source of truth for "involuntary focus loss → first card in first non-empty column." Two callers:
1. **Eviction during keyboard nav** — the provider's `$effect` watches `focusedRunId` against `locate()`; if `locate` returns null the focused run was evicted. The effect is gated on `kanbanHasFocus`: when the kanban owns focus, `restoreFocusToInitial()` fires; when the kanban does not own focus (user has Tab'd to TopBar, opened the palette, or opened the panel), the effect resets `focusedRunId = null` *without* calling `.focus()`, preventing a background TTL/store eviction from yanking focus away from the user's current target.
2. **Panel close with evicted source** — `RunDetailPanel.onCloseAutoFocus` calls it when the trigger-card querySelector returns null. (This replaces the previous bug where the optional-chained `?.focus()` silently no-opped, leaving focus on `<body>`.)

Both paths land focus on the same DOM node under identical preconditions (AC7.4).

**Why context, not a sixth store:** Roving state is component-scoped (dies with the kanban) and doesn't need to survive any persistence boundary. Folding into UIStore would mix preference-state with transient-state; a sixth store would fight the README's "5 stores is the ceiling" principle without empirical justification. Svelte context is the textbook fit: component-tree-scoped state, propagates by composition, dies with the provider.

## EmptyState Component

`frontend/src/lib/components/EmptyState.svelte` is a pure component that renders when the connection is established but no workflow runs exist yet. It replaces the former inline `"No workflows yet."` string in `KanbanBoard.svelte`.

### Props

```typescript
export interface EmptyStateProps {
  message?: string  // Default: "Watching for runs."
}
```

### Visual Treatment

The schematic preview renders three faint dashed column groups (Queued / Running / Completed), each containing three rows of monospace placeholder dots (`· · · · · · · ·`), with a caption below the preview. The treatment is purely cosmetic: all rows carry `aria-hidden="true"` so screen readers only see the caption text.

Selector surface: `[data-empty-col]` on each column group, `[data-empty-row]` on each placeholder row. These attributes are used by unit tests to assert structure without relying on CSS class names.

### Tri-state integration

`KanbanBoard.svelte` maintains three rendering branches:

1. **Connecting** — `connectionStore.status !== 'connected' && totalRuns === 0`: inline "Connecting…" hydration placeholder.
2. **Empty** — `connectionStore.status === 'connected' && totalRuns === 0`: `<EmptyState />` with default caption.
3. **Populated** — all other states: the kanban grid with three columns.

`EmptyState` is only rendered in branch 2. The tri-state shape is preserved by design — adding a variant for "filtered empty" (no runs match the current pool filter) is out of scope for 1.0 and deferred.

## Responsive Breakpoint Contract

The kanban grid and TopBar adapt to viewport width using Tailwind v4's mobile-first cascade. The detail panel (`RunDetailPanel`) uses Bits UI Sheet overlay-style positioning and does not shrink the kanban, so viewport breakpoints are sufficient — container queries would adapt to nothing the kanban cares about.

### Kanban grid

`KanbanBoard.svelte`'s grid `<div>` uses:

```
grid-cols-1 sm:grid-cols-2 xl:grid-cols-3
```

| Viewport | Breakpoint | Columns |
|----------|-----------|---------|
| `≥1280px` | `xl:` | 3 columns |
| `640–1279px` | `sm:` | 2 columns (Completed wraps to row 2) |
| `<640px` | (base) | 1 column stack |

No horizontal page scroll at any viewport width ≥320px. The `min-w-0` class on the grid container prevents overflow from long run titles.

### TopBar wrap

`TopBar.svelte`'s header uses `flex flex-wrap gap-y-2`. Four direct children:

1. **Logo** — natural flex order
2. **RunnerBar wrapper** — `order-3 md:order-2 basis-full md:basis-0 md:flex-1 min-w-0`; the `basis-full` at `<md` forces the row wrap
3. **ConnectionIndicator** — `order-2 md:order-4`; appears next to Logo on row 1 at `<md`
4. **SettingsPopover** — `order-5`; follows RunnerBar on row 2 at `<md`

Separators carry `hidden md:block` — hidden at `<md` to avoid floating dividers on the wrapped row.

| Viewport | TopBar layout |
|----------|--------------|
| `≥768px` (md) | Single row: Logo → Separator → RunnerBar → Separator → ConnectionIndicator → SettingsPopover |
| `<768px` | Row 1: Logo + ConnectionIndicator; Row 2: RunnerBar + SettingsPopover |

The `md:` (768px) breakpoint was verified manually during Sub-Phase 6b implementation. Header height increases from 48px to ~94px when wrapping (two rows). The breakpoint feels appropriate for the amount of content.

### Pool label truncation

`RunnerPool.svelte`'s label `<span>` carries `truncate max-w-[12ch] md:max-w-none`. At `<md`, long pool labels are capped at 12 characters to preserve layout density in the narrower two-row header. At `md+`, the full label is shown.

### Future extensibility

Container queries (`@container`) are the natural upgrade path if a future sidebar or split-view feature affects the kanban's effective width independently of the viewport. The migration is mechanical: wrap the kanban in a `@container` and replace `sm:`/`xl:` with `@sm:`/`@xl:`.

### Global keyboard chord listener

A single `window.addEventListener('keydown', ...)` is mounted via `onMount` in App.svelte and removed on destroy. It dispatches three Cmd/Ctrl chords:

- **Cmd+K** — calls `paletteStore.toggle()` to open or close the command palette.
- **Cmd+D** — toggles `uiStore.mode` between `'dark'` and `'light'`. `preventDefault()` is essential here because Cmd+D is the browser's "bookmark this page" default, which would otherwise win even when the palette is open. If the palette is open, it closes after the toggle so keyboard and click paths produce identical end states (the palette's "Toggle dark mode" command also closes the palette).
- **Cmd+\\** — toggles `uiStore.density` between `'comfortable'` and `'compact'`, with the same close-palette behavior.

All three chords share an editable-context guard: `e.target.closest('[data-slot="command-input"]')` is allowed (so chords still fire from inside the palette input), but other `input/textarea/[contenteditable]` elements opt out so future text inputs don't accidentally trigger global actions while the user is typing.

No separate Esc handler exists in App.svelte — Esc dismissal is delegated entirely to Bits UI's escape-keydown wiring on each dialog.

## Connection Protocol

The frontend uses a **WS-first protocol** with pre-connect buffering and seq-based reconciliation.

**Startup sequence:**
1. App mounts and creates stores
2. `ConnectionManager` opens WebSocket to `/v1/ws`
3. While WS is connecting, inbound messages are buffered in `preConnectBuffer`
4. WS opens; client fetches full state snapshot via REST `/v1/state`
5. Snapshot is loaded into stores; buffered events with seq >= snapshot seq are flushed synchronously
6. Connection transitions to `'connected'`; subsequent WS events are dispatched via `EventDispatcher`

**Seq reconciliation:**
- Each event carries a monotonic sequence number (`SeqEvent.seq`)
- Buffered pre-connect events with seq < snapshot seq are discarded as stale
- No explicit gap detection — reconnect re-fetches the full snapshot via `/v1/state`

**Reconnect:**
- Connection loss triggers exponential backoff (1s, 2s, 4s, ..., max 30s)
- On reconnect, client re-fetches full state snapshot to ensure consistency
- Event queue resumes from the new sequence number

## Test Strategy

Testing is split into four tiers: unit (Vitest jsdom), browser-mode (Vitest Playwright), integration, and E2E (Playwright).

**Unit tests (Vitest with jsdom, `*.test.ts`)**
- Test individual store functions and derived state computation
- Mock WebSocket with `new WebSocket()` stubs
- Test EventDispatcher batching and RAF flushing
- Verify ConnectionManager reconnect backoff and buffering
- Location: `src/lib/**/*.test.ts`
- Run with: `pnpm test` or `vitest`

**Browser-mode tests (Vitest with Playwright chromium, `*.browser.test.ts`)**
- Test animation behavior, DOM mutation observation, store reactivity with real Svelte 5 runes
- Test FLIP animations, crossfade transitions, reduced-motion support
- Run in headless Chromium (not jsdom) to access Animation API and browser rendering
- Location: `src/lib/**/*.browser.test.ts`
- Run with: `pnpm test:browser` (separate Vitest project)

**Integration tests (Vitest + MSW)**
- Test store interactions and full workflow runs through stores
- Mock server responses with Mock Service Worker (MSW)
- Verify state consistency after events
- Verify reconnect and state re-fetch behavior
- Location: `src/lib/__tests__/*.test.ts` (when added)

**E2E tests (Playwright)**
- Test app rendering, theme switching, and dark/light mode toggle
- Verify HTML attributes (`data-theme`, `data-mode`) and CSS custom properties (`--hue`)
- Run in headless Chromium with dev server auto-start
- Playwright starts `pnpm dev` automatically, reuses existing server in local mode
- `fullyParallel: true` so tests inside a single file are spread across workers, not just files
- `workers: process.env.CI ? '75%' : undefined` — Playwright's CI default is `workers: 1`, which would silently neutralise `fullyParallel`; setting an explicit fraction in CI restores parallelism while leaving headroom for the shared Vite dev server
- Location: `e2e/*.test.ts`
- Run with: `pnpm test:e2e` or `playwright test`

## Files

**Core App**
- `frontend/src/main.ts` — Svelte mount point
- `frontend/src/App.svelte` — Root component with theme switching UI
- `frontend/src/app.css` — Tailwind import, OKLCH design system tokens (theme definitions, semantic colors), base styles
- `frontend/index.html` — HTML entry point (sets data-theme, data-mode attributes)

**Infrastructure & Configuration**
- `frontend/vite.config.ts` — Vite config with @tailwindcss/vite and Svelte plugins
- `frontend/svelte.config.js` — Svelte preprocessor config
- `frontend/tsconfig.json` — TypeScript configuration
- `frontend/biome.json` — Biome lint/format config for .ts/.js files
- `frontend/eslint.config.mjs` — ESLint config for .svelte files
- `frontend/.prettierrc` — Prettier config for .svelte files
- `frontend/vitest.config.ts` — Vitest config (jsdom environment, test discovery)
- `frontend/playwright.config.ts` — Playwright config (webServer auto-start, Chromium headless, baseURL)
- `frontend/package.json` — Dependencies (Svelte, Vite, Tailwind, Vitest, Playwright, shadcn-svelte, etc.)
- `frontend/pnpm-workspace.yaml` — pnpm workspace with catalog version pins

**Type Definitions & Utilities**
- `frontend/src/vite-env.d.ts` — Vite/Svelte/TypeScript ambient types
- `frontend/src/lib/utils.ts` — Utility functions (WithElementRef type, cn() classname helper, etc.)
- `frontend/src/lib/types/generated/` — TypeScript types generated by ts-rs from Rust (do not hand-edit)

**Stores & State Management**
- `frontend/src/lib/stores/connection.svelte.ts` — ConnectionStore: WebSocket lifecycle and status
- `frontend/src/lib/stores/runs.svelte.ts` — RunsStore: WorkflowRun state and mutations
- `frontend/src/lib/stores/runners.svelte.ts` — RunnersStore: Runner state and mutations
- `frontend/src/lib/stores/ui.svelte.ts` — UIStore: Local UI state (theme, mode, density, selectedRunId, selectedJobId, lastTriggerRunId, activePoolFilter, nowMs)
- `frontend/src/lib/stores/palette.svelte.ts` — PaletteStore: command palette open/query/recent/submenu state

**Event Handling**
- `frontend/src/lib/connection.ts` — ConnectionManager: WebSocket client with WS-first protocol, pre-connect buffering, exponential backoff reconnect
- `frontend/src/lib/dispatcher.ts` — EventDispatcher: Batches store mutations and flushes via requestAnimationFrame

**Components**
- `frontend/src/lib/components/` — shadcn-svelte component library with tailwind aliases

**App Shell Components**
- `frontend/src/lib/components/AppShell.svelte` — Layout container: 100dvh flex column with TopBar + slot
- `frontend/src/lib/components/TopBar.svelte` — Header bar: Logo, RunnerBar, PoolFilterPill, ConnectionIndicator, SettingsPopover
- `frontend/src/lib/components/ConnectionManager.svelte` — Service component: connects WebSocket on mount, disconnects on destroy
- `frontend/src/lib/components/Logo.svelte` — Pure: "ATC" monospace text mark
- `frontend/src/lib/components/CapacityBar.svelte` — Pure: horizontal fill bar with color thresholds (unused/normal/warning/critical)
- `frontend/src/lib/components/ConnectionIndicator.svelte` — Pure: colored dot + tooltip showing connection state
- `frontend/src/lib/components/RunnerPool.svelte` — Pure: single pool indicator with pool name, running/queued counts, capacity bar; `isActiveFilter` prop adds accent border
- `frontend/src/lib/components/RunnerBar.svelte` — Pure: grid of pool indicators, receives pools[] prop
- `frontend/src/lib/components/PoolFilterPill.svelte` — Pure: active-filter badge showing label text + clear button; shown when `activePoolFilter` is non-null
- `frontend/src/lib/components/SettingsPopover.svelte` — Connected: theme selector popover, reads/writes UIStore

**Command Palette Components**
- `frontend/src/lib/components/CommandPalette.svelte` — Connected: reads PaletteStore + RunStore + RunnerStore + UIStore; renders Command.Dialog portaled to body; five sections (Recent/Runs/Jobs/Pools/Commands) + theme submenu
- `frontend/src/lib/components/PaletteSection.svelte` — Pure: Command.Group wrapper with heading
- `frontend/src/lib/components/PaletteRunItem.svelte` — Pure: Command.Item row for a workflow run with status icon and highlighted match
- `frontend/src/lib/components/PaletteJobItem.svelte` — Pure: Command.Item row for a job with parent run label and highlighted match
- `frontend/src/lib/components/PalettePoolItem.svelte` — Pure: Command.Item row for a runner pool with label highlight
- `frontend/src/lib/components/PaletteCommandItem.svelte` — Pure: Command.Item row for a utility command with optional keyboard shortcut badge

**Roving Focus Module (keyboard navigation)**
- `frontend/src/lib/components/roving/context.ts` — `RovingFocusContext` interface (includes `getVisibleColumns(): Columns`), `ROVING_CONTEXT_KEY` symbol, `setRovingContext()` / `getRovingContext()` helpers; `getRovingContext` throws with a descriptive message if called outside a provider tree
- `frontend/src/lib/components/roving/RovingFocusProvider.svelte` — Context-only wrapper (no DOM element); owns `focusedRunId: bigint | null` and `kanbanHasFocus: boolean` `$state` cells; derives `visibleColumns: Columns` (all three store arrays filtered through `filterRunsByPool` with `uiStore.activePoolFilter` — the single source of truth for what the kanban DOM renders); derives `initialFocusRunId` (first card of first non-empty visible column, Queued > InProgress > Completed), `currentFocusRunId` (focusedRunId ?? initialFocusRunId); exposes `setFocus`, `setKanbanHasFocus`, `restoreFocusToInitial`, `getVisibleColumns`; eviction `$effect` calls `restoreFocusToInitial` only when `kanbanHasFocus === true`; when kanban does not own focus, resets `focusedRunId = null` without touching the DOM
- `frontend/src/lib/components/roving/action.ts` — `roving` Svelte action; attaches `focusin` / `focusout` / `keydown` listeners to the grid element; `focusin` calls `ctx.setFocus(runId)` from `data-run-id`; `focusout` calls `ctx.setKanbanHasFocus(false)` when focus leaves the grid; `keydown` reads `ctx.getVisibleColumns()` for geometry resolution (filtered columns, matching the DOM)
- `frontend/src/lib/components/roving/geometry.ts` — `locate(runId, columns): Position | null` — pure function returning `{col, row}` for a given run id across the `visibleColumns` tuple; consumed by the action for arrow-key navigation and by the provider's eviction `$effect`

**Kanban Board Components**
- `frontend/src/lib/components/KanbanBoard.svelte` — Connected: tri-state (loading/empty/grid), reads RunStore + ConnectionStore + UIStore, applies `filterRunsByPool` when `activePoolFilter` set, threads `runStore.jobStatsByRun` to each KanbanColumn; attaches `use:roving={ctx}` to the grid `<div>` for keyboard focus management
- `frontend/src/lib/components/KanbanColumn.svelte` — Pure: receives sorted runs + `jobStatsByRun: ReadonlyMap<bigint, JobStats>`, renders ColumnHeader + `<div role="listitem">` wrappers around RunCards, enforces total-map invariant via throwing `requireJobStats` guard, applies crossfade/flip animations
- `frontend/src/lib/components/ColumnHeader.svelte` — Pure: uppercase column label + count badge
- `frontend/src/lib/components/RunCard.svelte` — Composition root: root `<article>` with `--status-color` inline, `data-status` and `data-run-id` attributes, state-aware `$derived.by` duration, inner activation button (tabindex=0/−1 from roving context), hover-peek popover

**Run Card Leaves**
- `frontend/src/lib/components/StatusIcon.svelte` — Pure: 11-StatusKey exhaustive glyph + sr-only label; color inherited from parent's `--status-color`
- `frontend/src/lib/components/JobHeader.svelte` — Pure: StatusIcon + displayTitle + durationText row with tabular-nums
- `frontend/src/lib/components/JobMeta.svelte` — Pure: `repo · branch` secondary line with null-branch elision and aria-hidden middle dot
- `frontend/src/lib/components/ProgressBar.svelte` — Pure: track + scaleX fill, `role="progressbar"`, `aria-valuetext="No jobs"` when total is 0
- `frontend/src/lib/components/RunnerLabel.svelte` — Pure: `⊞ summary` monospace line; renders nothing when summary is null
- `frontend/src/lib/components/HoverPeekPopover.svelte` — Pure: Popover with status, step progress, and runner summary; parent (RunCard) gates instantiation behind `(hover: hover) and (pointer: fine)` media query check

**Detail Panel Components**
- `frontend/src/lib/components/RunDetailPanel.svelte` — Connected: reads UIStore + RunStore; Sheet portaled to body; `escapeKeydownBehavior="defer-otherwise-close"` + `interactOutsideBehavior="defer-otherwise-close"` for correct Esc-unwind behind palette
- `frontend/src/lib/components/PanelHeader.svelte` — Pure: status icon + run title
- `frontend/src/lib/components/PanelActions.svelte` — Pure: open-in-GitHub link + close button with `aria-label="Close detail panel"` (stable selector used by CommandPalette focus restoration)
- `frontend/src/lib/components/MetaGrid.svelte` — Pure: two-column CSS grid wrapper
- `frontend/src/lib/components/MetaCell.svelte` — Pure: label/value pair; null value renders em-dash
- `frontend/src/lib/components/JobBlock.svelte` — Connected: reads `uiStore.selectedJobId`; scrolls into view via `$effect` when id matches; calls `onSelectedJobIdConsumed` after scroll to prevent re-scroll
- `frontend/src/lib/components/StepList.svelte` — Pure: ordered list of steps
- `frontend/src/lib/components/StepItem.svelte` — Pure: step row with status icon, name, and duration

**Animation Module**
- `frontend/src/lib/animations/kanban-transitions.ts` — Shared crossfade instance, motion constants, reduced-motion support

**Filter Utilities (pure functions)**
- `frontend/src/lib/filters/pool.ts` — `PoolKey` branded type + `poolKey(labels)` constructor + `filterRunsByPool(runs, jobsByRunId, poolFilter)` filter; first branded TypeScript type in the codebase — see ADR `docs/architecture-decisions/0001-pool-key-branded-type.md` for rationale

**Format Utilities (pure functions)**
- `frontend/src/lib/format/duration.ts` — `formatDuration({kind: 'static' | 'live', ...})` discriminated API; `MM:SS` under 1h, `H:MM:SS` at or above; negative-diff clamp
- `frontend/src/lib/format/duration-text.ts` — `computeDurationText(run, nowMs): string` and `computeJobDurationText(job, nowMs): string` — state-aware formulas; called by RunCard's `$derived.by` and RunDetailPanel
- `frontend/src/lib/format/runners.ts` — `summarizeRunners(jobs): string | null` — single-name / `N runners` / null branches
- `frontend/src/lib/format/status-key.ts` — `StatusKey` union (11 values) + `resolveStatusKey(run)` normalisation at the boundary
- `frontend/src/lib/format/timestamp.ts` — `formatTimestamp(iso: string): string` — locale-aware date/time formatting for the detail panel metadata grid

**Design Token Tests**
- `frontend/src/lib/design-tokens.test.ts` — Automated WCAG contrast gate: 11 status tokens × 4 themes × 2 modes against `--surface`. AA misses fail the test; AAA misses emit `console.info`.

**Testing**
- `frontend/src/lib/**/*.test.ts` — Vitest unit tests for stores, connection, and dispatcher
- `frontend/src/lib/**/*.browser.test.ts` — Vitest browser-mode tests for animations, store reactivity, reduced-motion support
- `frontend/src/lib/components/**/*.test.ts` — Vitest unit tests for components
- `frontend/src/lib/components/BackdropSuppression.browser.test.ts` — Browser-mode test: verifies the `[data-dialog-overlay] ~ [data-dialog-overlay] { display: none }` CSS rule hides the second overlay when both Sheet and Command.Dialog are open
- `frontend/e2e/lib/ws-mock.ts` — Shared Playwright harness: `WS_MOCK_INIT_SCRIPT`, `makeRunEvent`, `makeJobSeqEvent`, `sendWS` — intercepts `new WebSocket('/v1/ws')` and bridges store updates via `window.__stores`
- `frontend/e2e/theme.test.ts` — Playwright E2E tests: app rendering, theme switching, dark/light mode toggle
- `frontend/e2e/app-shell.test.ts` — Playwright E2E tests: app shell rendering, runner bar pool indicators, connection indicator, settings popover
- `frontend/e2e/kanban.test.ts` — Playwright E2E tests: kanban board lifecycle, card movement, WebSocket event handling
- `frontend/e2e/run-cards.test.ts` — Playwright E2E tests: RunCard rendering across all 11 StatusKeys, Queued→InProgress transition, density toggle, `page.clock.fastForward` duration updates
- `frontend/e2e/palette.test.ts` — Playwright E2E tests: Cmd+K open/close, query filtering across sections, pool filter selection, theme submenu, command actions
- `frontend/e2e/pool-filter.test.ts` — Playwright E2E tests: pool filter pill shows/clears, kanban filters by pool, RunnerPool accent border
- `frontend/e2e/stacking.test.ts` — Playwright E2E tests: palette+panel Esc-unwind order, single backdrop with both open, click-outside-palette-inside-panel, Cmd+K toggle while palette open

**Roving Focus Test Harnesses** (co-located with their respective test files)
- `frontend/src/lib/components/roving/RovingFocusProvider.test-harness.svelte` / `RovingHarnessGrid.svelte` — Two-component harness for RovingFocusProvider browser tests: outer wraps provider, inner (RovingHarnessGrid) calls `getRovingContext()` + `use:roving` and renders stub run-card buttons
- `frontend/src/lib/components/RunCard.test-harness.svelte` / `RunCardHarnessInner.svelte` — Two-component harness for RunCard unit tests: outer wraps RovingFocusProvider, inner calls `getRovingContext()` and renders N RunCards with `onCtxReady` context-capture callback
- `frontend/src/lib/components/KanbanColumn.test-harness.svelte` — Single-column harness wrapping one KanbanColumn in a RovingFocusProvider; used by KanbanColumn.test.ts unit tests
- `frontend/src/lib/components/KanbanBoardInvariant.test-harness.svelte` / `KanbanBoardInvariantHarnessInner.svelte` — Two-component harness for kanban-level tabindex invariant tests: renders all three KanbanColumns inside one RovingFocusProvider with `onCtxReady` callback; used by `KanbanColumn.tabindex.browser.test.ts`
