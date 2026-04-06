# CLAUDE.md — AI Agent Index

Last verified: 2026-03-26

> **Keep this file lean.** Detailed documentation lives in `docs/`. This file provides pointers, not content. When you update a feature, update its architecture doc in `docs/architecture/` — not this file.

## Project

**ATC — Actions Traffic Control**
Real-time GitHub Actions dashboard. Rust backend (Axum) + Svelte 5 + Vite frontend.

**Status:** Skeleton with CI. Both stacks compile, lint, and pass CI. No application features yet.

## Tech Stack

- **Backend:** Rust 1.94.0 with Axum — Cargo workspace at `backend/` with three crates (atc-core, atc-github, atc-server)
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
just dev      # Start dev servers
just build    # Production build
```

## Project Structure

- `.mise.toml` — Pinned tool versions (Rust, Node, just, lefthook)
- `.commitlintrc.mjs` — Conventional Commits config (free-form scopes)
- `.github/workflows/` — CI and release workflows (ci.yml, zizmor.yml, release-please.yml, release.yml)
- `lefthook.yml` — Three-tier git hook definitions
- `justfile` — Task runner recipes
- `backend/` — Rust workspace: atc-core (domain), atc-github (GitHub API), atc-server (Axum HTTP)
- `frontend/` — Svelte 5 + Vite SPA with Tailwind v4 OKLCH design system
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
- **Doc-staleness gate:** `scripts/check-docs-lefthook.sh` blocks push if source files changed without updating their mapped architecture doc. Mappings live in `scripts/doc-mapping.sh`.
- **CI gates PRs:** All PRs must pass CI checks (lint, type-check, test, build) before merge. Path-filtered on PRs; full matrix on pushes to main. See `docs/architecture/ci-pipeline.md`.
- **Non-duplication rule:** Each piece of documentation has exactly one canonical home. CLAUDE.md points to docs; it does not duplicate them.

## Commit Format

Conventional Commits required. See `CONTRIBUTING.md` section "Commit Conventions".

## Pull Requests

This repo uses **squash merges** — the PR description becomes the squashed commit body. Do **not** put test plans in the PR description. Post the test plan as the **first comment** on the PR instead.
