# Protocol Version Handshake and GoingAway Envelope

**Issue:** [#47](https://github.com/bojanrajkovic/atc/issues/47) — feat(frontend/server): protocol version handshake to trigger refresh on backend incompatibility
**Branch:** `feat/protocol-version-handshake` off `main` (tip: 884fcb8)
**PR target:** `main`
**PR title (squash commit subject):** `feat: protocol version handshake and GoingAway envelope`
**Implementation guidance:** `docs/implementation-guidance.md` governs all implementation work for this plan.

## Context

ATC ships frontend and backend as a single binary (frontend embedded via `rust-embed`). After a deploy with a breaking wire-contract change, open browser tabs continue running the prior frontend bundle against the new backend, and the dashboard breaks until the operator manually refreshes. ADR 0003 explicitly flagged this as deferred work: *"A future enhancement could automate this by sending a build version through the WebSocket and triggering a refresh on mismatch — tracked separately, not in scope for this ADR."* The `last_seq` cursor rename in ADR 0003 Phase 3a is the first concrete breaking-wire-change that exposes the papercut, but the design must work for any future breaking change too.

Issue #60 (graceful shutdown, see `docs/design-plans/2026-05-09-supervision-and-shutdown.md`) shipped `Close(CloseFrame { code: 1001, reason: "going away" })` at `backend/crates/atc-server/src/ws.rs:129-134` and explicitly deferred any envelope-level shutdown messaging to this plan. The #47 issue thread raised the fork: keep going-away as a transport-level signal only, or introduce an application-level `WireFrame::GoingAway` variant so the frontend can show a tailored "server restarting" state before disconnect. The user has chosen the application-level variant.

## Definition of Done

**Primary deliverables:**
1. Backend sends `WireFrame::ServerHello { version }` as the first text frame on every WS connect, carrying `env!("VERGEN_GIT_DESCRIBE")` (the existing operator-facing version identifier already used by `atc_build_info`).
2. Backend sends `WireFrame::GoingAway { reason }` immediately before `Close(1001)` on graceful shutdown.
3. Frontend stores the version from the first `ServerHello` in a tab session as the **session reference**. On every subsequent reconnect, if the wire version differs from the reference AND has not been superseded by an explicit "Refresh now", a banner with a 30-second countdown appears. The countdown autoreloads at zero; a "Refresh now" button skips the wait.
4. Frontend's connection indicator (`ConnectionIndicator.svelte`) shows a tailored "Server restarting…" state while `connectionStore.serverGoingAway === true`, flipped on `GoingAway` receipt and cleared on the next successful `connected` transition.
5. The pre-snapshot close race in `ConnectionManager.connect()` is fixed (`handleDisconnect` aborts in-flight fetches) so `GoingAway`-triggered closes never leave the UI stuck in a false-`connected` state against a dead socket.

**Success criteria:**
- An open tab connected through a redeploy sees the banner within ≤ ~3s (first reconnect backoff step) and is hard-reloaded within ~33s unless the user clicks "Refresh now" sooner.
- No regressions to pre-snapshot buffering, reconnect backoff, RAF coalescing, or ARIA announcements.
- Dev-server / Vite dev mode and tests do not surface the banner (no spurious mismatches).
- All existing tests pass; new tests cover the happy-path, the mismatch + countdown + auto-reload path, the "Refresh now" path, the `GoingAway` → indicator flip, and the abort-on-disconnect race.

**Exclusions (out of scope):**
- Build-time injection of any version constant into the frontend bundle (no `__BUILD_VERSION__`, no Vite `define`, no `ATC_BUILD_VERSION` env-var orchestration, no Dockerfile ARG).
- Separate REST API or protocol-version integer.
- HTTP `/v1/version` endpoint.
- Cross-binary protocol negotiation.
- Initial-load-during-deploy detection (loading the frontend bundle from version X and immediately connecting to a backend at version Y is not caught by the session-reference model; user hard-refresh covers it).
- Differentiated `GoingAway` reasons beyond `"server shutdown"`.
- Persistence of the session reference across browser tab refreshes (in-memory only).
- Snooze / dismiss UX (no user-initiated countdown extension).
- ADR for this work.

## Locked Decisions

- **Session-reference version tracking, NO build-time version baked into the frontend bundle.** First `ServerHello` in a tab session sets the reference; later mismatches trigger the banner. No `vite.config.ts` change, no env var, no Dockerfile/CI changes. Confirmed in clarification round.
- **30-second hard countdown with "Refresh now" button. No dismiss, no snooze.** Confirmed in clarification round; matches the "assume backend contract could break anytime" framing.
- **`GoingAway` envelope variant ships now,** sent immediately before the existing Close-1001 frame. Confirmed in clarification round.
- **Backend `version` field source is `env!("VERGEN_GIT_DESCRIBE")`** — the same identifier already exposed by `atc_build_info` (`backend/crates/atc-server/src/metrics.rs:21`). No change to backend's build pipeline.
- **Banner mounted in `AppShell.svelte`** between `<TopBar>` and `<main>`. Selected for correct layout and predictable focus order.
- **Banner owns its own ARIA live region (`aria-live="polite"` on its container).** Does NOT extend `LiveRegion` at `frontend/src/lib/aria/live-region.svelte.ts:21` — that channel is workflow-event-specific (burst accumulator, "Workflow run updates" label).
- **Banner component is custom** (Tailwind primitives + existing shadcn-svelte `Button` and `Progress`). No new shadcn dependency. Matches the minimalism of `ConnectionIndicator.svelte`.
- **Dispatcher's `default` arm stays warn-and-skip** (`frontend/src/lib/dispatcher.ts:59-70`) — intentional forward-compat for rolling deploys. New variants are covered by explicit tests, not compiler exhaustiveness.
- **Lockstep deploy semantics** (ADR 0003 § "Context") — binary uniquely determines both halves; this work automates refresh on lockstep change.
- **Close-1001 stays.** Transport-level Close frame remains the authoritative going-away signal; `GoingAway` is additive metadata sent immediately before.

## Architecture

### Wire envelope additions

Two new variants on `WireFrame` (`backend/crates/atc-server/src/ws.rs:60-73`):

```rust
ServerHello {
    version: String,        // env!("VERGEN_GIT_DESCRIBE")
},
GoingAway {
    reason: String,         // "server shutdown" for SIGTERM; freeform
},
```

Both inherit the existing `#[serde(tag = "kind")]` and `#[ts(export)]` so they regenerate into `frontend/src/lib/types/generated/WireFrame.ts` via `just types`. `ServerHello.version` is a freeform string compared with strict equality on the frontend. `GoingAway.reason` is informational (frontend reads it for logs / dev tooltip; visual UX is tone-only).

Rejected alternatives: separate `protocol_version` integer (CI-discipline burden); WS upgrade response header (browser `WebSocket` API does not expose response headers to JS); HTTP `/v1/version` endpoint (round-trip duplication; `atc_build_info` already serves operator probing).

### Backend lifecycle changes

`handle_socket` (`backend/crates/atc-server/src/ws.rs:118-209`) gains two edits:

1. **Before the `select!` loop** (between `tracing::info!("WebSocket client connected")` at line 124 and the `loop` at line 126): synchronous `send_frame(&mut socket, &WireFrame::ServerHello { version: env!("VERGEN_GIT_DESCRIBE").to_string() }).await`. On send failure, log + return. Broadcast receivers are created before the upgrade completes (`ws.rs:80-81`); events that fire between subscription and the first `send_frame` accumulate in the broadcast buffer (capacity 256) and drain via the `select!` loop after `ServerHello` ships. One task owns the socket — ordering invariant holds without additional synchronization.

2. **In the `shutdown.cancelled()` arm** (`ws.rs:129-134`): prepend `send_frame(&mut socket, &WireFrame::GoingAway { reason: "server shutdown".into() }).await` before the existing `Message::Close` send. Both sends best-effort (`let _ = ...`). `axum::extract::ws::WebSocket` writes through `futures::Sink::send` which queues then flushes; tungstenite emits the Close frame after the prior text frame is queued.

### Frontend version tracking

**`versionMismatchStore` (new rune-class store)** at `frontend/src/lib/stores/version-mismatch.svelte.ts`:

```typescript
class VersionMismatchStore {
  /** First ServerHello.version seen in this tab session; null until first connect. */
  reference = $state<string | null>(null)
  /** Latest observed version distinct from reference; drives banner visibility. */
  observed = $state<string | null>(null)
  /** Countdown deadline (epoch ms) — null while no banner is showing. */
  reloadAt = $state<number | null>(null)

  observe(serverVersion: string): void {
    if (this.reference === null) {
      this.reference = serverVersion
      return
    }
    if (serverVersion === this.reference) {
      // Reconnect to same backend — keep countdown if it was already running
      // against a different observed value; otherwise no-op.
      return
    }
    // New mismatch detected. If banner is not already showing, arm the countdown.
    if (this.observed !== serverVersion) {
      this.observed = serverVersion
      this.reloadAt = Date.now() + 30_000
    }
  }

  refreshNow(): void {
    window.location.reload()
  }
}
```

In-memory only; refresh wipes the store, and after refresh, the new bundle's first `ServerHello` matches the running backend, so no banner.

**Dispatcher additions** (`frontend/src/lib/dispatcher.ts:36`):
- `case 'ServerHello'`: call `versionMismatchStore.observe(frame.version)`. Bypasses RAF (apply immediately).
- `case 'GoingAway'`: call `connectionStore.markGoingAway(frame.reason)`. Bypasses RAF.

The `default` arm at `dispatcher.ts:59-70` stays warn-and-skip (intentional forward-compat).

**Pre-snapshot switch additions** (`frontend/src/lib/connection.ts:105-126`): add `case 'ServerHello'` and `case 'GoingAway'` arms calling the same store methods. Both apply immediately; neither is buffered for post-snapshot replay (the version check is snapshot-independent; `GoingAway` arrives once and is transient).

**`connectionStore` updates** (`frontend/src/lib/stores/connection.svelte.ts`): add `serverGoingAway: boolean` + `goingAwayReason: string | null` + `markGoingAway(reason)`. Reset both on the next `connected` transition (`connection.ts:209`).

### Pre-snapshot close-race fix

`handleDisconnect()` (`connection.ts:225-253`) currently does NOT abort the in-flight fetch from a prior `connect()`. If the socket closes during `/v1/state` fetch, `handleDisconnect()` runs (transition to `reconnecting`, schedule retry) AND the original fetch can complete and execute `connectionStore.status = 'connected'` at `connection.ts:209` against a dead socket. `GoingAway` makes this path likely on every redeploy.

Fix: in `handleDisconnect()` (`connection.ts:225`), invoke `this.abortController?.abort()` before scheduling the reconnect timer. The fetch chain already bails on `signal.aborted` at `connection.ts:155` and `:165`. `destroy()` and `reconnect()` already abort; this brings `handleDisconnect()` in line.

### Banner component

`frontend/src/lib/components/VersionMismatchBanner.svelte`:
- Visible when `versionMismatchStore.observed !== null && versionMismatchStore.reloadAt !== null`.
- Subscribes to a 1Hz tick (small `setInterval` started by the component on mount, cleared on destroy) to compute `remainingSeconds = Math.max(0, Math.ceil((reloadAt - Date.now()) / 1000))`.
- When `remainingSeconds === 0`, calls `versionMismatchStore.refreshNow()` (which calls `window.location.reload()`).
- Renders the locked design (countdown + "Refresh now" button, no dismiss/snooze). Final visual tuned during the playground + impeccable design pass — see Implementation Steps.
- Mounted in `AppShell.svelte` between `<TopBar>` and `<main>`.
- Reuses existing shadcn-svelte `Button` from `frontend/src/lib/components/ui/button/` and `Progress` from `frontend/src/lib/components/ui/progress/` (for the countdown bar).
- Container is `<aside role="status" aria-live="polite" aria-atomic="true">` so screen readers announce the message without overriding the workflow-update live region.

### Connection indicator going-away state

`ConnectionIndicator.svelte` reads `connectionStore.serverGoingAway`. When true, tooltip text becomes "Server restarting — reconnecting…" and the indicator's visual variant is the existing `reconnecting` tone (no new color needed; the `GoingAway`-triggered close transitions the store status to `reconnecting` ~immediately anyway). The flag is informational metadata that lets the tooltip say "restarting" instead of generic "reconnecting"; everything else carries through the existing reconnect path.

## Implementation Steps

### Branch setup and plan handoff

- Create feature branch `feat/protocol-version-handshake`.
- Copy `/Users/brajkovic/.claude/plans/plan-implementation-of-https-github-com-rosy-flurry.md` to `docs/design-plans/2026-05-18-protocol-version-handshake.md`.
- Commit on the feature branch (`docs: add design plan for issue #47`).

### Banner design exploration

- Use the `playground` skill to build a single-file HTML playground with controls for color, spacing, copy, countdown bar style, and motion. Iterate until the visual is locked.
- Run the `impeccable` skill against the locked playground to surface accessibility (focus order, ARIA, reduced-motion, color contrast against `--surface`), motion, copy, and visual-hierarchy issues. Apply findings.
- Approved design is captured as Tailwind classes + structure in a comment block at the top of the eventual `VersionMismatchBanner.svelte` (no separate design doc).

### Failing tests

- **Backend integration test** (`backend/crates/atc-server/tests/integration/`): WS open → first frame is `{"kind":"ServerHello","version":"..."}` with non-empty `version` equal to `env!("VERGEN_GIT_DESCRIBE")`.
- **Backend integration test**: cancel the shutdown token → next two frames are `{"kind":"GoingAway","reason":"server shutdown"}` then a Close-1001.
- **Frontend unit test** (Vitest): `versionMismatchStore.observe('v1')` then `observe('v1')` again leaves `reference === 'v1'`, `observed === null`, `reloadAt === null`. `observe('v1')` then `observe('v2')` sets `observed === 'v2'` and `reloadAt` ≈ 30s in the future.
- **Frontend unit test**: dispatcher calls `versionMismatchStore.observe` on `ServerHello` and `connectionStore.markGoingAway` on `GoingAway`.
- **Frontend unit test**: socket close during `/v1/state` fetch never lands `connectionStore.status === 'connected'`; the abort-fix regression test.
- **Frontend component test** (Vitest browser): `VersionMismatchBanner.svelte` is hidden when `observed === null`; visible when `observed !== null`; countdown decrements with virtual timers; "Refresh now" invokes a reload spy; at `remainingSeconds === 0` the reload spy fires automatically.
- **Frontend component test**: `ConnectionIndicator.svelte` shows the "Server restarting — reconnecting…" tooltip when `connectionStore.serverGoingAway === true`.
- **E2E test** (Playwright, using `frontend/e2e/lib/ws-mock.ts` helpers): inject two ServerHello frames with different versions, assert banner appears, click "Refresh now", assert reload.

These tests all fail initially.

### Envelope variants and TS regeneration

- Add `ServerHello { version: String }` and `GoingAway { reason: String }` to `WireFrame` at `backend/crates/atc-server/src/ws.rs:60-73` with matching doc comments.
- Run `just types` to regenerate `frontend/src/lib/types/generated/WireFrame.ts`.

### Backend emit sites

- In `handle_socket` (`ws.rs:118`), before the `loop` at line 126: send `WireFrame::ServerHello { version: env!("VERGEN_GIT_DESCRIBE").to_string() }` via `send_frame`. On error, log + return.
- In the `shutdown.cancelled()` arm (`ws.rs:129-134`): prepend `send_frame(&mut socket, &WireFrame::GoingAway { reason: "server shutdown".into() }).await` before the existing `Message::Close` send. Best-effort.
- Backend integration tests pass.

### Frontend dispatcher, store, and abort-race fix

- Create `versionMismatchStore` at `frontend/src/lib/stores/version-mismatch.svelte.ts` per Architecture.
- Extend `connectionStore` with `serverGoingAway`, `goingAwayReason`, `markGoingAway`. Reset both on the `connected` transition at `connection.ts:209`.
- Add `case 'ServerHello'` and `case 'GoingAway'` arms to `dispatcher.ts:36` and to `connection.ts:105-126`.
- In `handleDisconnect()` (`connection.ts:225`), call `this.abortController?.abort()` BEFORE scheduling the reconnect timer.
- Frontend store, dispatcher, and abort-race unit tests pass.

### Banner component and indicator wiring

- Create `VersionMismatchBanner.svelte` matching the playground+impeccable-approved design. Implement the 1Hz tick, the auto-reload at zero, and the "Refresh now" button. Use existing `Button` and `Progress` primitives.
- Mount the banner in `AppShell.svelte` between `<TopBar>` and `<main>`.
- Update `ConnectionIndicator.svelte` to read `connectionStore.serverGoingAway` and switch tooltip text accordingly.
- Component tests and E2E test pass.

### Documentation updates

- `docs/architecture/backend-server.md` — WS section: new envelope variants; ServerHello-on-connect + GoingAway-before-Close-1001 lifecycle.
- `docs/architecture/frontend-app.md` — new "Deploy detection" subsection: session-reference model, dispatcher arms, banner UX, countdown + auto-reload, `versionMismatchStore`, `serverGoingAway` flag, indicator variant, abort-on-disconnect race fix.
- `backend/crates/atc-server/CLAUDE.md` — `ws` Modules row updated for five `WireFrame` variants + on-connect handshake.
- `frontend/CLAUDE.md` — no proactive change. Reactive sharp-edge entry only if friction surfaces.
- `scripts/doc-mapping.sh` — no new entries; existing catch-alls cover all changed files.
- No ADR. No release-pipeline.md change. No metrics.md change.

## Acceptance Criteria

- **AC1** — On every WS open, the frontend receives a frame with `kind === 'ServerHello'` and a `version` string equal to the backend's `env!("VERGEN_GIT_DESCRIBE")` (verified by backend integration test).
- **AC2** — On graceful shutdown, the frontend receives `{"kind":"GoingAway","reason":"server shutdown"}` immediately before the Close-1001 transport frame (verified by backend integration test).
- **AC3** — The first `ServerHello` in a tab session sets `versionMismatchStore.reference` and does NOT show the banner (verified by unit test).
- **AC4** — A subsequent `ServerHello` with a `version` different from `reference` sets `observed` and `reloadAt ≈ Date.now() + 30_000`, making the banner visible (verified by unit test).
- **AC5** — A subsequent `ServerHello` with `version === reference` (reconnect to same backend) does not arm or rearm the countdown (verified by unit test).
- **AC6** — The banner renders the locked design (per playground+impeccable pass) with a decrementing countdown; at `remainingSeconds === 0` it calls `window.location.reload()` (verified by component test with virtual timers).
- **AC7** — Clicking "Refresh now" calls `window.location.reload()` immediately (verified by component test with a reload spy).
- **AC8** — On receiving `GoingAway`, `connectionStore.serverGoingAway === true` and `goingAwayReason` carries the wire `reason`; both reset on the next successful `connected` transition (verified by unit test).
- **AC9** — When `connectionStore.serverGoingAway === true`, `ConnectionIndicator.svelte` shows the "Server restarting — reconnecting…" tooltip (verified by component test).
- **AC10** — A socket close during `/v1/state` fetch never leaves `connectionStore.status === 'connected'` afterward (verified by unit test that races a `ws.close()` against an in-flight fetch and asserts the final status is `reconnecting`).
- **AC11** — Existing tests still pass: pre-snapshot buffering, reconnect backoff, RAF coalescing, ARIA announcements, snapshot `lastSeq` cursor (verified by full `just test` run).

## Documents to Update

| Doc | Change |
|-----|--------|
| `docs/architecture/backend-server.md` | WS section: new envelope variants; ServerHello-on-connect + GoingAway-before-Close-1001 lifecycle. |
| `docs/architecture/frontend-app.md` | New "Deploy detection" subsection: session-reference model, dispatcher arms, banner UX, countdown + auto-reload, `versionMismatchStore`, `serverGoingAway`, `ConnectionIndicator` variant, abort-on-disconnect race fix. |
| `backend/crates/atc-server/CLAUDE.md` | `ws` Modules row updated for five `WireFrame` variants + on-connect handshake. |
| `frontend/CLAUDE.md` | No proactive change. |
| `scripts/doc-mapping.sh` | No new entries. |

## Out of Scope

- Build-time version baked into the frontend bundle.
- Initial-load-during-deploy detection (first ServerHello sets reference unconditionally).
- HTTP `/v1/version` endpoint.
- Separate `protocol_version` integer.
- Cross-binary protocol negotiation.
- Persistent (cross-tab-refresh) banner dismissal — the design has no dismiss.
- Snooze button / countdown extension.
- Differentiated `GoingAway` reasons.
- ADR for this work.

## Glossary

- **Session reference** — The first `ServerHello.version` received in a given browser tab. Held in `versionMismatchStore.reference` until the tab is refreshed or closed.
- **Countdown** — The 30-second window between detecting a version mismatch and auto-reloading. Skippable via "Refresh now"; not pausable or dismissible.
- **`VERGEN_GIT_DESCRIBE`** — Existing build-time env var emitted by `vergen-gix` (`backend/crates/atc-server/build.rs:1-17`). Source for `ServerHello.version` and for the `git_describe` / `version` labels on `atc_build_info`.
- **`WireFrame`** — Outer-`kind`-discriminated enum at `backend/crates/atc-server/src/ws.rs:60-73`; serde-tagged, ts-rs-exported.
- **Lockstep deploy** — Invariant from ADR 0003: frontend and backend ship as one binary, so a deploy bumps both at once.
