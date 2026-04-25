# CLAUDE.md — frontend

Last verified: 2026-04-24

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component: mounts ConnectionManager and AppShell |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point; exports stores to `window.__stores` bridge for E2E test harness |
| `src/vite-env.d.ts` | Window type augmentation for `__stores?: { runStore?, connectionStore?, runnerStore? }` |
| `vite.config.ts` | Build config with Tailwind v4 and Svelte plugins |
| `vitest.config.ts` | Vitest workspace config (delegates to unit and browser projects) |
| `vitest.config.unit.ts` | Vitest unit project (jsdom, `*.test.ts`) |
| `vitest.config.browser.ts` | Vitest browser project (Playwright chromium, `*.browser.test.ts`) |
| `playwright.config.ts` | Playwright E2E test configuration with webServer auto-start |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts` |
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
| `src/lib/animations/kanban-transitions.ts` | Shared crossfade instance, motion constants, reduced-motion support |
| `src/lib/format/duration.ts` | Pure: `formatDuration({kind:'static'|'live', ...})` — `MM:SS` under 1h, `H:MM:SS` at or above |
| `src/lib/format/duration-text.ts` | Pure: `computeDurationText(run, nowMs): string` — state-aware formula called by RunCard's `$derived.by` |
| `src/lib/format/runners.ts` | Pure: `summarizeRunners(jobs)` — single / `N runners` / null branches |
| `src/lib/format/status-key.ts` | Pure: `StatusKey` union (11 values) + `resolveStatusKey(run)` |
| `src/lib/design-tokens.test.ts` | WCAG contrast gate: 11 status tokens × 4 themes × 2 modes against `--surface` |
| `e2e/lib/ws-mock.ts` | Shared Playwright harness: `makeRunEvent`, `makeJobSeqEvent` (with `poolStatsAfter` sidecar), `sendWS` (routes Job and Run events through `window.__stores` bridge) |
| `e2e/` | Playwright E2E tests (theme, app shell, kanban board, run cards) |

## Store Additions (Sub-Phase 4)

- `uiStore.nowMs` — `$state<number>` refreshed every 1s by a constructor `setInterval` (mirrors `connectionStore.tick`). Shared signal that feeds every live-duration derivation; single timer replaces per-card intervals. `uiStore.destroy()` clears it (used by tests only).
- `runStore.jobStatsByRun` — `$derived.by<ReadonlyMap<bigint, JobStats>>` total-map aggregate iterating `this.runs.keys()` so every known run resolves to a `JobStats` entry (completed/total/runnerSummary), even when the run has no jobs yet. Consumers never need to fall back.

## Status

Complete infrastructure, kanban board, and run cards. App shell with TopBar (logo, runner pool indicators, connection indicator, settings popover). Kanban board with three-column view, card animations via shared crossfade, sorted derived arrays. RunCard composes five leaves (StatusIcon/JobHeader/JobMeta/ProgressBar/RunnerLabel) with `--status-color` inline, `data-status` attribute, state-aware `$derived.by` duration (static-Completed does not subscribe to tick), 3px `::before` accent bar, halo animation on InProgress cards, compact-density CSS, and a three-file test split (jsdom composition + jsdom reactivity proof + browser-mode computed-style). All 302 unit/browser tests + 26 E2E tests passing.

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
