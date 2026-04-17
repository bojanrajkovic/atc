# CLAUDE.md — frontend

Last verified: 2026-04-16

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component: mounts ConnectionManager and AppShell |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point |
| `vite.config.ts` | Build config with Tailwind v4 and Svelte plugins |
| `vitest.config.ts` | Vitest workspace config (delegates to unit and browser projects) |
| `vitest.config.unit.ts` | Vitest unit project (jsdom, `*.test.ts`) |
| `vitest.config.browser.ts` | Vitest browser project (Playwright chromium, `*.browser.test.ts`) |
| `playwright.config.ts` | Playwright E2E test configuration with webServer auto-start |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts` |
| `src/lib/dispatcher.ts` | EventDispatcher — buffers WebSocket events and flushes to stores via requestAnimationFrame |
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
| `src/lib/components/KanbanBoard.svelte` | Connected: reads RunStore + ConnectionStore, renders tri-state (loading/empty/kanban grid) |
| `src/lib/components/KanbanColumn.svelte` | Pure: receives sorted runs array, renders ColumnHeader + animated card list |
| `src/lib/components/ColumnHeader.svelte` | Pure: uppercase label + count badge |
| `src/lib/components/RunCard.svelte` | Pure: skeleton card with displayTitle + accessible status indicator |
| `src/lib/animations/kanban-transitions.ts` | Shared crossfade instance, motion constants, reduced-motion support |
| `e2e/` | Playwright E2E tests (theme rendering, switching, mode toggle, app shell rendering) |

## Status

Complete infrastructure and kanban board. App shell with TopBar (logo, runner pool indicators, connection indicator, settings popover), AppShell layout (100dvh flex column), ConnectionManager service component. Kanban board with three-column view (queued/in-progress/completed), card animations via shared crossfade instance, sorted derived arrays in RunStore (ascending/descending/tiebreaker), unit + browser + E2E test coverage. All tests passing.

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
