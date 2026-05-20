# Design plan — URL-based deep linking for the selected run (issue #38)

## Context

`uiStore.selectedRunId: bigint | null` (`frontend/src/lib/stores/ui.svelte.ts:11`) is the single source of truth for "the detail panel is open on run X". Eighteen call sites already read or write it — `RunDetailPanel.svelte`, `RunCard.svelte`, `CommandPalette.svelte`, plus tests. The panel's `Sheet` opens iff `selectedRunId !== null` (`RunDetailPanel.svelte:19–20`), and a defensive `$effect` at `RunDetailPanel.svelte:37–46` auto-clears `selectedRunId` when the referenced run is missing from `runStore.runs`.

Today this state is in-memory only. Closing and reopening the tab loses it; a teammate cannot be sent a link that opens the same run; the browser back button does not close the panel. The frontend has no router and no `window.history` usage — `window.location` is read only once, by `ConnectionManager.svelte:11`, for the WS base URL.

`connectionStore.status` flips to `'connected'` at `frontend/src/lib/connection.ts:232`, which is the first moment `runStore.runs` is guaranteed to reflect the server snapshot (snapshot fetch + apply + buffered-event drain all complete in the preceding steps). This is the signal hydration needs.

The README already advertises *"deep-link semantics work across reconnects"* (line 25), but that wording covers the in-memory `selectedJobId` scroll-to behavior, not URL state. The URL deep link is net-new.

Issue #38 has been open since 2026-04-25 and is one of two remaining 1.0-relevant gaps after the closure pass that landed #17/#36/#56/#65.

The plan was Codex-reviewed once and revised in response to four blocker findings (URL-shape mismatch in loop guard, initial-load race, popstate-stale echo, architecture/test disagreement on query preservation) and four important-concern findings. All Codex findings are reflected in the Locked Decisions table below with their provenance.

## Definition of Done

**Primary deliverable.** Loading `https://atc.example/?run=<numeric-id>` opens the dashboard with `RunDetailPanel` showing run `<id>`, if that run exists in the snapshot. Closing the panel removes the query parameter. Opening the panel by clicking a card adds the parameter. Browser back / forward navigate between panel-closed and panel-open states.

**Success criteria (numbered, mirrored in AC).**

1. `?run=<id>` honors a known run on initial load.
2. `?run=<id>` is gracefully ignored for an unknown / evicted run — panel does not open, URL parameter is cleared via `replaceState`, no error toast, no spurious history entry.
3. Opening the panel from a `RunCard` click writes `?run=<id>` via `pushState`.
4. Closing the panel (Esc, click-outside, X button) strips `run` via `pushState`.
5. Browser back button closes a panel that was opened via card click. Forward button reopens it.
6. The URL-write effect does not echo when the panel-open transition originated from a `popstate` event or from the hydration effect (no infinite loop, no duplicate history entries).

**Exclusions.**

- Per-job deep linking (`?job=<id>`). The existing `selectedJobId` + scroll-to mechanism stays in-memory.
- Theme, filter, command-palette, or any other UI state in the URL.
- A general client-side router. Hand-rolled query-string handling only.

## Locked Decisions

| Decision | Source |
|---|---|
| URL shape: query string `?run=<numeric-id>` | User confirmation, 2026-05-19. Hash and full-router options rejected. |
| Hydration approach: buffer parsed run-ID, apply after `connectionStore.status === 'connected'` (snapshot-loaded). | User confirmation, 2026-05-19. The alternative (soften the missing-run auto-clear effect) was rejected as a less explicit coupling. |
| `uiStore.selectedRunId` remains the canonical store; URL is a projection of it, not a parallel source of truth. | Implicit in the plumbing shape — preserves all 18 existing call sites unchanged. |
| `pushState` (not `replaceState`) on both panel open and close, so back/forward work per AC5. `replaceState` is used only for the unknown-run cleanup paths (initial hydration finds unknown id; popstate restores an unknown id), where adding a history entry would be incorrect. | Required by AC5. |
| **URL canonical shape: relative URL (`pathname + search + hash`).** `formatUrlForRunId(runId, currentUrl)` returns this shape; the loop-guard comparison and the `pushState` / `replaceState` `url` argument both use this shape. Mixing absolute (`window.location.href`) with relative breaks the comparison and silently produces duplicate history entries. | Codex review finding, 2026-05-19. |
| **Query-param preservation.** `formatUrlForRunId` parses `URLSearchParams` from the input URL, sets or deletes `run`, and returns the reformatted relative URL — preserving all other query params and the hash. This is the only contract compatible with future URL state (e.g. deferred `?job=`) and is the contract used by both Architecture and the unit-test suite. | Codex review finding, 2026-05-19. Architecture and tests were misaligned in the first draft; the preservation contract wins. |
| **Outbound writes are suppressed until the initial URL has been consumed.** A `$state` boolean `initialUrlPending` (initialized `true`) guards the outbound effect; the hydration effect flips it to `false` after applying or discarding the buffered run ID. Without this, the outbound effect's first run sees `selectedRunId === null` and strips `?run=` before hydration ever fires. | Codex review finding, 2026-05-19. |
| **Stale `popstate` is handled entirely in the inbound path.** The popstate handler checks `runStore.runs.has(parsedRunId)` before assigning. If absent, it calls `replaceState` to strip the param and does NOT assign `selectedRunId` (so the outbound effect is not triggered, avoiding a duplicate history write). | Codex review finding, 2026-05-19. The alternative — routing stale popstates through `RunDetailPanel`'s missing-run cleanup — would echo via the outbound effect and create a duplicate history entry. |

