# CLAUDE.md — AI Agent Index

Last verified: 2026-05-13

> **Keep this file lean.** Detailed documentation lives in `docs/`. This file provides pointers, not content. When you update a feature, update its architecture doc in `docs/architecture/` — not this file.

## Project

**ATC — Actions Traffic Control**
Real-time GitHub Actions dashboard. Rust backend (Axum) + Svelte 5 + Vite frontend. Two storage modes: external Postgres (production, multi-replica) via a transactional outbox + LISTEN/NOTIFY drain, or in-memory (dev-only, single-replica). See `docs/architecture/backend-server.md` for the architecture and `docs/architecture/deployment.md` for the operator surface.

## Tech Stack

- **Backend:** Rust 1.94.0 with Axum — Cargo workspace at `backend/` with three crates:
  - `atc-core` — Pure domain model (WorkflowRun, Job, Step types; pure transition functions `apply_run_event` / `apply_job_event`; eviction predicate `is_evictable`). No tokio, no interior mutability.
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
  - `backend/crates/atc-core/` — Pure domain model and transition functions (types: RunId, JobId, StepId; events: RunEvent, JobEvent; pure `apply_*_event` and `is_evictable` free functions; Clock trait). All stateful persistence concerns (HashMap state, locks, TTL eviction task) live in `atc-server::persist`.
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
| Observability (metrics + spans + OTel pipeline) | `docs/architecture/metrics.md` |
| Architecture decisions (ADRs) | `docs/architecture-decisions/` |
| Design plans | `docs/design-plans/` |
| Pre-code design & research | `docs/ideation/` (architecture research, UI design, prototype, competitive analysis) |
| Design system config | `.impeccable.md` |
| Planning & design workflow | `docs/planning-workflow.md` |
| Implementation guidance | `docs/implementation-guidance.md` |
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
- **Implementation reads the plan:** When starting implementation from a design plan in `docs/design-plans/`, read `docs/implementation-guidance.md` before writing any code.
- **Cargo and workspace-rooted tools need absolute paths:** `cargo`, `cargo nextest`, `git grep`, `rg` invocations from the implementing agent must be prefixed with `cd /Users/brajkovic/Projects/atc/backend && ...` (cargo) or `cd /Users/brajkovic/Projects/atc && ...` (workspace-rooted greps). The Bash tool resets cwd between calls — relative `cd backend &&` is bug-prone because the prior cwd may have moved.

## Commit Format

Conventional Commits required. See `CONTRIBUTING.md` section "Commit Conventions".

## Pull Requests

This repo uses **squash merges** — the PR description becomes the squashed commit body. PR body voice, test-plan placement, and title conventions are in `CONTRIBUTING.md` § "Pull Requests".
