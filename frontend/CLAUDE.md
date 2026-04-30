# CLAUDE.md — frontend

Last verified: 2026-04-29

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component: mounts ConnectionManager + AppShell, plus CommandPalette and RunDetailPanel as portal-target siblings; owns the global `window` keydown listener that calls `paletteStore.toggle()` on Cmd/Ctrl+K (skipping editable contexts outside the palette input) |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point; exports stores to `window.__stores` bridge for E2E test harness |
| `src/vite-env.d.ts` | Window type augmentation for `__stores` bridge (runStore, connectionStore, runnerStore, uiStore, paletteStore, poolKey) |
| `vite.config.ts` | Build config with Tailwind v4 and Svelte plugins |
| `vitest.config.ts` | Vitest workspace config (delegates to unit and browser projects) |
| `vitest.config.unit.ts` | Vitest unit project (jsdom, `*.test.ts`) |
| `vitest.config.browser.ts` | Vitest browser project (Playwright chromium, `*.browser.test.ts`) |
| `playwright.config.ts` | Playwright E2E test configuration with webServer auto-start |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts`, `palette.svelte.ts` |
| `src/lib/stores/palette.svelte.ts` | PaletteStore — `paletteOpen`, `paletteQuery`, `recentRunIds`; separate store for high-frequency typing state and recent-items lifecycle (see `docs/architecture/frontend-app.md`) |
| `src/lib/filters/pool.ts` | `PoolKey` branded type + `poolKey()` factory + `filterRunsByPool()` — first branded TypeScript type in the codebase; see ADR `docs/architecture-decisions/0001-pool-key-branded-type.md` |
| `src/lib/dispatcher.ts` | EventDispatcher — routes primitive WebSocket events to stores; applies `SeqEvent.poolStatsAfter` sidecar to `runnerStore` when present; batches via requestAnimationFrame |
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
| `src/lib/components/RunCard.svelte` | Composition root: `--status-color` inline, `data-status` PascalCase attribute, state-aware `$derived.by` duration, five-child tree, scoped `<style>` with `::before` accent bar |
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
| `src/lib/components/RunDetailPanel.svelte` | Connected: slide-over Sheet panel (Bits UI Sheet); reads uiStore.selectedRunId + runStore; uses `escapeKeydownBehavior="defer-otherwise-close"` and `interactOutsideBehavior="defer-otherwise-close"` so a topmost dialog (palette) absorbs Esc/click-outside first; restores focus to the originating RunCard's `.run-card-activate` button via `uiStore.lastTriggerRunId` |
| `src/lib/components/PanelHeader.svelte` | Pure leaf: panel title row with StatusIcon, repo/branch, run number; `data-status-key` attribute |
| `src/lib/components/PanelActions.svelte` | Pure leaf: "Close detail panel" button (aria-label stable selector for focus restoration) + "Go to run" link |
| `src/lib/components/MetaGrid.svelte` | Pure leaf: two-column definition-list grid of key/value metadata pairs |
| `src/lib/components/MetaCell.svelte` | Pure leaf: single labeled cell inside MetaGrid |
| `src/lib/components/JobBlock.svelte` | Pure leaf: expandable job section (status, name, duration) with scroll-into-view when selectedJobId matches |
| `src/lib/components/StepList.svelte` | Pure leaf: ordered list of steps inside a JobBlock |
| `src/lib/components/StepItem.svelte` | Pure leaf: single step row (outcome glyph, name, duration) |
| `src/lib/animations/kanban-transitions.ts` | Shared crossfade instance, motion constants, reduced-motion support |
| `src/lib/format/duration.ts` | Pure: `formatDuration({kind:'static'|'live', ...})` — `MM:SS` under 1h, `H:MM:SS` at or above |
| `src/lib/format/duration-text.ts` | Pure: `computeDurationText(run, nowMs): string` — state-aware formula called by RunCard's `$derived.by` |
| `src/lib/format/runners.ts` | Pure: `summarizeRunners(jobs)` — single / `N runners` / null branches |
| `src/lib/format/status-key.ts` | Pure: `StatusKey` union (11 values) + `resolveStatusKey(run)` |
| `src/lib/design-tokens.test.ts` | WCAG contrast gate: 11 status tokens × 4 themes × 2 modes against `--surface` |
| `e2e/lib/ws-mock.ts` | Shared Playwright harness: `makeRunEvent`, `makeJobSeqEvent` (with `poolStatsAfter` sidecar), `sendWS` (routes Job and Run events through `window.__stores` bridge) |
| `e2e/` | Playwright E2E tests (see directory: theme, app-shell, kanban, run-cards, run-card-interactivity, palette, pool-filter, pool-indicators, run-detail-panel, stacking) |

## Store Additions (Sub-Phase 4)

- `uiStore.nowMs` — `$state<number>` refreshed every 1s by a constructor `setInterval` (mirrors `connectionStore.tick`). Shared signal that feeds every live-duration derivation; single timer replaces per-card intervals. `uiStore.destroy()` clears it (used by tests only).
- `runStore.jobStatsByRun` — `$derived.by<ReadonlyMap<bigint, JobStats>>` total-map aggregate iterating `this.runs.keys()` so every known run resolves to a `JobStats` entry (completed/total/runnerSummary), even when the run has no jobs yet. Consumers never need to fall back.

## Status

Complete through Sub-Phase 5 (interactivity). App shell with TopBar (logo, runner pool indicators with active-filter highlight, PoolFilterPill, connection indicator, settings popover). Kanban board with three-column view, card animations via shared crossfade, sorted derived arrays. RunCard composes five leaves with `--status-color` inline, state-aware `$derived.by` duration, halo animation on InProgress cards, hover-peek popover (HoverPeekPopover), and keyboard-activatable inner button for panel-open. Sub-Phase 5 added: Cmd+K command palette (CommandPalette + pure palette leaves), slide-over run detail panel (RunDetailPanel + panel leaves: PanelHeader/PanelActions/MetaGrid/MetaCell/JobBlock/StepList/StepItem), pool filter integration (PoolFilterPill, PoolKey branded type), and Bits UI dialog stacking with `defer-otherwise-close` semantics and single-backdrop CSS suppression. See `docs/architecture/frontend-app.md` for dialog stacking pattern, store-ceiling rationale, and new design tokens. All 547 unit/browser tests + 79 E2E tests passing.

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