## Architecture

### Helper module: `frontend/src/lib/url-state.ts`

Pure functions (no DOM access), unit-testable without a browser:

- `parseRunIdFromUrl(url: string): bigint | null` — parses `?run=` via `URLSearchParams`. Returns `null` for missing/empty/non-numeric/negative/non-integer values. Bigints preserve precision for the 64-bit IDs `WorkflowRun` uses.
- `formatUrlForRunId(runId: bigint | null, currentUrl: string): string` — returns a **relative URL** of the form `pathname + search + hash`. Uses `URLSearchParams` to mutate only the `run` key (delete if `runId === null`, set otherwise), preserving all other query params and the hash. The relative shape matches `pushState`'s `url` argument convention and matches the loop-guard comparison's shape exactly.

A small inline helper (in `App.svelte` or `url-state.ts`) returns `currentRelativeUrl(): string` = `window.location.pathname + window.location.search + window.location.hash`. The loop-guard compares `formatUrlForRunId(...)` against this; mixing absolute and relative shapes was the bug Codex caught.

### Three pieces of plumbing, all rooted in `App.svelte`

Two instance-local declarations live in the `<script>` block:

```ts
let initialRunId: bigint | null = parseRunIdFromUrl(window.location.href)
let initialUrlPending = $state(true) // suppresses outbound writes until hydration runs
```

**(1) Outbound — `selectedRunId` → URL.**

An `$effect` reads `uiStore.selectedRunId` and writes the URL:
- If `initialUrlPending`, return immediately. This suppression prevents the *first* effect run (with `selectedRunId === null`) from stripping `?run=42` before the hydration effect has a chance to apply it.
- Compute `target = formatUrlForRunId(uiStore.selectedRunId, window.location.href)`.
- Compute `current = window.location.pathname + window.location.search + window.location.hash`.
- If `target === current`, return — loop guard against popstate-originated writes.
- Otherwise `history.pushState(null, '', target)`.

**(2) Inbound from `popstate` — URL → `selectedRunId`.**

`App.svelte` `onMount` registers a `popstate` listener; the handler runs after the browser has updated `window.location`:
- `parsed = parseRunIdFromUrl(window.location.href)`.
- If `parsed === uiStore.selectedRunId`, no-op (rare: navigating to the same state).
- If `parsed === null`, assign `uiStore.selectedRunId = null`. Outbound effect fires, target === current, no-op. Panel closes via the existing Sheet binding.
- If `parsed !== null` and `runStore.runs.has(parsed)`, assign `uiStore.selectedRunId = parsed`. Outbound effect fires, target === current, no-op. Panel opens.
- If `parsed !== null` and `runStore.runs.has(parsed) === false` (stale link in history): **do not assign**. Call `history.replaceState(null, '', formatUrlForRunId(uiStore.selectedRunId, window.location.href))` to strip the bad `?run=` from the current entry without adding a new history entry. `selectedRunId` is unchanged, so the outbound effect is not triggered, so no duplicate history entry is written.

`onDestroy` removes the listener.

**(3) Initial hydration — buffered, gated on snapshot-loaded.**

An `$effect` watches `connectionStore.status`. On the first transition to `'connected'`:
- If `initialRunId !== null` and `runStore.runs.has(initialRunId)` → assign `uiStore.selectedRunId = initialRunId`. The outbound effect re-fires once `initialUrlPending` is flipped (next step), sees target === current, no-op. Panel opens.
- If `initialRunId !== null` and `runStore.runs.has(initialRunId) === false` → call `history.replaceState(null, '', formatUrlForRunId(null, window.location.href))` to strip the stale `?run=` from the current history entry. `selectedRunId` stays `null`. No panel.
- `initialRunId = null` (consumed; one-shot).
- `initialUrlPending = false` — unlocks the outbound effect. Subsequent `selectedRunId` changes now write the URL normally.

