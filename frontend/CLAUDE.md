# CLAUDE.md — frontend

Last verified: 2026-05-05 (Phase 3a/3b: snapshot cursor renamed to `lastSeq`, buffer filter inverted to `seq > lastSeq`, runner pools `$derived.by(() => computePoolStats(runStore.jobs))` — no `loadPools`, no `SeqEvent.poolStatsAfter` sidecar)

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component: mounts ConnectionManager + AppShell, plus CommandPalette and RunDetailPanel as portal-target siblings; owns the global `window` keydown listener that handles Cmd/Ctrl+K (palette toggle), Cmd/Ctrl+D (dark mode toggle, preventDefaults the browser bookmark shortcut), and Cmd/Ctrl+\\ (density toggle), all with the same allow-from-palette / block-from-other-editables guard |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point; exports stores to `window.__stores` bridge for E2E test harness |
| `src/vite-env.d.ts` | Window type augmentation for `__stores` bridge (runStore, connectionStore, runnerStore, uiStore, paletteStore, poolKey) |
| `vite.config.ts` | Build config with Tailwind v4 and Svelte plugins |
| `vitest.config.ts` | Vitest workspace config (delegates to unit and browser projects) |
| `vitest.config.unit.ts` | Vitest unit project (jsdom, `*.test.ts`) |
| `vitest.config.browser.ts` | Vitest browser project (Playwright chromium, `*.browser.test.ts`) |
| `playwright.config.ts` | Playwright E2E test configuration with webServer auto-start |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts`, `palette.svelte.ts` |
| `src/lib/stores/runners.svelte.ts` | RunnerStore — `readonly pools = $derived.by(() => computePoolStats(runStore.jobs))` (Phase 3b). Module also exports `computePoolStats(jobs: Job[]): RunnerPoolStats[]` as a pure function (skip Waiting/Completed; group by sorted `JSON.stringify(labels)`; bigint-aware `groupId === 0n` for `isElastic`). No `loadPools`, no `clear` |
| `src/lib/stores/runs.svelte.ts` | RunStore — adds `jobs: $derived.by<Job[]>` flat view across `jobsByRun.values()` (Phase 3b) used as the single dependency for `runnerStore.pools` |
| `src/lib/stores/palette.svelte.ts` | PaletteStore — `paletteOpen`, `paletteQuery`, `recentRunIds`; separate store for high-frequency typing state and recent-items lifecycle (see `docs/architecture/frontend-app.md`) |
| `src/lib/filters/pool.ts` | `PoolKey` branded type + `poolKey()` factory + `filterRunsByPool()` — first branded TypeScript type in the codebase; see ADR `docs/architecture-decisions/0001-pool-key-branded-type.md` |
| `src/lib/dispatcher.ts` | EventDispatcher — routes primitive WebSocket events (`Run`, `Job`) to `runStore`; batches via requestAnimationFrame; exposes `setOnFlush` post-flush hook for the ARIA live region. Phase 3b: no longer touches `runnerStore` (pool stats are derived from `runStore.jobs`) |
| `src/lib/connection.ts` | ConnectionManager — WS-first protocol with pre-connect buffering and exponential backoff reconnect |
| `src/lib/types/generated/` | ts-rs generated TypeScript types from Rust (do not hand-edit) |
| `src/lib/components/AppShell.svelte` | Layout container: 100dvh flex column with TopBar + slot for content area |
| `src/lib/components/TopBar.svelte` | Header bar: reads stores, composes Logo, RunnerBar, ConnectionIndicator, SettingsPopover |
| `src/lib/components/ConnectionManager.svelte` | Service component: connects WebSocket on mount, disconnects on destroy |
| `src/lib/components/Logo.svelte` | Pure: "ATC" monospace text mark |
| `src/lib/components/CapacityBar.svelte` | Pure: horizontal fill bar with color thresholds (unused/normal/warning/critical) |
| `src/lib/components/ConnectionIndicator.svelte` | Pure: colored dot + tooltip showing connection state |
| `src/lib/components/RunnerPool.svelte` | Pure: single pool indicator with pool name, running/queued counts, capacity bar |
| `src/lib/components/RunnerBar.svelte` | Pure: grid of pool indicators, receives pools[] prop |
| `src/lib/components/SettingsPopover.svelte` | Connected: theme selector popover, reads/writes UIStore |
| `src/lib/components/KanbanBoard.svelte` | Connected: reads RunStore + ConnectionStore, threads `runStore.jobStatsByRun` to each column, renders tri-state (loading/empty/kanban grid) |
| `src/lib/components/KanbanColumn.svelte` | Pure: receives sorted runs + `jobStatsByRun: ReadonlyMap<bigint, JobStats>`, enforces total-map invariant via throwing `requireJobStats` guard, renders ColumnHeader + animated card list |
| `src/lib/components/ColumnHeader.svelte` | Pure: uppercase label + count badge |
| `src/lib/components/RunCard.svelte` | Composition root: `--status-color` inline, `data-status` PascalCase attribute, state-aware `$derived.by` duration, five-child tree, scoped `<style>` with `::before` accent bar. Phase 6a: derives `isFocused` from RovingFocusContext, applies `tabindex={isFocused ? 0 : -1}` to `.run-card-activate`, and runs a `$effect` that calls `.focus()` when `isFocused && kanbanHasFocus` — drives both arrow-key navigation and cross-column re-focus across crossfade. |
| `src/lib/components/StatusIcon.svelte` | Pure: 11-StatusKey exhaustive glyph; color inherited from parent's `--status-color` |
| `src/lib/components/JobHeader.svelte` | Pure: StatusIcon + displayTitle + duration row with tabular-nums |
| `src/lib/components/JobMeta.svelte` | Pure: `repo · branch` secondary line with null-branch elision |
| `src/lib/components/ProgressBar.svelte` | Pure: `role="progressbar"` with scaleX fill; `aria-valuetext="No jobs"` when total is 0 |
| `src/lib/components/RunnerLabel.svelte` | Pure: `⊞ summary` monospace line; null-summary elision |
| `src/lib/components/CommandPalette.svelte` | Connected: Cmd+K command palette (Bits UI Command.Dialog); reads paletteStore + runStore + runnerStore + uiStore; suppresses default auto-focus via `onOpenAutoFocus` and re-focuses the input in a tick; on close, returns focus to the panel's "Close detail panel" button via `onCloseAutoFocus` when a panel is open underneath. No `defer-otherwise-close` — sibling dialogs don't establish a Bits UI parent context, so the palette is simply the topmost close-layer in mount order (see `docs/architecture/frontend-app.md` § Sheet + Command Dialog Stacking) |
| `src/lib/components/PaletteSection.svelte` | Pure leaf: section header wrapper with optional item count badge |
| `src/lib/components/PaletteRunItem.svelte` | Pure leaf: run row (repo, branch, status glyph, duration) inside palette |
| `src/lib/components/PaletteJobItem.svelte` | Pure leaf: job row (job name, run context) inside palette |
| `src/lib/components/PalettePoolItem.svelte` | Pure leaf: runner pool row (sorted labels, running/queued counts) inside palette |
| `src/lib/components/PaletteCommandItem.svelte` | Pure leaf: command row (label + optional keyboard shortcut badge) inside palette |
| `src/lib/components/HoverPeekPopover.svelte` | Pure: 250ms-delayed popover anchored to RunCard; dismissed on mouse-leave or card click; touch-suppressed |
| `src/lib/components/PoolFilterPill.svelte` | Pure: active filter indicator rendered in TopBar; shows sorted labels + clear button; absent when no filter |
| `src/lib/components/RunDetailPanel.svelte` | Connected: slide-over Sheet panel (Bits UI Sheet); reads uiStore.selectedRunId + runStore; uses `escapeKeydownBehavior="defer-otherwise-close"` and `interactOutsideBehavior="defer-otherwise-close"` so a topmost dialog (palette) absorbs Esc/click-outside first; `open={run !== undefined}` (reactive, not hardcoded `true`) so Bits UI sees the `true→false` transition on close and fires `onCloseAutoFocus`; `{#if run}` is inside `Sheet.Content` for the same reason; `onCloseAutoFocus` restores focus to the originating RunCard's `.run-card-activate` button via `uiStore.lastTriggerRunId`, or falls back to `ctx.restoreFocusToInitial()` (via `getRovingContext()`) when the trigger card was evicted while the panel was open. Phase 6a: `onCloseAutoFocus` rewritten to route the absent-trigger case through `ctx.restoreFocusToInitial()` instead of silently no-opping (was a latent bug where focus stranded on `<body>` when the trigger card had been TTL-evicted). |
| `src/lib/components/PanelHeader.svelte` | Pure leaf: panel title row with StatusIcon, repo/branch, run number; `data-status-key` attribute |
| `src/lib/components/PanelActions.svelte` | Pure leaf: "Close detail panel" button (aria-label stable selector for focus restoration) + "Go to run" link |
| `src/lib/components/MetaGrid.svelte` | Pure leaf: two-column definition-list grid of key/value metadata pairs |
| `src/lib/components/MetaCell.svelte` | Pure leaf: single labeled cell inside MetaGrid |
| `src/lib/components/JobBlock.svelte` | Pure leaf: expandable job section (status, name, duration) with scroll-into-view when selectedJobId matches |
| `src/lib/components/StepList.svelte` | Pure leaf: ordered list of steps inside a JobBlock |
| `src/lib/components/StepItem.svelte` | Pure leaf: single step row (outcome glyph, name, duration) |
| `src/lib/components/roving/` | Roving-tabindex module: `context.ts` (Svelte context shape, throwing accessor), `geometry.ts` (pure 2D nav resolution), `action.ts` (Svelte 5 action: focusin/focusout/keydown), `RovingFocusProvider.svelte` (context-only wrapper). First production use of Svelte 5 actions and `setContext`/`getContext`. See `docs/architecture/frontend-app.md` § Roving Focus for architecture. |
| `src/lib/components/roving/RovingFocusProvider.svelte` | Context-only wrapper around `AppShell` + `CommandPalette` + `RunDetailPanel` in App.svelte. Owns `focusedRunId` / `kanbanHasFocus` `$state`, derives `visibleColumns: Columns` (filtered via `filterRunsByPool` + `uiStore.activePoolFilter` — single source of truth matching the DOM), `initialFocusRunId` / `currentFocusRunId` `$derived`, and the eviction `$effect` that detects `locate(focusedRunId, visibleColumns) === null`; gates DOM focus restoration on `kanbanHasFocus` to prevent background eviction from yanking focus. Exposes `getVisibleColumns()` via context. |
| `src/lib/animations/kanban-transitions.ts` | Shared crossfade instance, motion constants, reduced-motion support |
| `src/lib/format/duration.ts` | Pure: `formatDuration({kind:'static'|'live', ...})` — `MM:SS` under 1h, `H:MM:SS` at or above |
| `src/lib/format/duration-text.ts` | Pure: `computeDurationText(run, nowMs): string` — state-aware formula called by RunCard's `$derived.by` |
| `src/lib/format/runners.ts` | Pure: `summarizeRunners(jobs)` — single / `N runners` / null branches |
| `src/lib/format/status-key.ts` | Pure: `StatusKey` union (11 values) + `resolveStatusKey(run)` |
| `src/lib/design-tokens.test.ts` | WCAG contrast gate: 11 status tokens × 4 themes × 2 modes against `--surface` |
| `e2e/lib/ws-mock.ts` | Shared Playwright harness: `makeRunEvent`, `makeJobSeqEvent`, `sendWS`, `sendWSBatch` (routes Job and Run events through `window.eventDispatcher` bridge). Phase 3b: `makeJobSeqEvent` no longer emits a `poolStatsAfter` sidecar — pool state is now derived from job mutations |
| `e2e/` | Playwright E2E tests (see directory: theme, app-shell, kanban, run-cards, run-card-interactivity, palette, pool-filter, pool-indicators, run-detail-panel, stacking) |

## Store Additions

**Sub-Phase 4:**

- `uiStore.nowMs` — `$state<number>` refreshed every 1s by a constructor `setInterval` (mirrors `connectionStore.tick`). Shared signal that feeds every live-duration derivation; single timer replaces per-card intervals. `uiStore.destroy()` clears it (used by tests only).
- `runStore.jobStatsByRun` — `$derived.by<ReadonlyMap<bigint, JobStats>>` total-map aggregate iterating `this.runs.keys()` so every known run resolves to a `JobStats` entry (completed/total/runnerSummary), even when the run has no jobs yet. Consumers never need to fall back.

**Phase 3b (frontend pool-stats derivation):**

- `runStore.jobs` — `$derived.by<Job[]>` flat view across `jobsByRun.values()`. Single stable dependency for the runner-pool derivation. The pool-derivation chain is `runStore.jobsByRun` → `runStore.jobs` → `runnerStore.pools`.
- `runnerStore.pools` — `readonly pools = $derived.by(() => computePoolStats(runStore.jobs))`. Replaces the previous `$state` array + `loadPools()`/`clear()` API. Pool state self-heals on every job event without explicit dispatch.
- `computePoolStats(jobs: Job[]): RunnerPoolStats[]` — exported pure function in `runners.svelte.ts`. Skips `Waiting`/`Completed` jobs, groups by sorted-label set (using `JSON.stringify(sortedLabels)` as map key), increments `queued`/`running` per job status, derives `groupName` from latest observed runner, sets `isElastic` when any runner has `groupId === 0n` (bigint-aware), sorts result lexicographically by labels.

## Status

Complete through Sub-Phase 6b (polish + responsive) — all frontend sub-phases for the 1.0 release are complete. App shell with TopBar (logo, runner pool indicators with active-filter highlight, PoolFilterPill, connection indicator, settings popover). Kanban board with three-column view (responsive: 1 col <640px, 2 cols 640–1279px, 3 cols ≥1280px), card animations via shared crossfade, sorted derived arrays. RunCard composes five leaves with `--status-color` inline, state-aware `$derived.by` duration, halo animation on InProgress cards, hover-peek popover (HoverPeekPopover), and keyboard-activatable inner button for panel-open. Sub-Phase 5 added: Cmd+K command palette, slide-over run detail panel, pool filter integration, and Bits UI dialog stacking. Sub-Phase 6a (kanban keyboard navigation) added: `<RovingFocusProvider>` with 2D arrow + Home/End navigation, card-stable focus through FLIP/crossfade, and lost-trigger restoration on panel close. Sub-Phase 6b added: `EmptyState` component (schematic-preview treatment), responsive kanban/TopBar/RunnerPool breakpoints, reduced-motion audit (CommandPalette submenu slide gated), global `.atc-scrollbar` styling, `:focus-visible` rules on four custom elements, and the `lib/aria/` module — `LiveRegion` rune-class store with `BurstAccumulator` (announces `RunEvent::Requested` and `RunEvent::Completed` via `EventDispatcher.setOnFlush` callback hook), `AriaLiveRegion.svelte` (`role="status" aria-live="polite" aria-atomic="true" aria-busy`). Performance verification: Tier 1 deterministic RAF-coalescing gate (`dispatcher.perf.browser.test.ts`, 1000-event burst, exactly 10 flush callbacks, CI hard fail) and Tier 2 informational frame-budget trace artifact (`frame-budget.test.ts`, `test-results/frame-budget-trace.json`).

Phase 3a/3b (state externalization, wire contract): the snapshot cursor renamed to `lastSeq` (highest committed seq; `0` is the cold-start sentinel) and `connection.ts` filters buffered events with `seq > lastSeq` (was `>=`). Runner pool stats are now derived on the frontend — `runnerStore.pools = $derived.by(() => computePoolStats(runStore.jobs))` consumes a new `runStore.jobs` flat view; the `SeqEvent.poolStatsAfter` sidecar and `StateSnapshot.poolStats` field were removed in lockstep with the backend. See ADRs 0003 and 0004 in `docs/architecture-decisions/`.

See `docs/architecture/frontend-app.md` for the full architecture, store-ceiling rationale, new design tokens, AriaLiveRegion module, and performance verification methodology. All 756 unit/browser tests + 147 E2E tests passing.

## Commands

```bash
pnpm dev          # Dev server with HMR
pnpm build        # Production build to dist/
pnpm check        # svelte-check type checking
pnpm lint         # Biome (ts/js) + eslint-plugin-svelte (.svelte)
pnpm format       # Biome + prettier-plugin-svelte
pnpm test         # Vitest unit + browser tests
pnpm test:e2e     # Playwright E2E tests
```

## Key References

- Architecture: `docs/architecture/frontend-app.md`
- Design system config: `.impeccable.md`
