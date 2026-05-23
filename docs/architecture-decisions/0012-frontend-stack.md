# ADR 0012 — Svelte 5 + Vite + Tailwind v4 + OKLCH for the frontend stack

Date: 2026-05-23
Status: Accepted

Last verified: 2026-05-23

## Context

ATC is a real-time GitHub Actions dashboard: a Rust/Axum backend that ingests webhooks and pushes state over WebSocket, and a browser frontend that renders what it receives. The frontend is a pure consumer of server-pushed data — it never queries GitHub directly, never does SSR, and has no server-side rendering requirement. The entire rendered surface is a live kanban of in-flight workflow runs and jobs.

During ATC's ideation phase (early 2026), the frontend architecture was chosen as part of the initial setup before active feature development began. The selection covered four interdependent concerns: SPA framework, build tool, CSS framework, and color model. This ADR records those choices and the alternatives that were considered and rejected.

The selection was driven by three hard constraints that narrowed the field significantly:

1. **Real-time, fine-grained reactivity.** The dashboard must handle high-frequency WebSocket bursts (many jobs starting at once) with sub-frame batching and surgical DOM updates — rebuilding the card tree on every tick was a known performance failure mode from the prototype.

2. **Bespoke single-hue OKLCH design system.** ATC's visual language (four neutral-hue themes, dark/light mode, high-chroma status colors) derives all surface, text, and border tokens from a single `--hue` CSS variable. Any CSS framework with its own theme layer would create a collision.

3. **Standalone SPA, separate from the Rust backend.** The frontend builds to a static `dist/` tree that the backend embeds via rust-embed. There is no Node.js runtime in production; the frontend is not a server.

## Decision

### Framework: Svelte 5

Svelte 5 is the SPA framework. Its rune system (`$state`, `$derived`, `$effect`) provides the fine-grained reactivity model needed for the real-time use case: a WebSocket message batch can be applied to a rune-class store in a single RAF flush, and `$derived` columns recompute only when their specific slice of state changes. The compile-time-no-runtime model keeps the deployed bundle small relative to virtual-DOM alternatives.

The evaluation considered React, Vue, and SvelteKit alongside Svelte 5. See Rejected Alternatives.

Component primitives come from **shadcn-svelte** (built on Bits UI), chosen because it uses the copy-into-project model — component source lives in the repo and is fully owned. This lets ATC's OKLCH tokens be applied directly without fighting a framework's token namespace. The component catalog covers the dashboard's accessibility-critical needs (command palette, collapsibles, tooltips, focus management) without requiring a separate headless layer.

### Build tool: Vite

Vite is the build tool and dev server. The `@sveltejs/vite-plugin-svelte` handles Svelte compilation; HMR during development is fast. Vite's proxying capability is used in dev mode to forward API requests to the Rust backend, so the frontend can run independently without CORS complications.

### CSS framework: Tailwind v4

Tailwind v4 is the CSS framework, integrated via its first-party Vite plugin (`@tailwindcss/vite`). The v4 Vite plugin eliminates the PostCSS pipeline entirely — Tailwind is processed as a Vite transform rather than as a PostCSS step. This simplifies the build graph and removes the `postcss.config.*` file from the project entirely. Design tokens are declared in CSS using Tailwind v4's `@theme` directive, which keeps OKLCH custom properties in CSS (where they belong) rather than in a JavaScript config file.

### Color model: OKLCH, single-hue

The color system uses OKLCH throughout. All neutral surfaces, text, and border tokens derive from a single `--hue` CSS custom property via `oklch()` function calls. Switching between ATC's four themes (Warm, Radar, Violet, Pink) changes one value; all derived tokens recompute automatically in the browser. Status colors (queued, running, success, failed, etc.) use fixed hues independent of the theme — they are perceptually consistent semantic indicators, not part of the neutral ramp.

The single-hue derivation strategy means the entire neutral ramp is coherent across dark and light modes without managing parallel token sets. WCAG AA contrast (≥ 4.5:1) for all status tokens against `--surface` is enforced by an automated test that runs on every build.

### Lint/format toolchain: Biome + ESLint + Prettier

Biome handles JS and TS linting and formatting. Because Biome does not yet support `.svelte` files, ESLint (with `eslint-plugin-svelte` and `svelte-eslint-parser`) handles `.svelte`-specific linting, and `prettier-plugin-svelte` handles `.svelte` formatting. This split — Biome for pure JS/TS, ESLint + Prettier for Svelte files — avoids forcing a single tool across the syntax boundary it cannot cross.

## Rejected alternatives

### SvelteKit (full-stack Svelte with SSR + file-based routing)

SvelteKit was the natural comparison point because it is the officially recommended way to build Svelte applications. It would have provided SSR, a file-based router, and API route support within the same tree.

ATC does not need SSR — the dashboard is not indexable content and benefits from no initial-render latency advantage. More importantly, SvelteKit would have collapsed the frontend and Rust backend into one architectural unit, or required a separate Node.js adapter process in production. The chosen shape — Rust binary embeds a static SPA — gives operators a single deployment artifact with no Node.js runtime dependency. Adding SvelteKit's Node or Bun adapter to serve the frontend would have created a two-runtime deployment (Rust + Node) with no functional gain.

Rejected: SSR is unnecessary for a real-time dashboard, and the deployment shape benefit (single Rust binary) outweighed any SvelteKit convenience.

