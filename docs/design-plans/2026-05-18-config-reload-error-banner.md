# Design Plan: ConfigReloadError admin alert banner (issue #203)

## Context

Issue #203 is the frontend half of the hot-reload work that landed in #172 + #227. The backend already:

- Broadcasts `WireFrame::ConfigReloadError { reason: String }` whenever the runner-pool config watcher hits a failed reload (`backend/crates/atc-server/src/config_watcher.rs:394-405`).
- Keeps serving last-known-good capacities and emits `atc_config_reload_failures_total{reason="read"|"parse"|"validate"}` (backend metric only — the **wire `reason` is a free-form `String` from `err.to_string()`**, not the metric-side category enum).
- Generates the TypeScript binding (`frontend/src/lib/types/generated/WireFrame.ts:35`).

The frontend dispatcher already handles the variant (`frontend/src/lib/dispatcher.ts:53-59`) but only fires a `console.warn` referencing this issue:

```ts
case 'ConfigReloadError':
  console.warn(
    `Config reload failed on server: ${frame.reason}. ` +
      `UI surfacing tracked in https://github.com/bojanrajkovic/atc/issues/203.`,
  )
  break
```

A reusable banner pattern landed in #227: `frontend/src/lib/components/VersionMismatchBanner.svelte` is a full-width strip mounted in `AppShell.svelte:11` between `<TopBar />` and `<main>`, driven entirely by `ConnectionStore` state. This PR follows the same pattern with a second consumer.

The work matters now because the hot-reload feature is in the deploy stream and operators currently must `kubectl logs` or open Prometheus to see that a config edit was rejected. The banner closes that observability gap for anyone already looking at the dashboard.

## Definition of Done

1. A `ConfigReloadError` frame received while the dashboard is connected surfaces as a visible banner displaying the backend's `reason` string.
2. The banner is **dismissible** by the operator (close button) and **auto-dismisses 60 seconds after the most recent error** (wall-clock timer, not tied to a follow-up `ConfigUpdate`).
3. Successive `ConfigReloadError` frames replace the visible reason (single-slot, last-wins) and restart the 60s timer.
4. The banner visual treatment uses the `--failed` design-system token to distinguish "error" from `VersionMismatchBanner`'s informational `--queued` tone.
5. Frontend architecture doc (`docs/architecture/frontend-app.md`) and the dispatcher row in `frontend/CLAUDE.md` are updated to describe the new behavior. Both unit and browser-mode tests cover the store extension and the banner.

## Locked Decisions

These were resolved during planning and are **not** open for re-evaluation:

- **Store placement: extend `ConnectionStore`** (`frontend/src/lib/stores/connection.svelte.ts`), not a new `adminAlerts.svelte.ts` framework and not a dedicated `configReload` store. Source: planning conversation 2026-05-18. Rationale: `ConnectionStore` already drives `VersionMismatchBanner` via the same pattern; two consumers do not yet justify a general framework (issue #203 wording: "broaden only if a second consumer arrives" — ConfigReloadError IS that second consumer, but the pattern hasn't proven itself yet).

- **Dismissal: manual close + 60s wall-clock auto-dismiss, no `ConfigUpdate`-triggered clear.** Source: planning conversation 2026-05-18. Rationale: a non-operator viewer of the dashboard should not be stuck with a banner that names a config-validation failure they can't act on. The 60s window is short enough that staleness after a fix is not a real concern; the close button covers the "I read it, hide it" case.

- **Stacking: single slot, last-wins.** Mirrors `VersionMismatchBanner`'s pattern. Source: planning conversation 2026-05-18.

- **Pre-snapshot frames remain dropped** (the existing behavior at `frontend/src/lib/connection.ts:114-145`). The store's `markConfigReloadError` is only called by the dispatcher, which only runs post-snapshot. Source: existing architecture, intentionally preserved.

- **Banner placement: full-width strip below `TopBar`**, sibling to `VersionMismatchBanner` in `AppShell.svelte`. Source: existing pattern in `frontend/src/lib/components/AppShell.svelte:11`. The issue's "open question" about top-of-page sticky vs. corner toast vs. TopBar-inline is resolved by reusing the established slot.

- **`reason` is rendered verbatim** as a string. The backend's metric-side `{read|parse|validate}` enum is **not** on the wire and the frontend does not branch on category. Source: `backend/crates/atc-server/src/config_watcher.rs:401` (`let reason = err.to_string()`).

## Architecture

### Store extension

Add to `ConnectionStore` (`frontend/src/lib/stores/connection.svelte.ts`):

```ts
private static readonly CONFIG_RELOAD_ERROR_AUTO_DISMISS_MS = 60_000

