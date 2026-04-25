# Test Plan — Live Pool Stats

Source: [docs/design-plans/2026-04-18-live-pool-stats.md](../design-plans/2026-04-18-live-pool-stats.md)
Implementation plan: [docs/implementation-plans/2026-04-18-live-pool-stats/](../implementation-plans/2026-04-18-live-pool-stats/)
Branch: `feat/live-pool-stats`
Commit at plan generation: `f2646542d452d1cf5d3e9f6be179f30ed63a0b02`

## Coverage Summary

- **Automated criteria:** 19 / 19 covered (AC1.1–AC1.8, AC2.1–AC2.6, AC3.1–AC3.5)
- **Doc-artifact criteria:** 4 / 4 covered (AC4.1–AC4.4)
- **Result:** PASS

## Prerequisites

- Branch `feat/live-pool-stats` checked out at the commit above.
- `mise install` and `just setup` have run in this worktree (verifies hooks are installed in the worktree).
- `just lint` and `just test` pass on HEAD.
- `just types` produces a clean `git diff --exit-code frontend/src/lib/types/generated/` (gates AC1.6 wire-format shape).
- `helm-unittest` plugin installed if you also want `just check` green; this is a pre-existing dev-environment gap, not a Phase regression. Phase 1–3 touch zero helm files.
- A working GitHub webhook tunnel into the local backend (smee.io or equivalent), configured against the ATC repo. The ATC repo's CI matrix exercises both `ubuntu-latest` and `ubuntu-24.04` elastic pools under the "GitHub Actions" group — that is the actual #32 reproducer.

## Phase 1 — Static gates and unit tests

| Step | Action | Expected |
|------|--------|----------|
| 1 | `cd /Users/brajkovic/Projects/atc && just lint` | Exit 0; no clippy or eslint warnings. |
| 2 | `just test` | All Rust + TS unit/browser suites pass. In particular `cargo test -p atc-server --test sidecar_tests` reports 4 tests passing; full frontend Vitest suite reports 316 tests passing across 41 files. |
| 3 | `just types && git diff --exit-code frontend/src/lib/types/generated/` | No diff produced (TS types in sync with Rust `SeqEvent`). |
| 4 | `pnpm --dir frontend test:e2e` | 27 E2E tests pass, including the new `pool-indicators.test.ts`. |

## Phase 2 — End-to-end live webhook session (sidecar update path)

Purpose: confirm the broadcast sidecar drives the TopBar pool indicators in real time on real GitHub traffic, not synthetic fixtures.

| Step | Action | Expected |
|------|--------|----------|
| 1 | Start the stack: `just dev`. | Backend at `http://localhost:3000`, Vite at `http://localhost:5173`, smee tunnel forwarding GitHub webhooks. |
| 2 | Open `http://localhost:5173` and open browser devtools Network tab; filter for `/v1/ws`. | WebSocket connection opens; first frame arrives within seconds. |
| 3 | In the browser console, run `window.__stores.connectionStore.status`. | Returns `"connected"`. |
| 4 | Trigger any CI workflow on the ATC repo (push a no-op commit on a temp branch, or rerun the most recent workflow via the GitHub UI). | Webhook delivered through smee → backend → broadcast. |
| 5 | Watch the TopBar pool indicators while the workflow lifecycle proceeds: Queued → InProgress → Completed. | Queue and run counts on the pool indicator update **in-session, without page reload or WebSocket reconnect**, as Job events flow through. |
| 6 | At completion, verify the running counter for that label set returns to its prior baseline. | Confirms the empty-stats branch (AC1.2 step 3 in fixtures) does not leak ghost runners. |

If any step is observed without the expected outcome, the sidecar wiring is broken and the bug-fix loop must reopen Phase 2.

## Phase 3 — End-to-end TopBar disambiguation session (closes #32)

