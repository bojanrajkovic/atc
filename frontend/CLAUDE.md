# CLAUDE.md — frontend

Last verified: 2026-04-14

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed.

## Key Files

| File | Role |
|------|------|
| `src/App.svelte` | Root component |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions |
| `src/main.ts` | Vite entry point |
| `vite.config.ts` | Build config, Tailwind plugin |
| `src/lib/stores/` | Svelte 5 rune stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts` |
| `src/lib/dispatcher.ts` | EventDispatcher — buffers WebSocket events and flushes to stores via requestAnimationFrame |
| `src/lib/connection.ts` | ConnectionManager — WS-first protocol with pre-connect buffering and exponential backoff reconnect |

## Status

Phase 4 complete. WebSocket client with WS-first connect protocol, event buffering, and reconnect backoff. EventDispatcher batches events via RAF. Stores initialized but component hierarchy not yet built.

## Commands

```bash
pnpm dev          # Dev server with HMR
pnpm build        # Production build to dist/
pnpm check        # svelte-check type checking
pnpm lint         # Biome (ts/js) + eslint-plugin-svelte (.svelte)
pnpm format       # Biome + prettier-plugin-svelte
```

## Key References

- Architecture: `docs/architecture/frontend-app.md`
- Design system config: `.impeccable.md`