configReloadError = $state<string | null>(null)
private configReloadErrorTimeout: ReturnType<typeof setTimeout> | null = null

markConfigReloadError(reason: string): void {
  if (this.configReloadErrorTimeout !== null) {
    clearTimeout(this.configReloadErrorTimeout)
  }
  this.configReloadError = reason
  this.configReloadErrorTimeout = setTimeout(() => {
    this.configReloadError = null
    this.configReloadErrorTimeout = null
  }, ConnectionStore.CONFIG_RELOAD_ERROR_AUTO_DISMISS_MS)
}

dismissConfigReloadError(): void {
  if (this.configReloadErrorTimeout !== null) {
    clearTimeout(this.configReloadErrorTimeout)
    this.configReloadErrorTimeout = null
  }
  this.configReloadError = null
}
```

Update `destroy()` to also clear `configReloadErrorTimeout` (currently clears `tickInterval` only — `connection.svelte.ts:103-108`). Note: the `connectionStore` singleton is not wired into production teardown (`ConnectionManager.svelte` does not call `destroy`); the clear here is primarily for test cleanup parity with the existing `tickInterval` pattern. Adding it is the right shape regardless — if a future lifecycle hook calls `destroy()`, the new timer participates correctly.

**Why store-side timer (not component `$effect`):** Unlike `VersionMismatchBanner`'s visible countdown, the auto-dismiss here is opaque (no `13s, 12s, ...` UI). Putting the timer on the store means the test can call `markConfigReloadError`, advance fake timers, and assert state without mounting the Svelte component. The component becomes a pure reactive renderer.

### Dispatcher swap

Replace the `case 'ConfigReloadError'` branch in `frontend/src/lib/dispatcher.ts:53-59`:

```ts
case 'ConfigReloadError':
  connectionStore.markConfigReloadError(frame.reason)
  break
