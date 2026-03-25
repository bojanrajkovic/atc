# Frontend App — Architecture

Last verified: 2026-03-25

## Purpose

The frontend app is a standalone Svelte 5 single-page application built with Vite. It provides:

- The user interface for the ATC dashboard
- A complete OKLCH-based design system with four themes and dark/light mode
- A static build output (`frontend/dist/`) that the backend embeds into its release binary

In the skeleton phase, the app renders a hello-world component demonstrating that the Tailwind + OKLCH token system works. No application features or API integration yet.

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

## Boundaries

**Owns:** UI rendering, design tokens (OKLCH system), theme switching, Tailwind configuration, Svelte component structure, frontend build output
**Does not own:** API communication (future phase), state management (future phase), routing (future phase), backend serving logic
**Prohibitions:** Do not import backend code. Do not add SvelteKit. Do not use PostCSS for Tailwind (use @tailwindcss/vite). Do not let Biome process .svelte files (use eslint/prettier for those).

## Files

- `frontend/src/main.ts` — Svelte mount point
- `frontend/src/App.svelte` — Root component with theme switching demo
- `frontend/src/app.css` — Tailwind import, OKLCH design system tokens, theme definitions, base styles
- `frontend/src/vite-env.d.ts` — Vite/Svelte type references
- `frontend/index.html` — HTML entry point
- `frontend/vite.config.ts` — Vite config with Tailwind and Svelte plugins
- `frontend/svelte.config.js` — Svelte preprocessor config
- `frontend/tsconfig.json` — TypeScript configuration
- `frontend/biome.json` — Biome lint/format config for .ts/.js
- `frontend/eslint.config.mjs` — ESLint config for .svelte files
- `frontend/.prettierrc` — Prettier config for .svelte files
- `frontend/package.json` — Dependencies with catalog: references
- `frontend/pnpm-workspace.yaml` — pnpm workspace with catalog version pins
