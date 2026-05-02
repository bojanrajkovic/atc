# CLAUDE.md — AI Agent Index

Last verified: 2026-05-02

> **Keep this file lean.** Detailed documentation lives in `docs/`. This file provides pointers, not content. When you update a feature, update its architecture doc in `docs/architecture/` — not this file.

## Project

**ATC — Actions Traffic Control**
Real-time GitHub Actions dashboard. Rust backend (Axum) + Svelte 5 + Vite frontend.

**Status:** Core domain model in `atc-core`, GitHub webhook integration in `atc-github`, and server wiring in `atc-server` (webhook ingestion, WebSocket streaming, REST state) are implemented. Frontend complete through Sub-Phase 6a (kanban keyboard navigation): app shell with TopBar, kanban board rendering inside app shell with three-column layout (queued/in-progress/completed), run cards (StatusIcon/JobHeader/JobMeta/ProgressBar/RunnerLabel composition with state-aware duration, Warn-First tokens, density modes, reduced-motion halo), card animations via shared crossfade and FLIP, sorted derived arrays in RunStore plus jobStatsByRun aggregation, OKLCH design system with four themes, Svelte 5 rune stores with WS client and event dispatching, unit + browser-mode + E2E test coverage across all components. Live runner pool stats flow end-to-end via a `poolStatsAfter` sidecar on Job-event `SeqEvent`s (sorted lexicographically by labels), with TopBar disambiguating labels when `groupName` is shared across pools. Sub-Phase 5 added: Cmd+K command palette (Recent/Runs/Jobs/Pools/Commands sections, theme submenu), slide-over run detail panel, pool filter integration, the first branded TypeScript type (`PoolKey`, see ADR 0001), and Bits UI dialog stacking for sibling palette+panel via `defer-otherwise-close` on the panel and a sibling-combinator backdrop-suppression rule. Sub-Phase 6a added: roving-tabindex keyboard navigation across the kanban via `<RovingFocusProvider>` (first production use of Svelte 5 actions and `setContext`/`getContext`), 2D arrow + Home/End nav with no-wrap and empty-column-skip, modifier-key delegation back to the App-level window handler, structural suspension while dialogs are open, card-stable focus across FLIP/crossfade transitions, and lost-trigger restoration on panel close when the originating card was TTL-evicted. Sub-Phase 6b (polish + responsive) is the remaining frontend work for 1.0. Both stacks compile, lint, and pass CI.

## Tech Stack

- **Backend:** Rust 1.94.0 with Axum — Cargo workspace at `backend/` with three crates:
  - `atc-core` — Domain model (WorkflowRun, Job, Step types; StateStore with event-driven mutations; TTL eviction)
  - `atc-github` — GitHub API integration (webhook parsing via `parse_webhook`, HMAC-SHA256 signature verification via `verify_signature`, event translation to atc-core domain types)
  - `atc-server` — Axum HTTP server (webhook ingestion, WebSocket event stream, REST state snapshot, config with GitHub secrets, asset serving, metrics, dev proxy)
- **Frontend:** Svelte 5 + Vite + Tailwind v4 — standalone SPA at `frontend/` with OKLCH design system
- **Package manager:** pnpm (via Corepack)
- **Task runner:** just (see `justfile` for all commands)
- **Git hooks:** lefthook (pre-commit linting, commit-msg validation, pre-push tests)
- **Tool provisioning:** mise (`.mise.toml` pins all tool versions)

## Commands

```bash
just setup    # Bootstrap environment (mise + corepack + pnpm + lefthook)
just lint     # Run all linters
just fmt      # Format all code
just check    # Type-check / compile-check
just test     # Run all tests
just types    # Generate TypeScript types from Rust via ts-rs
just dev      # Start dev servers
just build    # Production build
```

## Project Structure