Purpose: confirm the `"GitHub Actions · ubuntu-latest"` vs `"GitHub Actions · ubuntu-24.04"` rendering on real production pool composition (the direct #32 fix).

| Step | Action | Expected |
|------|--------|----------|
| 1 | With `just dev` running and a `connected` WebSocket, trigger a workflow on the ATC repo that exercises **both** `ubuntu-latest` and `ubuntu-24.04` runners (the repo's CI matrix already does this). | Two Queued Job events fire in quick succession, each with a different `LabelSet` but the same `groupName: "GitHub Actions"`. |
| 2 | Inspect the TopBar in the browser. | TopBar renders **two distinct** indicators: `"GitHub Actions · ubuntu-latest"` and `"GitHub Actions · ubuntu-24.04"`. **Not** two visually identical `"GitHub Actions"` indicators (the bug pre-fix). |
| 3 | Hover or read the indicators while the workflow runs. | Each indicator's queue/run counts evolve independently — confirming label-keyed pool tracking, not a single conflated pool. |
| 4 | Complete the workflow and verify both indicators settle back to their idle baseline. | Both return to 0/0 (or to their prior queue depth from any concurrent unrelated workflows). |

## Phase 4 — Doc artifact audit (post-merge sanity)

| Step | Action | Expected |
|------|--------|----------|
| 1 | `git log -p docs/architecture/backend-server.md` filtered to this branch. | The "Last verified" line and the SeqEvent.pool_stats_after content land in the same set of phase commits — not a bare date bump in a follow-up. |
| 2 | `git log -p docs/architecture/frontend-app.md` filtered to this branch. | Same: RunnerStore rewrite and "Last verified: 2026-04-24" co-occur. |
| 3 | `ls -la backend/crates/atc-{server,core,github}/AGENTS.md frontend/AGENTS.md`. | All four resolve as symlinks to their sibling `CLAUDE.md` (no broken links, no duplicated content). |
| 4 | Skim each touched CLAUDE.md for non-duplication: each carries a minimal pointer to the architecture doc, no copied paragraphs. | Confirms project's non-duplication invariant is honored. |

## Human Verification Items (from test-requirements.md)

| Item | Why Manual | Where |
|------|------------|-------|
| Design's done-when: Phase 2 manual session | End-to-end real-traffic confirmation cannot be replicated by any unit/E2E fixture — it validates the smee → webhook → store → broadcast → dispatcher → store → DOM path on actual GitHub deliveries. | Phase 2 above |
| Design's done-when: Phase 3 manual session (closes #32) | The bug is a visual regression on production pool composition (`ubuntu-latest` + `ubuntu-24.04` both under "GitHub Actions"); only a session against the live ATC repo CI matrix reproduces it faithfully. | Phase 3 above |

## Traceability — AC → Test → Manual Step

| Acceptance Criterion | Automated Test | Manual Step |
|----------------------|----------------|-------------|
| AC1.1 | `backend/crates/atc-server/tests/sidecar_tests.rs:141` | Phase 2 step 5 (sidecar drives TopBar updates) |
| AC1.2 | `backend/crates/atc-server/tests/sidecar_tests.rs:232` | Phase 2 step 5 + step 6 (Q→IP→C lifecycle without ghosts) |
| AC1.3 | `backend/crates/atc-server/tests/sidecar_tests.rs:362` | Phase 2 step 4 (Run events arrive without disturbing the pool indicator) |
| AC1.4 | `backend/crates/atc-server/tests/routes_tests.rs:140` | Phase 2 step 2 (initial `/v1/state` snapshot already produces sorted pools) |
| AC1.5 | `backend/crates/atc-server/tests/sidecar_tests.rs:419` | Phase 2 step 5 (no double-update on duplicate webhooks; UI does not jitter) |
| AC1.6 | `backend/crates/atc-server/tests/state_tests.rs:321,371` | Phase 1 step 3 (`just types` clean) |
| AC1.7 | `backend/crates/atc-core/src/store/tests/runner_pools.rs:376,446` | Phase 2 step 5 (TopBar order remains stable across updates) |
| AC1.8 | `backend/crates/atc-github/src/webhook/translate/tests.rs:469` | Phase 3 step 2 (no spurious `"GitHub Actions · "` empty-suffix rendering) |
| AC2.1 | `frontend/src/lib/dispatcher.test.ts:196` (with `invocationCallOrder` assertion) | Phase 2 step 5 |
| AC2.2 | `frontend/src/lib/dispatcher.test.ts:255` | Phase 2 step 4 (Run events do not clear the pool indicator) |
| AC2.3 | `frontend/src/lib/connection.connect.test.ts:108` | Phase 2 step 2 (TopBar already shows seeded pools at WS-connected time) |
| AC2.4 | `frontend/src/lib/connection.buffering.test.ts:287` | Phase 2 step 1 (early Job events queued during reconnect still apply correctly) |
| AC2.5 | `frontend/src/lib/dispatcher.test.ts:298` (with `invocationCallOrder` assertion) | Phase 2 step 5 (rapid burst still settles to last-wins) |
| AC2.6 | `frontend/src/lib/dispatcher.browser.test.ts:5` | Phase 2 step 5 (visual confirmation under real `$state` reactivity) |
| AC3.1 | `frontend/src/lib/components/TopBar.browser.test.ts:150` | Phase 3 step 2 |
| AC3.2 | `frontend/src/lib/components/TopBar.browser.test.ts:163` | Phase 3 step 2 (the unambiguous control case if you also have a non-elastic pool registered) |
| AC3.3 | `frontend/src/lib/components/TopBar.browser.test.ts:172` | Phase 3 step 2 (any self-hosted runners — ATC has none in production, but verify any Custom Runner pool falls back to joined labels) |
| AC3.4 | `frontend/src/lib/components/TopBar.browser.test.ts:180` | Not exercised live (ATC's matrix has 2; pinning `>= 2` is sufficient via the unit test) |
| AC3.5 | `frontend/src/lib/components/TopBar.browser.test.ts:195` | Not exercised live (mixed-group case is unit-tested only) |
| AC4.1 | doc artifact: `docs/architecture/frontend-app.md:225-228` | Phase 4 step 2 |
| AC4.2 | doc artifact: `docs/architecture/backend-server.md:229-241` | Phase 4 step 1 |
| AC4.3 | both architecture docs | Phase 4 steps 1 & 2 |
| AC4.4 | four CLAUDE.md files | Phase 4 steps 3 & 4 |
