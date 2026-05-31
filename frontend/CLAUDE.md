# CLAUDE.md — frontend

Last verified: 2026-05-23

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed. The only frontend in the workspace.

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

## Sharp edges

**Typed-union switches need runtime exhaustiveness, not just compile-time.** Functions that `switch` over a generated typed union (e.g. `RunConclusion`, `StatusKey`) MUST include a `default: const _: never = value; throw new Error(...)` branch. Without it, off-shape values from boundaries — test fixtures with loose `Record<string, unknown>` typing, JSON over the wire, raw `evaluate` injections in Playwright — silently return `undefined` and cascade into broken downstream renders that look like reactivity bugs.

**E2E imports must come from `e2e/lib/fixtures.ts`, not `@playwright/test` directly.** That fixtures file re-exports `test`/`expect` from `@bgotink/playwright-coverage` so the V8 coverage capture hook fires on every test. A direct `@playwright/test` import bypasses the hook and that test's coverage silently drops to zero.

**E2E fixture typing in `e2e/lib/ws-mock.ts`.** Helpers like `makeRunEvent` / `makeJobCommittedEvent` should use the generated discriminated-union types, not `Record<string, unknown>`, so casing mismatches surface at edit time rather than after a multi-hour debug.

**`src/lib/test-utils/factories.ts` must stay value-import-free.** E2E specs import the factories by relative path (`../src/lib/test-utils/factories`) because Playwright resolves relative imports but not the `$lib` tsconfig alias for *value* imports. The factory module works there only because all its imports are `import type` (erased at transpile, so nothing hits the resolver). Add a single value import — e.g. a shared default constant or a runtime enum — and every e2e spec throws a module-resolution error at Playwright runtime with **no compile-time signal** (svelte-check and vitest use Vite, which resolves `$lib`, so they stay green; the break only surfaces in the separate Playwright CI job). Keep shared defaults inside the factory file. See `docs/architecture/frontend-app.md` § Test Strategy.

**URL ↔ `selectedRunId` sync.** Two guards govern the loop: `initialUrlPending` (suppresses outbound writes until the first snapshot lands) and a **semantic** `parseRunIdFromUrl(...) === uiStore.selectedRunId` comparison (suppresses popstate echoes and tolerates non-canonical encoding of unrelated query params). A string-equality guard would treat `?q=my%20term` vs `?q=my+term` as different and push a spurious history entry on hydration. See `docs/architecture/frontend-app.md` § App Shell URL sync for the full mechanism.

Debugging heuristic: when a page becomes unresponsive after a `page.evaluate(...)` store mutation and the snapshot shows an unrelated empty state, suspect a downstream render error from a mismatched value shape, NOT a reactivity propagation bug.

## Key References

- Architecture: `docs/architecture/frontend-app.md`
- Design system config: `.impeccable.md`
- ADRs: `docs/architecture-decisions/0001-pool-key-branded-type.md`, `0003-state-cursor-contract-and-operator-policy.md`, `0004-frontend-derived-pool-stats.md`