- `.mise.toml` — Pinned tool versions (Rust, Node, just, lefthook)
- `.commitlintrc.mjs` — Conventional Commits config (free-form scopes)
- `.github/workflows/` — CI and release workflows (ci.yml, zizmor.yml, release-please.yml, release.yml)
- `lefthook.yml` — Three-tier git hook definitions
- `justfile` — Task runner recipes
- `backend/` — Rust workspace with three crates:
  - `backend/crates/atc-core/` — Domain model and state store (types: RunId, JobId, StepId; events: RunEvent, JobEvent; StateStore with RwLock and TTL eviction; Clock trait)
  - `backend/crates/atc-github/` — GitHub API integration (webhook parsing and translation to atc-core domain events, HMAC-SHA256 signature verification)
  - `backend/crates/atc-server/` — Axum HTTP server (webhook ingestion, WebSocket event stream, REST state snapshot, config with GitHub secrets, asset serving, metrics, dev proxy)
- `frontend/` — Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system
- `deploy/helm/` — Helm chart at `deploy/helm/atc/`
- `.impeccable.md` — Design system config (brand, color tokens, type scale, accessibility)
- `Dockerfile` — Multi-stage container build (cargo-chef caching, distroless runtime)
- `.dockerignore` — Docker build context filter
- `release-please-config.json` — release-please manifest config (version sync, changelog)
- `.release-please-manifest.json` — Version tracker for release-please
- `scripts/doc-mapping.sh` — Source-to-architecture-doc mappings
- `scripts/check-docs-lefthook.sh` — Pre-push doc-staleness gate
- `docs/architecture/` — Architecture docs (created as features ship)
- `docs/architecture-decisions/` — ADRs
- `docs/design-plans/` — Feature design plans
- `docs/ideation/` — Pre-code design artifacts, research, and prototype

## Documentation Map

| What | Where |
|------|-------|
| Architecture docs | `docs/architecture/` (created per feature) |
| Deployment | `docs/architecture/deployment.md` |
| Architecture decisions (ADRs) | `docs/architecture-decisions/` |
| Design plans | `docs/design-plans/` |
| Pre-code design & research | `docs/ideation/` (architecture research, UI design, prototype, competitive analysis) |
| Design system config | `.impeccable.md` |
| Human workflows & conventions | `CONTRIBUTING.md` |
| Doc enforcement mappings | `scripts/doc-mapping.sh` |

## Documentation Framework

This project uses a five-layer documentation model with a strict non-duplication rule. See `CONTRIBUTING.md` section "Documentation Conventions" for the full specification.

**Key rule for AI agents:** When you modify source files, check `scripts/doc-mapping.sh` to see if an architecture doc needs updating. Update the architecture doc alongside your code changes.

## Invariants

- **Conventional Commits enforced:** Every commit must pass commitlint via lefthook commit-msg hook. Free-form scopes allowed.
- **Three-tier hooks:** Pre-commit (linters, parallel, glob-filtered) -> commit-msg (commitlint) -> pre-push (tests + doc-staleness). Do not bypass.
- **Worktree hook installation:** Git worktrees do not inherit hooks from the parent repo. Run `just setup` or `lefthook install` in each new worktree, or hooks will not fire and lint/format issues will only surface in CI.
- **Doc-staleness gate:** `scripts/check-docs-lefthook.sh` blocks push if source files changed without updating their mapped architecture doc. Mappings live in `scripts/doc-mapping.sh`.
- **CI gates PRs:** All PRs must pass CI checks (lint, type-check, test, build) before merge. Path-filtered on PRs; full matrix on pushes to main. See `docs/architecture/ci-pipeline.md`.
- **Non-duplication rule:** Each piece of documentation has exactly one canonical home. CLAUDE.md points to docs; it does not duplicate them.
- **Slim CLAUDE.md in every domain directory:** Every subdirectory that represents a distinct domain (crates, frontend, helm chart, .github, etc.) must have a slim `CLAUDE.md` that states its purpose, points to canonical architecture docs, and provides domain-specific guidance. Do not duplicate architecture doc content — reference it. Follow the pattern established in `backend/crates/atc-core/CLAUDE.md`.
- **AGENTS.md symlinks:** Every `CLAUDE.md` must have a corresponding `AGENTS.md` symlink (`ln -s CLAUDE.md AGENTS.md`) in the same directory. This ensures tools that look for either filename find the same content. Create both files together — never one without the other.

## Commit Format

Conventional Commits required. See `CONTRIBUTING.md` section "Commit Conventions".

## Pull Requests

This repo uses **squash merges** — the PR description becomes the squashed commit body. Do **not** put test plans in the PR description. Post the test plan as the **first comment** on the PR instead.
