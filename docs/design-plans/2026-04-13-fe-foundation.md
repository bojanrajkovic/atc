# Frontend Foundation Design

## Summary

This design establishes the frontend's foundational infrastructure: the OKLCH design system integration, reactive state management, and real-time server communication. Rather than building visible UI, it lays the plumbing that all subsequent sub-phases build on — token system, stores, connection protocol, and test harness.

The approach hinges on three key decisions. First, TypeScript types are generated from Rust structs via `ts-rs`, creating a closed loop where type drift is structurally impossible (the same binary serves the API and embeds the frontend that was compiled against the generated types). Second, shadcn-svelte components are integrated via a CSS alias layer that maps shadcn's variable names to ATC's OKLCH tokens, so copied components work unmodified and can be updated by re-running the CLI. Third, the real-time connection uses a WS-first connect protocol with seq-based reconciliation — the WebSocket opens before the state snapshot is fetched, ensuring no events are lost during the gap, with RAF-batched updates to prevent excessive re-renders during CI burst activity.

## Definition of Done

Sub-Phase 1 (Foundation) of the ATC frontend dashboard is complete when:

1. **Design system integration:** shadcn-svelte is installed with an initial component set (Card, Badge, Toggle, Progress) copied into the project and remapped to ATC's `.impeccable.md` OKLCH token system. `app.css` is replaced wholesale with the canonical playground/impeccable tokens — short names (`--bg`, `--surface`, `--queued`, etc.), rich chroma values, Major Third type scale, system font stacks, and domain-specific status colors.

2. **TypeScript domain types:** Exact mirror of the Rust types serialized by the server — `WorkflowRun`, `Job`, `Step`, `RunnerInfo`, `RunnerPoolStats`, `RunStatus`, `JobStatus`, `StepStatus`, `RunConclusion`, `JobConclusion`, `RunId`, `JobId`, `RepoKey`, `LabelSet`, `StateSnapshot`, `SeqEvent`, `WebhookEvent` (discriminated union). PascalCase enum serialization matching the backend.

3. **Store scaffolding:** Four Svelte 5 rune-class stores (`runs`, `runners`, `connection`, `ui`) with `$derived` state for kanban column filtering. No duplicated state — column contents are derived, never maintained as separate arrays.

4. **ConnectionManager:** WebSocket client with exponential backoff reconnect, `GET /v1/state` initial backfill, seq-based reconciliation (connect WS first, GET state, discard buffered events with `seq < snapshot.seq`), and `requestAnimationFrame`-batched updates for burst messages.

5. **Test infrastructure:** Vitest + `@testing-library/svelte` + jsdom configured for component testing. Playwright installed with dev server fixture. Store unit tests (apply event, derived column filters). ConnectionManager tests (mock WebSocket, verify store mutations, verify reconnect logic). E2E: app renders, theme switching works across all four themes and both modes.

**Out of scope:** Visual components beyond the copied shadcn set, kanban layout, runner bar, card rendering, responsive breakpoints, command palette, detail panel.

## Acceptance Criteria

