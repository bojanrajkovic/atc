# CLAUDE.md — AI Agent Index

Last verified: 2026-05-18

> **Keep this file lean.** Detailed documentation lives in `docs/`. This file provides pointers, not content. When you update a feature, update its architecture doc in `docs/architecture/` — not this file.

## Project

**ATC — Actions Traffic Control**
Real-time GitHub Actions dashboard. Rust backend (Axum) + Svelte 5 + Vite frontend. Two storage modes: external Postgres (production, multi-replica) via a transactional outbox + LISTEN/NOTIFY drain, or in-memory (dev-only, single-replica). See `docs/architecture/backend-server.md` for the architecture and `docs/architecture/deployment.md` for the operator surface.

## Tech Stack

- **Backend:** Rust 1.94.0 with Axum — Cargo workspace at `backend/` with seven crates. See each crate's `CLAUDE.md` for the contract; per-crate docs are kept current.
  - `atc-core` — Pure domain model (WorkflowRun, Job, Step, RunnerPoolCapacity types; pure transition functions `apply_run_event` / `apply_job_event`; eviction predicate `is_evictable`; `Clock` trait). No tokio, no interior mutability, no I/O.
  - `atc-github` — GitHub webhook parsing, HMAC-SHA256 verification, translation into `atc-core` events.
  - `atc-wire` — Wire types (`CommittedEvent`, `StateSnapshot`) that cross WebSocket + REST to the frontend; ts-rs–exported.
  - `atc-persist` — `PersistentStore` trait, `LivenessError`, `join_with_timeout`. Zero storage-library deps — the interface waist between `atc-server` and concrete stores.
  - `atc-store-mem` — In-memory `PersistentStore` implementation (HashMap + secondary indexes + seq mutex + TTL eviction). Dev/test mode only — single replica, lossy on restart.
  - `atc-store-pg` — Postgres-backed `PersistentStore` implementation: outbox writes, LISTEN/NOTIFY drain, retention sweep, snapshot reads, `PgMetrics`. Production path.
  - `atc-server` — Axum HTTP server (webhook ingestion, WebSocket stream, REST snapshot, config + hot-reload watcher, OTel init, shutdown orchestration, asset serving with dev-proxy fallback). The only executable crate.
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
- `backend/` — Rust workspace with seven crates:
  - `backend/crates/atc-core/` — Pure domain types, events, state-machine transitions, eviction predicate, `Clock` trait. No tokio, no locks, no I/O. Stateful persistence concerns live in the store crates (ADR-0008).
  - `backend/crates/atc-github/` — GitHub webhook parsing + HMAC-SHA256 verification + translation into `atc-core` events.
  - `backend/crates/atc-wire/` — Serializable wire types (`CommittedEvent`, `StateSnapshot`) that cross WS + REST to the frontend. ts-rs–exported.
  - `backend/crates/atc-persist/` — `PersistentStore` trait, `LivenessError`, `join_with_timeout`. Zero storage-library deps.
  - `backend/crates/atc-store-mem/` — In-memory `PersistentStore` implementation. Dev/test mode only.
  - `backend/crates/atc-store-pg/` — Postgres `PersistentStore` implementation: writes + outbox + drain + retention + snapshot reads + `PgMetrics` + embedded migrations.
  - `backend/crates/atc-server/` — Axum HTTP server: routes, WS handler, config + hot-reload watcher, OTel init, shutdown orchestration, asset serving with dev-proxy fallback. The only executable crate.
- `frontend/` — Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system
- `deploy/helm/` — Helm chart at `deploy/helm/atc/`
- `.impeccable.md` — Design system config (brand, color tokens, type scale, accessibility)
- `Dockerfile` — Multi-stage container build (cargo-chef caching, distroless runtime)
- `.dockerignore` — Docker build context filter
- `release-please-config.json` — release-please manifest config (version sync, changelog)
- `.release-please-manifest.json` — Version tracker for release-please
- `scripts/doc-mapping.yaml` — Source-to-architecture-doc mappings (data)
- `scripts/check-docs.sh` — Pre-push doc-staleness gate (driver)
- `docs/architecture/` — Architecture docs (created as features ship)
- `docs/architecture-decisions/` — ADRs
- `docs/design-plans/` — Feature design plans
- `docs/ideation/` — Pre-code design artifacts, research, and prototype