```

The `console.warn` is removed entirely — the banner replaces the operator-visible-warning intent.

### Banner component

New file: `frontend/src/lib/components/ConfigReloadErrorBanner.svelte`.

Structure mirrors `VersionMismatchBanner.svelte`:

- `visible = $derived(connectionStore.configReloadError !== null)`
- Mounted as a sibling in `AppShell.svelte`, between `<TopBar />` and `<main>` (next to `VersionMismatchBanner`).
- `role="status"` + `aria-live="polite"` + **`aria-atomic="true"`** + `aria-label` (match `VersionMismatchBanner`'s accessibility shape — assertive announcements would be hostile to non-operator viewers; `aria-atomic="true"` is **required** so a last-wins reason replacement triggers a full re-announcement rather than AT-dependent partial output).
- Tint: `color-mix(in oklch, var(--surface) 94%, var(--failed) 6%)` — the `--failed` analogue of `VersionMismatchBanner`'s `--queued` tint. **Caveat:** in light mode `--surface` is `oklch(99% 0.013 hue)` and `--failed` is `oklch(48% 0.18 25)`; a 6% mix may read as nearly-untinted white. Unlike `VersionMismatchBanner`, this banner has no failed-tone countdown text or bar to carry the cue. The implementer MUST verify the light-mode tint visually during step 6 and bump to 8–10% if the cue is too subtle; the test of "operator notices it across the room" is the bar.
- Glyph: `✗` in `--text-dim` (per impeccable rule: status colors are the only high-chroma elements; tint goes on the surface, not the glyph). **`✗` not `⚠`** — `.impeccable.md` § "Status Symbols" assigns `✗` to the **Failed** state and `⚠` to **ActionRequired**; pairing `⚠` with `--failed` would break color+symbol duality (Design Principle 2). No side stripe (impeccable absolute ban).
- Copy: `Config reload failed on server: <reason>` — show `reason` verbatim.
- Close button (`Button` from `$lib/components/ui/button` with `variant="ghost"`, `size="icon-sm"`, `aria-label="Dismiss"`, slotting an `X` glyph or Lucide-style icon) → `connectionStore.dismissConfigReloadError()`.
- Entrance animation: same `ease-out-expo 220ms` slide-up-and-fade (lift the `@keyframes banner-in` block; or factor it into a shared CSS file if a third consumer arrives later — not now).
- No countdown bar, no countdown text — the 60s timer is opaque.
- `prefers-reduced-motion`: respect it by disabling the entrance animation (same `@media (prefers-reduced-motion: reduce)` block).

### AppShell wiring

Add `<ConfigReloadErrorBanner />` **above** `<VersionMismatchBanner />` in `AppShell.svelte` (i.e., as the first child after `<TopBar />`, before `<VersionMismatchBanner />`). Two independent banners can be visible simultaneously (rare but possible: ConfigReloadError during a deploy that also bumped the server version). Rationale for ordering: the ConfigReloadError is the more immediately actionable item (operator can fix the file now); the VersionMismatch is a passive deploy notice with its own 30s auto-reload. Putting the error on top draws attention without burying the deploy notice (its own countdown remains visible).

### Rejected alternatives

- **Generic `adminAlerts` framework with a typed alert list.** Rejected: two consumers is not enough signal to justify the abstraction; a typed list would just be two parallel renderers wrapped in a `{#each}`. The cost of premature abstraction here is much larger than the cost of factoring later if a third consumer arrives.
- **Dismiss banner on next `ConfigUpdate`.** Rejected per Locked Decisions — staleness window after fix is bounded at 60s already, and the issue's original spec was overridden by the user during planning to favor a wall-clock timer.
- **TopBar-inline chip near the runner pools.** Rejected: the issue's "open question" is settled by reusing the AppShell banner slot that already houses `VersionMismatchBanner`. Operators learn one place to look for server-side notices.
- **Component-side `$effect` for the auto-dismiss timer.** Rejected: see "Why store-side timer" above.

## Implementation Steps

### 1. Create feature branch and commit the plan

Per `docs/planning-workflow.md` § "Finalize and Hand Off":

- Create a feature branch from `main`: `feat/config-reload-error-banner` (or operator-chosen variant).
- Copy `~/.claude/plans/reflective-tinkering-journal.md` to `docs/design-plans/2026-05-18-config-reload-error-banner.md`.
- Commit the plan to the feature branch with a conventional-commits message such as `docs: design plan for ConfigReloadError admin alert banner`.
- All subsequent steps land on this branch.

### 2. Write failing tests

Add to three existing test files (verified present at `frontend/src/lib/stores/connection.test.ts`, `frontend/src/lib/dispatcher.test.ts`, and a new browser-mode file alongside `VersionMismatchBanner.browser.test.ts`). All new tests should fail before step 3 lands:

- `frontend/src/lib/stores/connection.test.ts` covering:
  - `markConfigReloadError("missing key X")` sets `configReloadError` to that string.
  - Calling `markConfigReloadError` twice replaces the value (single-slot, last-wins).
  - With Vitest fake timers, `vi.advanceTimersByTime(60_000)` clears `configReloadError` to `null`.
  - A second `markConfigReloadError` mid-display resets the 60s deadline (advancing 30s before the second mark + 30s after the second mark should still show the error; advancing 60s after the second mark clears it).
  - `dismissConfigReloadError()` clears state immediately and any pending timeout.
  - `destroy()` clears the timeout.

- `frontend/src/lib/dispatcher.test.ts` covering:
  - Dispatching `{ kind: 'ConfigReloadError', reason: 'bad yaml' }` calls `connectionStore.markConfigReloadError('bad yaml')`.
  - The dispatch path no longer touches `console.warn` for this kind (regression guard: `expect(consoleWarnSpy).not.toHaveBeenCalled()` for that frame).

- `frontend/src/lib/components/ConfigReloadErrorBanner.browser.test.ts` (new file, browser-mode Vitest, Playwright chromium, mirroring `VersionMismatchBanner.browser.test.ts`) covering:
  - Hidden when `connectionStore.configReloadError === null`.
  - Visible with the reason text when state is set, with `role="status"`, `aria-live="polite"`, and `aria-atomic="true"`.
  - Clicking the close button (queryable via its `aria-label="Dismiss"`) calls `dismissConfigReloadError` and the banner disappears.
  - `prefers-reduced-motion`: follow the **structural** pattern at `VersionMismatchBanner.browser.test.ts:108-123` — query `window.matchMedia('(prefers-reduced-motion: reduce)').matches` and assert that a specific motion-bearing element (e.g., an element gated by `{#if !reduceMotion}` in the markup, marked with a stable `data-*` attribute) is absent when reduce matches and present otherwise. Do NOT assert computed CSS properties — the global `app.css` reset uses `!important` on `prefers-reduced-motion`, which makes property-level assertions fragile.

- `frontend/e2e/config-hot-reload.test.ts` — **modify the existing test at lines 106-134** (`'ConfigReloadError WireFrame fires console.warn without breaking the dashboard'`). Replace the `console.warn` assertion with banner-based assertions: after `sendWS(... ConfigReloadError ...)`, expect a banner with `role="status"` containing the reason text to become visible; clicking the dismiss button hides it. Rename the test to reflect the new behavior. The previous "doesn't break the dashboard" intent is preserved by the existing meter visibility check.

### 3. Implement the store extension and dispatcher swap

- Extend `ConnectionStore` per the Architecture section (new `$state`, `markConfigReloadError`, `dismissConfigReloadError`, timeout management, `destroy()` update).
- Replace the dispatcher's `case 'ConfigReloadError'` branch with the single `connectionStore.markConfigReloadError(frame.reason)` call. Remove the `console.warn` and its `biome-ignore` directive.
- Confirm the failing tests from Step 2 now pass.

### 4. Implement the banner component and mount it

- Create `frontend/src/lib/components/ConfigReloadErrorBanner.svelte` per the Architecture section.
- Mount it in `AppShell.svelte` adjacent to `<VersionMismatchBanner />`.
- Confirm the browser-mode tests pass and there are no svelte-check errors.

### 5. Update wire-contract docstring and regenerate types

- Edit `backend/crates/atc-server/src/ws.rs:50-51`. The current `ConfigReloadError` rustdoc says "Informational; the frontend logs and waits for the next successful reload." Replace with a contract-only wording that does not assert specific frontend behavior — the frontend doc owns UI surfacing. Suggested replacement: `/// - ConfigReloadError — reload failed on the server. The server keeps serving the last-known-good capacities; the wire \`reason\` is a human-readable string (\`err.to_string()\`), not a category enum. Frontend handling is owned by docs/architecture/frontend-app.md.`
- Run `just types` from the repo root to regenerate `frontend/src/lib/types/generated/WireFrame.ts` (the rustdoc comment is preserved into the generated TS by ts-rs). The TS payload shape itself does not change; only the comment refreshes.

### 6. Update architecture docs and CLAUDE.md

Per the Documents to Update section.

### 7. Manual + automated verification

- `pnpm test`, `pnpm check`, `pnpm lint` from `frontend/` — all green.
- `pnpm test:e2e` from `frontend/` — the updated `config-hot-reload.test.ts` passes; no regression elsewhere.
- `cargo nextest run -p atc-server` — backend remains green (the `ws.rs` docstring edit is comment-only).
- `pnpm dev` in `frontend/`, `cargo run -p atc-server` with the dev runner-pool config.
- Edit the runner-pool config file to introduce a YAML parse error; observe the banner appears, shows the backend error string, and remains for ~60 seconds.
- Click the close button; the banner disappears immediately.
- Edit the file again to introduce a different validation error before the timer expires; the visible reason updates and the timer resets.
- Fix the file; observe the runner-pool capacities update (`ConfigUpdate` arrives) but the banner stays until it auto-dismisses or is closed (per Locked Decisions).
- Test with `prefers-reduced-motion: reduce` in Chrome devtools; the entrance animation is disabled.
- **Light-mode visual check** (per the tint caveat in Architecture): switch to light mode, trigger the banner, and confirm the `--failed` tint reads as a clearly tinted strip — not as nearly-untinted white. If it does, bump the mix percentage from 6% to 8–10% and re-verify across all four theme hues.
- Open the dashboard in a tab where the operator is not the viewer; observe the banner reads as informational, not alarming.

## Acceptance Criteria

- **AC1** — A `ConfigReloadError` frame received post-snapshot causes the `ConfigReloadErrorBanner` to become visible within one RAF, displaying the backend's `reason` string verbatim. Verified by the browser-mode test added in Implementation Step 2.
- **AC2** — The banner uses the `--failed` design-system token for its tint and the `✗` glyph in `--text-dim` (no side stripe, glyph not in the tone color, symbol matches the failed-state assignment in `.impeccable.md`). Verified by visual inspection (step 7) and a browser-mode test that asserts the banner's `background-color` style attribute contains `var(--failed)` and the rendered glyph is `✗`.
- **AC3** — Clicking the close button clears `connectionStore.configReloadError` to `null` immediately and removes any pending auto-dismiss timeout. Verified by unit test + browser-mode test.
- **AC4** — The banner auto-dismisses exactly 60 seconds after the most recent `markConfigReloadError` call. Verified by unit test with `vi.advanceTimersByTime`.
- **AC5** — A second `ConfigReloadError` arriving mid-display replaces the visible reason (last-wins) AND restarts the 60s timer. Verified by unit test.
- **AC6** — A `ConfigUpdate` arriving while the banner is visible does NOT clear the banner (no extra dismissal trigger beyond manual close + 60s timer). Verified in `frontend/src/lib/dispatcher.test.ts`: call `connectionStore.markConfigReloadError('x')`, then dispatch a `ConfigUpdate` frame via `eventDispatcher.dispatch`, then assert `connectionStore.configReloadError === 'x'`.
- **AC7** — Pre-snapshot `ConfigReloadError` frames remain dropped at `connection.ts:114-145` — the banner does not appear during initial connection. Verified by **preserving** the existing test at `frontend/src/lib/connection.config.test.ts:92` (`'pre-snapshot ConfigReloadError is dropped silently'`) and confirming it continues to pass post-implementation. No new test required; this AC is a regression guard.
- **AC8** — The temporary issue reference is removed from production code: `git grep "issues/203" frontend/src frontend/e2e` returns zero hits after implementation. (The plan file lives under `docs/design-plans/` and `~/.claude/plans/`, which are outside the grep scope.)
- **AC9** — Banner accessibility: `role="status"` + `aria-live="polite"` + `aria-atomic="true"` + an `aria-label` describing the banner's purpose; the close button has an accessible name (`aria-label="Dismiss"`). Verified by browser-mode test using `@testing-library/svelte`'s `getByRole` plus explicit attribute assertions for `aria-atomic` and `aria-live`.
- **AC10** — The existing E2E at `frontend/e2e/config-hot-reload.test.ts:106-134` is updated to assert the banner-based behavior (not `console.warn`) and continues to pass via `pnpm test:e2e`.
- **AC11** — `backend/crates/atc-server/src/ws.rs:50-51`'s `ConfigReloadError` rustdoc is updated to contract-only wording (no claim about specific frontend behavior); `just types` regenerates `frontend/src/lib/types/generated/WireFrame.ts` with the matching comment refresh. The wire-payload shape is unchanged.
- **AC12** — All previously-passing tests still pass (`pnpm test`, `pnpm check`, `pnpm lint`, `pnpm test:e2e`, `cargo nextest run -p atc-server`). No regression in `VersionMismatchBanner` tests.

## Documents to Update

| Document | Change |
|----------|--------|
| `docs/architecture/frontend-app.md` | (a) Update the "Outer-kind switch" prose (around line 583) to describe `ConfigReloadError` calling `connectionStore.markConfigReloadError` and surfacing as `ConfigReloadErrorBanner`, replacing the current "fires a single `console.warn` referencing issue #203 (UI surfacing is deferred to that issue)" line. (b) Add a "ConfigReloadErrorBanner" subsection to the existing "Banner UX" section (around line 552) describing the 60s wall-clock auto-dismiss, manual close, single-slot last-wins, and `--failed` treatment. (c) Update the App Shell component tree to mention the second banner sibling. (d) Update the ConfigReloadError handling note around lines 588-589 to remove the "informational only" framing (which becomes inaccurate after this PR). |
| `frontend/CLAUDE.md` | Update the `src/lib/dispatcher.ts` row in the Key Files table (currently says `ConfigReloadError → console.warn referencing issue #203`) to say `ConfigReloadError → connectionStore.markConfigReloadError(reason)`. Update the `src/lib/stores/` row to add `configReloadError` $state + `markConfigReloadError` / `dismissConfigReloadError` methods on `ConnectionStore`. No new sharp-edges section unless something surprising surfaces during implementation. |
| `backend/crates/atc-server/src/ws.rs` | Update the `ConfigReloadError` rustdoc at lines 50-51 to contract-only wording (drop the "Informational; the frontend logs and waits for the next successful reload" sentence, which becomes false after this PR). Suggested replacement is in Implementation Step 5. The wire-payload shape is **not** changing. |
| `frontend/src/lib/types/generated/WireFrame.ts` | Regenerate via `just types` after the `ws.rs` edit. This is a generated file and must never be hand-edited; the docstring refresh flows through ts-rs automatically. |
| `frontend/e2e/config-hot-reload.test.ts` | Rewrite the `ConfigReloadError WireFrame fires console.warn without breaking the dashboard` test (lines 106-134) per Implementation Step 2. |
| `scripts/doc-mapping.sh` | No change. `frontend/src/*` already maps to `docs/architecture/frontend-app.md`; `backend/crates/atc-server/src/ws.rs` falls under the `backend/crates/atc-server/src/*` catch-all → `backend/crates/atc-server`-owning architecture doc; `frontend/e2e/` is not mapped (E2E tests are not subject to the doc-staleness gate). The gate is satisfied by editing `frontend-app.md` alongside the source changes. |
| `CLAUDE.md` (root) | No change. The Documentation Map row for frontend already points at the architecture doc. |
| `docs/architecture/backend-server.md` | No change expected — the WireFrame § documents wire-level invariants (envelope shape, sequencing, broadcast cadence), not frontend UI surfacing. If the implementer finds it makes claims about ConfigReloadError frontend behavior, update those claims to point at frontend-app.md. |
| ADRs | None — no architectural-decision-record-worthy change; this is a feature consuming an existing pattern. |

## Out of Scope

- A general `adminAlerts` framework or alert-list UI. Deferred until a third consumer arrives (see Locked Decisions and `docs/architecture/frontend-app.md` § Banner UX for the future-pattern note).
- Persisting `ConfigReloadError` state across reconnects (the backend has no "last known error" snapshot field; a reconnect after a failure misses the event). Owner: would require a backend snapshot change, out of scope for #203.
- Surfacing the categorical `{read|parse|validate}` reason in the UI. The wire payload is a free-form `String`; promoting the category to the wire would be a separate backend change tracked as a follow-up if operators report needing it.
- Showing a per-pool indicator of which pool failed to load. The `reason` string already includes the relevant context from the backend's error format.
- Re-tuning `--failed`'s light-mode variant itself. The mix-percentage adjustment called out in Architecture (bump 6%→8–10% if visual review demands) is in scope; changing the token's OKLCH definition in `app.css` is not — that's a design-system change tracked separately if ever needed.

## Glossary

- **WireFrame** — the outer envelope of every WS message, with a `kind` discriminator. Variants: `Committed`, `ConfigUpdate`, `ConfigReloadError`, `ServerHello`, `GoingAway`. Defined at `backend/crates/atc-server/src/ws.rs:73-91`, generated to TypeScript at `frontend/src/lib/types/generated/WireFrame.ts:35`.
- **Single-slot, last-wins** — the store holds one current value; an incoming value replaces it. Mirrors `connectionStore.serverVersionMismatch`'s behavior.
- **Snapshot / pre-snapshot** — the initial REST `/v1/state` fetch establishes the dashboard's baseline. Frames received before that fetch returns are "pre-snapshot" and are routed through `connection.ts`'s buffering logic, not the dispatcher.