### fe-foundation.AC1: Design system tokens render correctly
- **fe-foundation.AC1.1 Success:** Dark mode (default) renders surfaces with `.impeccable.md` OKLCH values (rich chroma 0.063, not the scaffold's 0.01)
- **fe-foundation.AC1.2 Success:** Light mode override changes surface/text lightness without duplicating all tokens
- **fe-foundation.AC1.3 Success:** Each theme hue (70, 155, 280, 310) applies via `[data-theme]` attribute and all neutral surfaces shift accordingly
- **fe-foundation.AC1.4 Success:** Status colors (`--queued`, `--running`, `--success`, `--failed`, `--cancelled`) are independent of theme hue
- **fe-foundation.AC1.5 Success:** shadcn components (Card, Badge, Toggle, Progress) render using ATC token colors via the CSS alias layer
- **fe-foundation.AC1.6 Success:** `prefers-reduced-motion` media query disables all transitions and animations
- **fe-foundation.AC1.7 Edge:** Theme and mode can be changed independently (switching theme preserves mode, switching mode preserves theme)

### fe-foundation.AC2: Generated TypeScript types match Rust serialization
- **fe-foundation.AC2.1 Success:** `just types` generates `frontend/src/lib/types/generated.ts` without errors
- **fe-foundation.AC2.2 Success:** Generated `WorkflowRun` interface has camelCase field names matching `#[serde(rename_all = "camelCase")]`
- **fe-foundation.AC2.3 Success:** `WebhookEvent` is a discriminated union with `type` field (`"Run"` | `"Job"`) and `data` field containing the envelope
- **fe-foundation.AC2.4 Success:** Status enums generate as PascalCase string literal unions (`"Queued" | "InProgress" | "Completed"`)
- **fe-foundation.AC2.5 Failure:** CI fails if `generated.ts` is out of date (modified Rust types without regenerating)

### fe-foundation.AC3: Stores manage state correctly
- **fe-foundation.AC3.1 Success:** `RunStore.applyRunEvent()` creates a new run for an unknown run ID
- **fe-foundation.AC3.2 Success:** `RunStore.applyRunEvent()` updates an existing run's status and fields
- **fe-foundation.AC3.3 Success:** `RunStore.applyJobEvent()` groups jobs by run ID
- **fe-foundation.AC3.4 Success:** Derived `queuedRuns` returns only runs with status `"Queued"`, `inProgressRuns` only `"InProgress"`, `completedRuns` only `"Completed"`
- **fe-foundation.AC3.5 Success:** `RunStore.loadSnapshot()` replaces all state atomically (existing runs/jobs cleared, snapshot data loaded)
- **fe-foundation.AC3.6 Success:** `RunnerStore.loadPools()` replaces pool stats
- **fe-foundation.AC3.7 Success:** `ConnectionStore.isStale` returns true when connected but no event received for >30 seconds
- **fe-foundation.AC3.8 Success:** `UIStore` persists theme and mode to `localStorage` and restores on initialization
- **fe-foundation.AC3.9 Edge:** Duplicate events (same run ID, same status) are idempotent — no duplicate entries, no errors

### fe-foundation.AC4: ConnectionManager handles the full lifecycle
- **fe-foundation.AC4.1 Success:** Connect sequence: opens WebSocket, fetches state snapshot, loads stores, discards buffered WS events with `seq < snapshot.seq`, transitions to "connected"
- **fe-foundation.AC4.2 Success:** Post-connect WS messages are dispatched to stores via EventDispatcher
- **fe-foundation.AC4.3 Success:** EventDispatcher batches multiple events within one animation frame into a single store update pass
- **fe-foundation.AC4.4 Success:** On WebSocket close, status transitions to "reconnecting" and retries with exponential backoff (1s, 2s, 4s, 8s, capped at 30s)
- **fe-foundation.AC4.5 Success:** Reconnect re-runs the full connect sequence (re-fetches state to fill gaps)
- **fe-foundation.AC4.6 Success:** `destroy()` closes WebSocket and clears all reconnect timers
- **fe-foundation.AC4.7 Failure:** State fetch failure during connect triggers reconnect (does not leave app in "connecting" state forever)
- **fe-foundation.AC4.8 Edge:** Events arriving during state fetch are buffered and replayed after seq filtering

### fe-foundation.AC5: E2E tests verify rendering and theming
- **fe-foundation.AC5.1 Success:** App renders at `/` without console errors
- **fe-foundation.AC5.2 Success:** Clicking each theme option changes `data-theme` attribute on `<html>` to the correct value
- **fe-foundation.AC5.3 Success:** Toggling dark/light mode changes `data-mode` attribute and visibly changes surface colors

## Glossary

- **OKLCH**: A perceptual color space (Lightness, Chroma, Hue) used by ATC's design system. All neutral colors derive from a single `--hue` CSS variable; switching themes changes one value and all surfaces recompute.
- **shadcn-svelte**: A Svelte port of shadcn/ui that copies component source into your project via a CLI. Components are owned and editable, built on Bits UI headless primitives.
- **Bits UI**: Headless accessible component library for Svelte 5. Provides keyboard navigation, ARIA attributes, and focus management without visual opinions. The behavioral layer underneath shadcn-svelte.
- **ts-rs**: Rust crate that generates TypeScript type definitions from Rust structs via `#[derive(TS)]`. Ensures frontend types match backend serialization at build time.
- **Adjacently tagged (serde)**: A Rust enum serialization format (`#[serde(tag = "type", content = "data")]`) that produces `{"type": "Variant", "data": {...}}` JSON — maps cleanly to TypeScript discriminated unions with `switch(event.type)`.
- **Svelte 5 runes**: Svelte 5's reactivity primitives — `$state` for reactive values, `$derived` for computed values, `$effect` for side effects. Replace Svelte 4's store API.
- **Seq reconciliation**: Protocol for syncing a WebSocket event stream with a REST state snapshot. The `seq` counter (monotonic, server-assigned) lets the client discard stale buffered events after loading a snapshot.
- **RAF batching**: Accumulating WebSocket updates in a buffer and flushing them on `requestAnimationFrame`, so multiple events arriving in one frame produce a single reactive update instead of N re-renders.
- **MSW (Mock Service Worker)**: Testing library that intercepts `fetch()` and WebSocket connections at the network level. Tests exercise real application code while controlling server responses.
- **rust-embed**: Rust crate that embeds static files (the compiled frontend) into the release binary. In dev mode, reads from disk; in release mode, serves from the embedded binary.
- **CSS alias layer**: A set of CSS custom property definitions (`--background: var(--bg)`) that map shadcn-svelte's expected variable names to ATC's canonical OKLCH tokens, allowing copied components to work unmodified.
- **Discriminated union**: A TypeScript union type where each member has a literal field (the discriminant, here `type`) that uniquely identifies it, enabling exhaustive `switch` dispatch without type casts.
- **Seq (sequence number)**: A monotonically increasing integer on each WebSocket event. Used to reconcile the overlap between the initial state snapshot fetch and buffered WebSocket messages: events with `seq < snapshot.seq` are discarded as already represented in the snapshot.
- **TTL eviction**: The backend's mechanism for expiring completed `WorkflowRun` entries after a configurable time-to-live. Does not currently generate WebSocket events, so the frontend accumulates stale completed runs until the next reconnect.
- **`$lib` path alias**: A Vite/TypeScript path alias mapping `$lib` to `src/lib/`. Required by the shadcn-svelte CLI to resolve component paths correctly.

## Architecture

### Design System Integration

`app.css` is replaced wholesale with the `.impeccable.md` OKLCH token system. The structure is dark-first: `:root` defines dark mode tokens (the primary mode), and `[data-mode="light"]` overrides only the tokens that change for light mode. No `[data-mode="dark"]` selector exists — dark is the default. Theme hues are set via `[data-theme="warm|radar|violet|pink"]` changing a single `--hue` custom property from which all neutral surfaces, text, and borders derive.

Tokens use the short names from the playground prototype: `--bg`, `--surface`, `--surface-raised`, `--border`, `--text`, `--text-dim` for neutrals; `--queued`, `--running`, `--success`, `--failed`, `--cancelled` for domain-specific status colors; `--accent` and `--text-on-accent` for interactive elements. Typography uses the Major Third scale (`--text-xs: 0.625rem`, `--text-sm: 0.75rem`, `--text-base: 0.9375rem`) with system font stacks. Motion tokens (`--ease-out-expo`, `--duration-fast/normal/slow`) and a `prefers-reduced-motion` reset are included.

A `@theme` block bridges OKLCH tokens into Tailwind v4 for font families and layout utilities. Components use `var(--token)` directly for colors; Tailwind handles layout (`flex`, `gap`, `p-*`).

**shadcn compatibility layer:** Rather than modifying copied shadcn-svelte component source, `app.css` defines CSS aliases that map shadcn's variable names to ATC canonical tokens:

| shadcn variable | ATC alias |
|---|---|
| `--background` | `var(--bg)` |
| `--foreground` | `var(--text)` |
| `--primary` | `var(--accent)` |
| `--primary-foreground` | `var(--text-on-accent)` |
| `--secondary` | `var(--surface-raised)` |
| `--secondary-foreground` | `var(--text)` |
| `--muted` | `var(--surface-raised)` |
| `--muted-foreground` | `var(--text-dim)` |
| `--border` | `var(--border)` |
| `--ring` | `var(--accent)` |
| `--destructive` | `var(--failed)` |

This means copied shadcn components work unmodified, future `pnpm exec shadcn-svelte add` commands require no post-copy patching, and the light mode override only redefines ATC canonical tokens — the aliases follow via `var()` indirection.

shadcn-svelte is initialized using the official Vite installation path. A `$lib` path alias is configured in both `vite.config.ts` and `tsconfig.json` (pointing to `src/lib`) so the CLI works as documented. The initial component set (Card, Badge, Toggle, Progress) is copied via `pnpm exec shadcn-svelte add`.

### TypeScript Domain Types

Types are generated from Rust structs using `ts-rs`. The `atc-core` crate adds `#[derive(TS)]` to all domain types, exporting TypeScript interfaces to `frontend/src/lib/types/generated.ts`. This creates a closed loop with `rust-embed`: Rust types generate TypeScript → frontend compiles against them → `rust-embed` embeds the compiled frontend → the same binary serves the API that matches those types. Type drift is structurally impossible.

**Backend serde change:** `WebhookEvent`, `RunEvent`, and `JobEvent` switch from externally-tagged to adjacently-tagged serialization (`#[serde(tag = "type", content = "data")]`). This enables clean TypeScript discriminated unions:

```typescript
// Adjacently tagged — clean switch on .type
type Tagged<T extends string, D = never> =
  [D] extends [never] ? { type: T } : { type: T; data: D };

type WebhookEvent =
  | Tagged<"Run", RunEventEnvelope>
  | Tagged<"Job", JobEventEnvelope>;
```

The generated types include: `WorkflowRun`, `Job`, `Step`, `RunnerInfo`, `RunnerPoolStats`, all status/conclusion enums (as string literal unions matching PascalCase serialization), `RunEventEnvelope`, `JobEventEnvelope`, `RunEvent`, `JobEvent`, `StateSnapshot`, `SeqEvent`, and `WebhookEvent`.

A `just types` recipe regenerates the TypeScript file. CI verifies the generated file is up to date.

### Store Architecture

Four module-level singleton stores in `src/lib/stores/`, each a class with `$state` fields and `$derived` getters exported from `.svelte.ts` files:

**`runs.svelte.ts` — RunStore:**
- `$state`: `Map<number, WorkflowRun>` keyed by run ID
- `$state`: `Map<number, Job[]>` — jobs grouped by run ID
- `$derived`: `queuedRuns`, `inProgressRuns`, `completedRuns` (filtered arrays for kanban columns)
- Methods: `applyRunEvent(envelope)`, `applyJobEvent(envelope)`, `loadSnapshot(snapshot)`, `clear()`

**`runners.svelte.ts` — RunnerStore:**
- `$state`: `RunnerPoolStats[]`
- Methods: `loadPools(pools)`, `clear()`

**`connection.svelte.ts` — ConnectionStore:**
- `$state`: `status` (`"connecting" | "connected" | "reconnecting" | "disconnected"`), `lastEventAt` (timestamp), `reconnectAttempt` (count)
- `$derived`: `isStale` (no event received in >30 seconds while connected)

**`ui.svelte.ts` — UIStore:**
- `$state`: `theme`, `mode`, `density`, `selectedRunId`
- `$effect`: syncs `theme` and `mode` to `document.documentElement` attributes, persists to `localStorage`

Stores are module-level singletons because there is no SSR (standalone Vite SPA), no multi-instance requirement, and the store surface area is small. If SvelteKit/SSR is ever introduced, migrating to factory + `setContext` is a mechanical refactor.

### ConnectionManager + EventDispatcher

Two plain TypeScript classes that separate network concerns from data concerns:

**`EventDispatcher` (`src/lib/dispatcher.ts`):**
- Constructor takes `RunStore` and `RunnerStore` references
- `dispatch(event: SeqEvent)` pushes to a pending buffer. On the first call per animation frame, schedules a `requestAnimationFrame` callback that processes all buffered events: routes `Run` events to `runStore.applyRunEvent()`, routes `Job` events to `runStore.applyJobEvent()`
- `flush()` method processes the buffer synchronously, bypassing RAF. Used in tests.

**`ConnectionManager` (`src/lib/connection.ts`):**
- Constructor takes `EventDispatcher`, `ConnectionStore`, `RunStore`, `RunnerStore`, and a `baseUrl`
- **Connect sequence:** (1) Set `connStore.status = "connecting"`. (2) Open WebSocket to `${baseUrl}/v1/ws`. (3) On WS open, begin buffering incoming messages. (4) Fetch `GET ${baseUrl}/v1/state`, call `runStore.loadSnapshot()` and `runnerStore.loadPools()`, save `snapshot.seq`. (5) Flush buffered WS messages, discarding any with `seq < snapshot.seq`. (6) Set `connStore.status = "connected"`, route future messages to `dispatcher.dispatch()`.
- **Reconnect:** On WS close or error, set `connStore.status = "reconnecting"`, wait with exponential backoff (1s → 2s → 4s → 8s, capped at 30s), then re-run the full connect sequence (re-fetches state to fill gaps from the disconnection).
- **Destroy:** Closes WebSocket, clears reconnect timers.

The WS-first connect order is critical: connecting the WebSocket before fetching state ensures no events are lost during the gap. The seq-based discard after the state fetch deduplicates any overlap.

### Test Infrastructure

**Vitest configuration:** `@testing-library/svelte` v4.1+ with `svelteTesting()` Vite plugin, jsdom environment. Store tests and dispatcher tests are plain `.test.ts` files (these classes are testable without Svelte component context since `$state`/`$derived` work in `.svelte.ts` files under jsdom). Coverage via `@vitest/coverage-v8`.

**Test organization:** Sibling files — `runs.svelte.ts` gets `runs.test.ts` next to it, per the UI decomposition README's principle #2.

**MSW for ConnectionManager tests:** `msw/node` intercepts both `fetch()` (for `GET /v1/state`) and WebSocket connections (for `/v1/ws`) at the network level. Tests exercise the real `ConnectionManager` code path — no manual WebSocket mock classes. MSW handlers control server behavior (send messages, close connections, simulate errors) from within test assertions.

**Store and dispatcher tests use direct method calls:** No MSW or network mocking needed — instantiate the store/dispatcher, call methods, assert state. `EventDispatcher.flush()` bypasses RAF for synchronous test assertions.

**Playwright E2E:** Dev server fixture starts `pnpm dev` and waits for ready. Tests verify: app renders without errors, theme switching changes `data-theme` and `--hue` across all four themes, dark/light mode toggle changes `data-mode` and surface colors.

## Existing Patterns

**Frontend structure:** The existing frontend (`frontend/src/`) is a skeleton with `App.svelte`, `app.css`, and `main.ts`. The OKLCH token system in `app.css` establishes the `[data-theme]` and `[data-mode]` attribute approach for theme and mode switching. This design preserves the attribute mechanism while replacing the token values and names.

**Backend patterns:** The `atc-core` crate uses `#[derive(Serialize, Deserialize)]` with `#[serde(rename_all = "camelCase")]` on all domain structs. This design adds `#[derive(TS)]` alongside existing derives. The adjacently-tagged serde change (`#[serde(tag = "type", content = "data")]`) affects three enums in `atc-core` and `atc-github`; all other serialization conventions are preserved.

**Testing patterns:** The backend uses sibling test files and organizes tests by acceptance criteria (per `feedback_test_organization_by_ac.md`). The frontend test organization follows the same principle — sibling test files with tests grouped by the acceptance criteria they verify.

**Project conventions:** `justfile` recipes evolve as code lands (per `.ed3d/design-plan-guidance.md`). This design adds `just types` for TypeScript generation. Lefthook hooks are not modified.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Backend Coordination (ts-rs + serde)

**Goal:** Generate TypeScript types from Rust structs and switch to adjacently-tagged enum serialization.

**Components:**
- `ts-rs` dependency added to `atc-core` — `#[derive(TS)]` on all domain types (`WorkflowRun`, `Job`, `Step`, `RunnerInfo`, `RunnerPoolStats`, `RunId`, `JobId`, `RepoKey`, `LabelSet`, status/conclusion enums, event types, envelopes)
- `#[serde(tag = "type", content = "data")]` on `WebhookEvent` in `backend/crates/atc-github/src/webhook/mod.rs`, `RunEvent` and `JobEvent` in `backend/crates/atc-core/src/event.rs`
- `StateSnapshot` and `SeqEvent` types in `backend/crates/atc-server/src/` — add `#[derive(TS)]`
- `just types` recipe in `justfile`
- CI step to verify `frontend/src/lib/types/generated.ts` is up to date

**Dependencies:** None (first phase)

**Done when:** `just types` generates `frontend/src/lib/types/generated.ts`, all backend tests pass with the new serde format, CI verifies generated types are current
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Design System + shadcn Setup

**Goal:** Replace scaffold `app.css` with canonical OKLCH tokens and initialize shadcn-svelte.

**Components:**
- `frontend/src/app.css` — replaced with `.impeccable.md` token system (dark-first `:root`, `[data-mode="light"]` override, `[data-theme]` hue switching, shadcn compatibility aliases, motion tokens, `prefers-reduced-motion` reset, `@theme` Tailwind bridge)
- `frontend/vite.config.ts` — add `$lib` path alias
- `frontend/tsconfig.json` — add `$lib` path alias
- shadcn-svelte CLI initialization and component copy (Card, Badge, Toggle, Progress into `src/lib/components/ui/`)
- `frontend/src/App.svelte` — updated to use new token names

**Dependencies:** None (independent of Phase 1)

**Done when:** All four themes render correctly in both dark and light mode. shadcn components render with ATC token colors. `pnpm build` succeeds.

**Covers:** fe-foundation.AC1.*
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Stores

**Goal:** Scaffold the four Svelte 5 rune-class stores with derived state and unit tests.

**Components:**
- `frontend/src/lib/stores/runs.svelte.ts` — RunStore with `$state` maps, `$derived` column filters, event application methods, snapshot loading
- `frontend/src/lib/stores/runners.svelte.ts` — RunnerStore with pool stats
- `frontend/src/lib/stores/connection.svelte.ts` — ConnectionStore with status, staleness detection
- `frontend/src/lib/stores/ui.svelte.ts` — UIStore with theme/mode/density, DOM sync, localStorage persistence
- Vitest configuration in `frontend/vitest.config.ts` — `@testing-library/svelte`, jsdom, `svelteTesting()` plugin, coverage
- Unit tests for all four stores (sibling `.test.ts` files)

**Dependencies:** Phase 1 (generated TypeScript types)

**Done when:** All store unit tests pass — event application, derived column filtering, snapshot loading, status transitions, staleness detection. `pnpm test` succeeds.

**Covers:** fe-foundation.AC2.*, fe-foundation.AC3.*
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: ConnectionManager + EventDispatcher

**Goal:** Implement WebSocket client with backfill reconciliation and RAF-batched event dispatching.

**Components:**
- `frontend/src/lib/dispatcher.ts` — EventDispatcher with RAF batching and `flush()` for tests
- `frontend/src/lib/connection.ts` — ConnectionManager with connect sequence, seq reconciliation, exponential backoff reconnect, destroy cleanup
- MSW setup — `msw` dependency, `msw/node` handlers for `GET /v1/state` and WebSocket `/v1/ws`
- Integration tests for ConnectionManager (connect sequence, reconnect, seq filtering, destroy)
- Unit tests for EventDispatcher (routing, batching, flush)

**Dependencies:** Phase 3 (stores to write into)

**Done when:** ConnectionManager tests verify full connect sequence (WS open → state fetch → buffer flush → connected), reconnect with backoff, seq-based event filtering, and clean destroy. EventDispatcher tests verify event routing and RAF batching. All tests pass.

**Covers:** fe-foundation.AC4.*
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Playwright E2E

**Goal:** End-to-end tests verifying the app renders and theme system works.

**Components:**
- Playwright installation and configuration in `frontend/`
- Dev server fixture (starts `pnpm dev`, waits for ready)
- E2E test: app renders without errors
- E2E test: theme switching across all four themes verifies `data-theme` attribute and `--hue` value
- E2E test: dark/light mode toggle verifies `data-mode` attribute and surface color changes

**Dependencies:** Phase 2 (design system must be in place)

**Done when:** All E2E tests pass in headless Chromium. `pnpm exec playwright test` succeeds.

**Covers:** fe-foundation.AC5.*
<!-- END_PHASE_5 -->

## Additional Considerations

**Server eviction gap:** The backend's `StateStore` evicts completed runs past TTL, but eviction does not generate WebSocket events. The frontend store will accumulate stale completed runs until the next full state refresh (reconnect). This is acceptable for Sub-Phase 1 — the number of active runs is small enough that accumulation isn't a performance concern. A periodic state refresh or server-side eviction events can be added in a later sub-phase if needed.

**Documents to update alongside implementation:**
| Document | Update |
|---|---|
| `frontend/CLAUDE.md` | Status from "Skeleton phase" to reflect stores, connection, test infrastructure |
| `docs/architecture/frontend-app.md` | Store architecture, connection protocol, test strategy |
| `backend/crates/atc-core/CLAUDE.md` | Note ts-rs dependency and generated types contract |
| `backend/crates/atc-server/CLAUDE.md` | Note adjacently-tagged serde format for WebSocket events |
