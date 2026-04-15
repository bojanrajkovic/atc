# CLAUDE.md — frontend

Last verified: 2026-04-14

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component with theme/mode switching demo |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point |
| `vite.config.ts` | Build config with Tailwind v4 and Svelte plugins |
| `vitest.config.ts` | Vitest configuration (jsdom environment for unit tests) |
| `playwright.config.ts` | Playwright E2E test configuration with webServer auto-start |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts` |
| `src/lib/dispatcher.ts` | EventDispatcher — buffers WebSocket events and flushes to stores via requestAnimationFrame |
| `src/lib/connection.ts` | ConnectionManager — WS-first protocol with pre-connect buffering and exponential backoff reconnect |
| `src/lib/types/generated/` | ts-rs generated TypeScript types from Rust (do not hand-edit) |
| `e2e/` | Playwright E2E tests (theme rendering, switching, mode toggle) |

## Status

Phase 5 complete. App foundation infrastructure established: OKLCH design system with four themes, dark/light mode, Svelte 5 stores with WS client, event dispatcher with RAF batching, comprehensive unit tests (Vitest), and E2E tests (Playwright) verifying rendering and theming. Component hierarchy skeleton in place but feature implementation deferred to next phase.

## Commands

```bash
pnpm dev          # Dev server with HMR
pnpm build        # Production build to dist/
pnpm check        # svelte-check type checking
pnpm lint         # Biome (ts/js) + eslint-plugin-svelte (.svelte)
pnpm format       # Biome + prettier-plugin-svelte
pnpm test         # Vitest unit tests + Playwright E2E tests
```

## Key References

- Architecture: `docs/architecture/frontend-app.md`
- Design system config: `.impeccable.md`
