# CLAUDE.md — frontend

Last verified: 2026-05-20

> Canonical documentation lives in `docs/architecture/frontend-app.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system. Produces a static build (`dist/`) that the backend embeds into its release binary via rust-embed. The only frontend in the workspace.

## Key Files

| File / Directory | Role |
|------------------|------|
| `src/App.svelte` | Root component; mounts ConnectionManager + AppShell + portal-target siblings (CommandPalette, RunDetailPanel); owns the global `window` keydown listener for Cmd/Ctrl+K, +D, +\\ |
| `src/app.css` | Design tokens (`@theme` block), OKLCH color definitions, base styles |
| `src/main.ts` | Vite entry point; exports stores to `window.__stores` for the E2E test harness |
| `vite.config.ts` | Vite + Tailwind v4 + Svelte plugins |
| `vitest.config.{ts,unit.ts,browser.ts}` | Vitest workspace + jsdom unit project + Playwright-chromium browser project; coverage to `coverage/vitest/lcov.info` |
| `playwright.config.ts` | Playwright E2E config; webServer auto-start; `@bgotink/playwright-coverage` reporter writes V8 coverage to `coverage/e2e/lcov.info` |
| `e2e/lib/fixtures.ts` | Re-exports `test`/`expect` from `@bgotink/playwright-coverage` so the V8 capture hook fires; all e2e tests import from here |
| `e2e/lib/ws-mock.ts` | Shared Playwright WebSocket harness: `makeRunEvent`, `makeJobCommittedEvent`, `sendWS`, `sendWSBatch`, `sendWSBatchPaced` (paces events across rAF / macrotask / interval boundaries to exercise dispatcher coalescing), `randomBatchSchedule` (seeded LCG for reproducible pacing schedules) |
| `src/lib/stores/` | Svelte 5 rune-class stores: `connection.svelte.ts`, `runs.svelte.ts`, `runners.svelte.ts`, `ui.svelte.ts`, `palette.svelte.ts`. `connection.svelte.ts:ConnectionStore` carries `configReloadError: string \| null` `$state` (issue #203) with `markConfigReloadError(reason)` (single-slot, last-wins; arms a 60s wall-clock auto-dismiss timer on the store, not on the component) and `dismissConfigReloadError()` (manual-close target, also clears the pending timer). `runs.svelte.ts:RunStore` carries a `runnerPoolCapacities: RunnerPoolCapacity[]` `$state` slice (wire field `capacity: number \| null`) replaced atomically by `loadSnapshot(runs, jobs, runnerPoolCapacities = [])` (third arg defaults to `[]` for rolling-deploy tolerance and back-compat with existing callers) and by `applyConfigUpdate(capacities)` — the hot-reload entry point invoked by the dispatcher when a `ConfigUpdate` WireFrame arrives (full capacity list, not a delta). `runners.svelte.ts:computePoolStats(jobs, capacities = [])` merges declared capacities into `RunnerPoolStats.total` as a `RunnerPoolTotal` adjacent-tagged enum: `{ kind: 'Bounded', value: n }` for declared integers, `{ kind: 'Unbounded' }` for declared `null`, `{ kind: 'Undeclared' }` for non-declared pools. Capacity lookup is keyed by canonical label-set via `poolKey()`. Runner-`groupId` values do not participate in classification. |
| `src/lib/dispatcher.ts` | EventDispatcher — accepts `WireFrame` and switches on the outer `kind` discriminator: `Committed` → existing RAF-batched store path; `ConfigUpdate` → `runStore.applyConfigUpdate(capacities)` (immediate, bypasses RAF); `ConfigReloadError` → `connectionStore.markConfigReloadError(reason)`. Also accepts a bare `CommittedEvent` for test ergonomics (auto-wraps as `{ kind: 'Committed', ...ev }`). `setOnFlush` post-flush hook for the ARIA live region applies only to the Committed batched path. |
| `src/lib/connection.ts` | ConnectionManager — WS-first protocol with pre-connect buffering and exponential backoff reconnect. Parses outer `WireFrame`s. Pre-snapshot: Committed frames buffer to `preConnectBuffer: CommittedEvent[]` (existing seq-keyed replay); `ConfigUpdate` overwrites a single `pendingConfigUpdate: RunnerPoolCapacity[] | null` slot (latest-wins, since the frame carries the full list); `ConfigReloadError` is dropped (informational only — the next successful reload repaints state). After `runStore.loadSnapshot` runs, the `pendingConfigUpdate` slot is drained via `runStore.applyConfigUpdate` so the race window between snapshot generation and snapshot fetch cannot lose a hot-reload. |
| `src/lib/url-state.ts` | Pure helpers for the `?run=<id>` deep-link surface: `parseRunIdFromUrl(url)`, `formatUrlForRunId(runId, currentUrl)` — both DOM-free, both work on the **relative URL** shape (`pathname + search + hash`). Consumed only by `src/App.svelte`. See `docs/architecture/frontend-app.md` § App Shell URL sync. |
| `src/lib/filters/pool.ts` | `PoolKey` branded type + `poolKey()` factory + `filterRunsByPool()` (ADR 0001) |
| `src/lib/types/generated/` | ts-rs generated TypeScript types from Rust (do not hand-edit; run `just types`) |
| `src/lib/components/` | Svelte components (TopBar, KanbanBoard, RunCard, CommandPalette, RunDetailPanel, RunnerBar, EmptyState, etc.). See `docs/architecture/frontend-app.md` § Component Tree for the hierarchy and contracts. |
| `src/lib/components/roving/` | Roving-tabindex module (Svelte 5 actions, `setContext`/`getContext`). See `frontend-app.md` § Roving Focus. |
| `src/lib/aria/` | ARIA live region module: `LiveRegion` rune-class store with `BurstAccumulator`; `AriaLiveRegion.svelte` |
| `src/lib/animations/kanban-transitions.ts` | Shared crossfade instance, motion constants, reduced-motion support |
| `src/lib/format/` | Pure formatters: `duration.ts`, `duration-text.ts`, `runners.ts`, `status-key.ts` |
| `src/lib/design-tokens.test.ts` | WCAG contrast gate: 11 status tokens × 4 themes × 2 modes against `--surface` |

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

## Sharp Edges

**Typed-union switches need runtime exhaustiveness, not just compile-time.** Functions that `switch` over a generated typed union (e.g. `RunConclusion`, `StatusKey` in `src/lib/format/status-key.ts`) MUST include a `default: const _: never = value; throw new Error(...)` branch. Without it, off-shape values from boundaries — test fixtures with loose `Record<string, unknown>` typing, JSON over the wire, raw `evaluate` injections in Playwright — silently return `undefined` and cascade into broken downstream renders that look like reactivity bugs.

**E2E fixture typing.** `frontend/e2e/lib/ws-mock.ts` helpers (`makeRunEvent`, `makeJobCommittedEvent`) should use the generated discriminated-union types, not `Record<string, unknown>`, so casing mismatches surface at edit time rather than after a multi-hour debug.

**URL ↔ `selectedRunId` sync.** Two flags govern the loop: `initialUrlPending` (suppresses outbound writes until the first snapshot lands) and the relative-URL `target === current` comparison (suppresses popstate echoes). Both must hold; mixing absolute (`window.location.href`) and relative (`pathname + search + hash`) URL shapes in the comparison silently produces duplicate history entries. See `docs/architecture/frontend-app.md` § App Shell URL sync for the full mechanism.

Debugging heuristic: when a page becomes unresponsive after a `page.evaluate(...)` store mutation and the snapshot shows an unrelated empty state, suspect a downstream render error from a mismatched value shape, NOT a reactivity propagation bug.

## Key References

- Architecture: `docs/architecture/frontend-app.md`
- Design system config: `.impeccable.md`
- ADRs: `docs/architecture-decisions/0001-pool-key-branded-type.md`, `0003-state-cursor-contract-and-operator-policy.md`, `0004-frontend-derived-pool-stats.md`