Reconnects (`'connected' → 'reconnecting' → 'connected'`) do not re-trigger hydration because `initialRunId` is now `null`. The effect could technically re-enter on the second `'connected'` but the body is a no-op (and `initialUrlPending` is already `false`).

### Why `replaceState` for both unknown-run paths

Both the initial-hydration unknown-run path and the popstate stale-link path use `replaceState`, not `pushState`. The user did not intentionally navigate to a "panel closed" state — they navigated (or arrived) to a URL pointing at a nonexistent run. Adding a history entry for the cleanup would mean the back button takes them to the broken URL again, where the same cleanup would fire. `replaceState` collapses the broken state into the current entry; the back button then takes them to whatever entry preceded it.

### Interaction with the missing-run auto-clear in `RunDetailPanel.svelte:37–46`

That effect handles only the "panel is open, run gets evicted while open" case — a live state mutation, not a URL-driven transition. It clears `selectedRunId`; the outbound effect fires (with `initialUrlPending === false`); the URL is correctly stripped via `pushState`. This is the *desired* behavior for live eviction: the user's history shows panel-open → panel-closed-because-evicted, and they can back-button to before the eviction.

The popstate stale path explicitly does NOT route through this effect; it bypasses by leaving `selectedRunId` unchanged. That's why the popstate handler does its own `runStore.runs.has` check.

### Rejected alternatives

- **Hash fragments (`#run=<id>`).** Rejected by user direction.
- **Soften the missing-run effect to tolerate "snapshot not loaded yet."** Couples URL flow to panel internals implicitly. Buffer-based hydration is the more explicit seam.
- **`history.replaceState` everywhere.** Breaks AC5 (back-button closes panel).
- **Use `RunDetailPanel`'s missing-run effect as the sole stale-link cleanup mechanism.** Codex finding: this echoes through the outbound effect and creates a duplicate history entry.

## Implementation Steps

### `url-state.ts` helpers (TDD)

Write failing unit tests for `url-state.ts` helpers. Cover `parseRunIdFromUrl` (valid id, missing param, non-numeric, negative, scientific notation, multiple `?run=` params take first via `URLSearchParams.get` semantics, very large bigint preserved). Cover `formatUrlForRunId` (null deletes the `run` param, non-null sets it, preserves pathname and hash, preserves other query params, returns relative URL shape — `pathname + search + hash` — never absolute). Implement `frontend/src/lib/url-state.ts` to pass — pure functions only, no DOM access; takes input URL as string, returns relative URL string.

### E2E tests covering all 6 ACs

Write failing E2E tests in `frontend/e2e/url-deep-link.test.ts` mirroring `run-detail-panel.test.ts` patterns:
- **AC1:** `goto('/?run=<known-id>')` → after WS snapshot lands (await `[role="dialog"]`), panel is open on that run, URL is unchanged.
- **AC2:** `goto('/?run=<unknown-id>')` → panel never opens, `run` param removed via `replaceState`, `history.length` does NOT increment beyond the initial `goto` entry.
- **AC3:** card-click test → URL gains `?run=<id>`, history length +1.
- **AC4:** Esc/click-outside/X test → URL strips `?run=`, history length +1.
- **AC5:** open → close → back → forward sequence verifies panel state and URL at each step.
- **AC6:** Two sub-assertions, both required:
  - **AC6a — mount-time no-pollution:** load `/?run=<known-id>`, wait for panel open, read `history.length`. Compare against a `goto('/')` baseline (panel-closed, no `?run=`). Difference must be exactly the entries `goto` itself added; no extra entries from the outbound effect mistakenly stripping and re-adding `?run=` during hydration.
  - **AC6b — popstate-no-echo:** after AC3's card-click (history.length = N+1), press the back button. Wait for popstate to settle. `history.length` must still be `N+1` (back does not add an entry); the inbound assignment of `selectedRunId` did not trigger an outbound `pushState`.

### Wire App.svelte

Order: (a) `initialRunId` capture and `initialUrlPending` `$state` declaration; (b) outbound `$effect` with the `initialUrlPending` early-return and the `target === current` loop guard; (c) `onMount` popstate listener with the inbound stale-id `replaceState` path and `onDestroy` cleanup; (d) hydration `$effect` watching `connectionStore.status` that flips `initialUrlPending = false` after applying or discarding `initialRunId`. Run E2E suite; all six ACs must pass.

### Manual smoke

`pnpm dev`, exercise: known `?run=` initial load; unknown `?run=` initial load; click card and verify URL; Esc to close; back/forward sequence. Watch dev-tools Application → History for unexpected entries.

### Frontend hygiene

`pnpm lint`, `pnpm check`, `pnpm test`, `pnpm test:e2e`. Resolve anything that surfaces.

### Documentation

Apply the changes listed under "Documents to Update" below.