### React

React is the dominant frontend framework by ecosystem size and familiarity. Its ecosystem (React Query, Zustand, React Spring, Framer Motion) is mature. The tooling investment is low for most teams.

Two factors weighed against it here. First, Svelte 5's rune system offers finer-grained reactivity without a virtual DOM: `$derived` columns react only to their specific state slice, while React's component re-render model requires `useMemo`/`useCallback` to achieve comparable isolation — adding boilerplate for the same outcome. Second, the dashboard was being built during the Svelte 5 runes release cycle, and targeting the new primitive was a deliberate capability bet on compile-time reactivity without a runtime. ATC's use case — high-frequency WebSocket updates, minimal branching UI logic — is well-suited to Svelte's fine-grained model.

Rejected: higher per-component boilerplate, larger runtime, and no advantage for this specific use case over Svelte 5 runes.

### Vue 3

Vue 3 (Composition API) is comparable in scope and performance to Svelte 5. Its reactivity model, `<script setup>`, and single-file components are mature. Nuxt 3 would have provided a SvelteKit-equivalent path.

The deciding factor was timing: Svelte 5's runes were newly released and represented a compile-time-no-runtime reactivity model that Vue's runtime-based `ref`/`reactive` does not match. For a high-frequency dashboard, the performance ceiling difference mattered. The choice was partially subjective — Vue is viable — but Svelte's compile-time approach was the target capability for this project.

Rejected: functionally viable, but Svelte 5's compile-time model and runes were the preferred target capability.

### Tailwind v3

Tailwind v3 was the stable, widely deployed version at decision time. Tailwind v4 was newer (early release / beta) and its Vite plugin integration was not yet as widely adopted.

The deciding trade-off was the Vite plugin. Tailwind v4's first-party `@tailwindcss/vite` plugin removes PostCSS from the pipeline entirely: no `postcss.config.js`, no PostCSS transform step, just a Vite plugin. For a project with a custom CSS-variable-based design system (OKLCH tokens in an `@theme` block), having the CSS processed directly by Vite — rather than through PostCSS as a separate tool — reduces the number of moving parts. The v4 `@theme` directive also lets design tokens live in CSS natively, which is where the OKLCH `oklch()` function calls belong.

Rejected: v3 is stable but requires a PostCSS pipeline and a JS config file for tokens; v4's Vite plugin and native CSS config were cleaner for this design system.

### HSL / RGB / sRGB color models

HSL is the most common perceptually-motivated color model used in CSS design systems. RGB and sRGB are the underlying color spaces browsers have historically operated in.

Neither model provides perceptual uniformity across hues: two colors with the same HSL lightness value can appear visually very different brightnesses to the human eye (the classic example: yellow at 50% lightness looks far brighter than blue at 50% lightness). For a real-time dashboard where status colors (running amber, queued blue, failed red, success green) need to feel equally visible and equally important regardless of hue, perceptual uniformity matters. A viewer should not have to mentally compensate for one status color appearing washed out relative to another.

OKLCH's L axis is perceptually uniform: `oklch(72% 0.15 250)` (blue) and `oklch(72% 0.16 25)` (red) appear the same apparent brightness to human vision, enabling consistent legibility across hues at a single lightness value. This also makes the single-hue neutral ramp predictable: incrementing or decrementing the lightness value produces visually even steps regardless of which theme hue is selected.

Rejected: HSL/RGB lack perceptual uniformity across hues, making consistent visual weight across status colors and theme ramp steps difficult to achieve without per-hue manual tuning.

## Consequences

- **Deployment simplicity:** The frontend builds to a static `dist/` that the Rust binary embeds at compile time. Production deployments are a single binary with no Node.js runtime.
- **OKLCH everywhere:** Adding new semantic color tokens, new themes, or new status states requires only a `--hue`-compatible `oklch()` value. The contrast gate test at build time enforces WCAG AA compliance automatically; failures are caught before merge.
- **Biome/ESLint split is durable until Biome adds Svelte support:** When Biome ships Svelte support, ESLint and prettier-plugin-svelte can be retired, reducing the toolchain to one linter and one formatter.
- **Tailwind v4 Vite plugin immaturity:** At adoption time, the `@tailwindcss/vite` plugin had some rough edges in dev mode (a known interaction with Playwright coverage source-map requests). These were documented and worked around; the core build path was stable.
- **Svelte 5 runes are the reactivity primitive:** All new stores should use the rune-class pattern (`class Foo { value = $state(...); derived = $derived(...) }`), not the Svelte 4 `writable`/`derived` store API.

## References

- Ideation: [`docs/ideation/architecture-research.md`](../ideation/architecture-research.md) — webhook-backend + SPA architecture
- Ideation: [`docs/ideation/design-research.md`](../ideation/design-research.md) — visual language, Concourse/Linear references, animation requirements
- UI decomposition: [`docs/ideation/ui-decomposition/framework-evaluation.md`](../ideation/ui-decomposition/framework-evaluation.md) — shadcn-svelte vs Skeleton vs Melt UI vs Bits UI evaluation
- Design system config: [`.impeccable.md`](../../.impeccable.md) — brand, color tokens, type scale, accessibility targets
- Frontend architecture: [`docs/architecture/frontend-app.md`](../architecture/frontend-app.md) — component tree, store contracts, animation patterns
