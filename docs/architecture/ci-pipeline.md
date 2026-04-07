# CI Pipeline — Architecture

Last verified: 2026-04-07

## Purpose

The CI pipeline ensures code quality and security across the ATC project through two specialized workflows:

1. **Quality pipeline (ci.yml)** — Lints, type-checks, tests, and builds both the Rust backend and Svelte frontend. Runs on every pull request and push to main, with path-based filtering to only run affected stacks on PRs.
2. **Workflow security linter (zizmor.yml)** — Scans GitHub Actions workflow files for security violations, hardcoded secrets, unsafe ref pinning, and permission creep. Runs only when workflow files change.

Both workflows are gated by lefthook pre-push hooks at development time, preventing broken or insecure code from reaching the remote.

## Key Decisions

**Decision:** Separate workflow files for quality vs. security linting
**Alternatives considered:** Monolithic single workflow with all jobs
**Rationale:** Separation of concerns allows the security linter to trigger only on workflow changes (path filtering) rather than on every commit. This keeps CI feedback focused and reduces noise. Each workflow can be maintained and updated independently.

**Decision:** Path-based filtering on pull requests
**Alternatives considered:** Always run all checks, run on label, run based on file extensions
**Rationale:** PRs often touch only one stack (frontend or backend). Running only the affected stack's tests reduces CI time and feedback latency. Pushes to main always run both stacks to catch integration issues.

**Decision:** Zizmor findings are security advisories, not required status checks
**Alternatives considered:** Required status check, blocking gate, optional warning
**Rationale:** Zizmor findings are security improvement opportunities, not build blockers. Displaying them in the Security tab allows teams to triage and fix them as part of the security review process without blocking PRs on first occurrence.

**Decision:** All cargo build/check/test invocations use `--locked`
**Alternatives considered:** No lockfile enforcement; only enforce in release builds
**Rationale:** `--locked` makes CI fail loudly when `Cargo.toml` is edited without committing the regenerated `Cargo.lock`, preventing dependency drift between developer machines, CI, and the released artifacts. Pairs with the release pipeline's bot-driven `Cargo.lock` refresh on release PRs (see release-pipeline.md), so the release PR's own CI passes under the same `--locked` rule. Applied to clippy, check, llvm-cov, and the release build; `cargo fmt` is exempt because it is not a build command.

## Boundaries

**Owns:** Workflow file definitions (.github/workflows/), test execution configuration, linting rules, security scanning configuration
**Does not own:** Application code (test by backend/frontend tests), deployment pipelines, artifact generation, secret management
**Prohibitions:** Never store secrets in workflow files — use GitHub Secrets. Never commit workflow files without matching updates to ci-pipeline.md. Never bypass the pre-push gate (no --no-verify).

## Files

- `.github/workflows/ci.yml` — Quality pipeline (linting, type-checking, testing, building)
- `.github/workflows/zizmor.yml` — Workflow security linter
- `scripts/doc-mapping.sh` — Maps workflow file changes to this architecture doc