### PR

Squash-merge title `feat(frontend): URL-based deep linking for selected run`. PR body cites AC6's two sub-checks explicitly so a reviewer can spot-verify the loop-guard story.

## Acceptance Criteria

The contract for "URL strips `run`" is: the resulting URL has the same pathname and hash as before, with the `run` query param removed (other query params preserved). Concrete examples use `/` for clarity but the invariant is the parametric one.

- **AC1.** Visiting `/?run=<id>` for a known `<id>` opens `RunDetailPanel` after the WS snapshot loads. The URL is unchanged after hydration (no flicker of `?run=` being stripped and re-added).
- **AC2.** Visiting `/?run=<unknown-id>` results in no panel and the `run` query param removed via `replaceState` (no extra history entry). Other query params and hash are preserved.
- **AC3.** With panel closed, clicking a `RunCard` for run `<id>` produces a URL with `run=<id>` set (other query params preserved) and adds exactly one entry to `history`.
- **AC4.** With panel open, pressing Esc / clicking X / clicking outside removes the `run` query param (preserving other query params and hash) and adds exactly one entry to `history`.
- **AC5.** Open → close → press back-button: panel reopens, URL has `run=<id>`. Open → close → back → forward: panel closes again, URL has `run` removed.
- **AC6.** No history pollution from the URL ↔ store sync mechanism itself:
  - **AC6a (mount-time).** Loading `/?run=<known-id>` directly produces exactly the history entries that `goto` itself creates; the outbound effect's first run does not strip and re-add `?run=`.
  - **AC6b (popstate-no-echo).** After a card-click (which adds one history entry), pressing the back button does NOT add an additional entry. (The inbound assignment of `selectedRunId` must not trigger an outbound `pushState`.) Measured as: `history.length` is unchanged across the back-button transition.

## Documents to Update

- **`docs/architecture/frontend-app.md`** — canonical home of the URL-sync invariant. Add a new subsection (in the `App.svelte` / data-flow vicinity, whichever fits the existing structure) that documents: the three pieces of plumbing, the relative-URL canonical shape, the `initialUrlPending` suppression flag, the popstate stale-id `replaceState` cleanup, and the `connectionStore.status === 'connected'` hydration trigger. Reference `frontend/src/lib/url-state.ts` and the relevant `App.svelte` block. Future URL-state additions (e.g., filter persistence, deferred `?job=`) should follow this same shape.
- **`frontend/CLAUDE.md`** — add a Sharp Edge whose body is a pointer plus the one-line invariant that bites in practice: *"URL ↔ `selectedRunId` sync. Two flags govern the loop: `initialUrlPending` (suppresses outbound writes until the first snapshot lands) and the relative-URL `target === current` comparison (suppresses popstate echoes). Both must hold; mixing absolute and relative URL shapes silently produces duplicate history entries. See `docs/architecture/frontend-app.md` § App Shell URL sync for the full mechanism."* Keep the sharp edge tight; the architecture doc is the canonical home, per the non-duplication rule.
- `scripts/doc-mapping.sh` — **no change needed.** The existing `frontend/src/*` catch-all (line 102) already maps `frontend/src/lib/url-state.ts` and `frontend/src/App.svelte` to `docs/architecture/frontend-app.md`, so the pre-push doc-staleness gate already fires on changes to those files.

## Out of Scope

- Per-job URL state (`?job=<id>`) — tracked implicitly under #38's "out of scope: persisted selection state" framing; file a follow-up if/when the README's job-scroll behavior needs cross-tab durability.
- Filter / theme / palette in the URL — separate feature.
- Server-side rendering of the `?run=` query for crawlers / link previews — not relevant; ATC is auth-gated.

## Glossary

- **Hydration moment.** The first `connectionStore.status === 'connected'` transition after mount, which is the first moment `runStore.runs` is provably populated with the server snapshot.
- **Outbound write.** `history.pushState` driven by a `selectedRunId` change in the store.
- **Inbound write.** `selectedRunId` assignment driven by a `popstate` event or the initial-hydration effect.
- **Loop guard.** The "current URL already matches target" check in the outbound effect, which silently no-ops popstate-induced writes.
- **`initialUrlPending` suppression.** The boolean `$state` flag (initialized `true` in `App.svelte`) that disables outbound writes until the hydration effect has consumed the buffered `initialRunId`. Without this, the outbound effect's first run with `selectedRunId === null` would strip `?run=42` from the URL before hydration ever gets a chance to apply it.
- **Relative URL shape.** `pathname + search + hash` — the form returned by `formatUrlForRunId` and the form used by `history.pushState`'s `url` argument. The loop-guard comparison must operate on this shape on both sides; mixing in `window.location.href` (absolute) breaks the comparison silently.