## Documentation Map

| What | Where |
|------|-------|
| Architecture docs (why + what) | `docs/architecture/` (created per feature) |
| Operator runbooks (how) | `docs/operator/` (auth-proxy recipes, integration patterns) |
| Deployment architecture | `docs/architecture/deployment.md` |
| Observability (metrics + spans + OTel pipeline) | `docs/architecture/metrics.md` |
| Architecture decisions (ADRs) | `docs/architecture-decisions/` |
| Design plans | `docs/design-plans/` |
| Pre-code design & research | `docs/ideation/` (architecture research, UI design, prototype, competitive analysis) |
| Design system config | `.impeccable.md` |
| Planning & design workflow | `docs/planning-workflow.md` |
| Implementation guidance | `docs/implementation-guidance.md` |
| Human workflows & conventions | `CONTRIBUTING.md` |
| Doc enforcement mappings | `scripts/doc-mapping.yaml` |

## Documentation Framework

This project uses a six-layer documentation model with a strict non-duplication rule. See [`docs/documentation-system.md`](docs/documentation-system.md) for the full specification.

**Key rule for AI agents:** When you modify source files, check `scripts/doc-mapping.yaml` to see if an architecture doc needs updating. Update the architecture doc alongside your code changes.

## Invariants

- **Conventional Commits enforced:** Every commit must pass commitlint via lefthook commit-msg hook. Free-form scopes allowed.
- **Three-tier hooks:** Pre-commit (linters, parallel, glob-filtered) -> commit-msg (commitlint) -> pre-push (tests + doc-staleness). Do not bypass.
- **Worktree hook installation:** Git worktrees do not inherit hooks from the parent repo. Run `just setup` or `lefthook install` in each new worktree, or hooks will not fire and lint/format issues will only surface in CI.
- **Doc-staleness gate:** `scripts/check-docs.sh` blocks push if source files changed without updating their mapped architecture doc. Mappings live in `scripts/doc-mapping.yaml`.
- **CI gates PRs:** All PRs must pass CI checks (lint, type-check, test, build) before merge. Path-filtered on PRs; full matrix on pushes to main. See `docs/architecture/ci-pipeline.md`.
- **Non-duplication rule:** Don't put content into CLAUDE.md that lives in another canonical doc — point and link instead. Full rule + the six-layer model in [`docs/documentation-system.md`](docs/documentation-system.md).
- **Slim directory-level CLAUDE.md (two-tier):** Every domain subdirectory gets a CLAUDE.md with a Tier 1 skeleton (Purpose + pointer to canonical doc); Tier 2 Sharp edges accrete reactively from observed agent failures, not pre-authored. Template, exemplars, and full rule in [`docs/documentation-system.md`](docs/documentation-system.md) § "Directory-Level CLAUDE.md Files".
- **AGENTS.md symlinks:** Every CLAUDE.md gets a sibling `AGENTS.md` symlink (`ln -s CLAUDE.md AGENTS.md`). The `scripts/check-agents-symlinks.sh` pre-push gate enforces this; create both files together when standing up a new domain dir.
- **Implementation reads the plan:** When starting implementation from a design plan in `docs/design-plans/`, read `docs/implementation-guidance.md` before writing any code.
- **Cargo and workspace-rooted tools need absolute paths:** `cargo`, `cargo nextest`, `git grep`, `rg` invocations from the implementing agent must be prefixed with `cd /Users/brajkovic/Projects/atc/backend && ...` (cargo) or `cd /Users/brajkovic/Projects/atc && ...` (workspace-rooted greps). The Bash tool resets cwd between calls — relative `cd backend &&` is bug-prone because the prior cwd may have moved.

## Commit Format

Conventional Commits required. See `CONTRIBUTING.md` section "Commit Conventions".

## Pull Requests

This repo uses **squash merges** — the PR description becomes the squashed commit body. PR body voice, test-plan placement, and title conventions are in `CONTRIBUTING.md` § "Pull Requests".
