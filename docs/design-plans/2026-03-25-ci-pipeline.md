# CI Pipeline Design

## Summary

ATC's CI pipeline adds automated quality enforcement that runs on every pull request and main branch push. The pipeline is split across two GitHub Actions workflow files: `ci.yml` runs the main quality checks (formatting, linting, compilation, tests, release build, and PR convention validation), while `zizmor.yml` separately audits the workflow files themselves for security issues. Both workflows are new — ATC currently compiles and lints locally but has no CI enforcement.

The central design challenge is that ATC is a two-stack project (Rust backend, Svelte/TypeScript frontend), and the two stacks share almost no tooling. Rather than running everything unconditionally, `ci.yml` uses path-based change detection to skip the unaffected stack on PRs that touch only one side. Required branch-protection status checks are satisfied through dedicated "result gate" jobs that translate skipped runs into passing statuses, so branch protection works correctly even when a stack is bypassed. Coverage from both stacks is uploaded to Codecov under separate flags and merged into a single unified report. All workflows follow a consistent security posture inherited from the project's other repositories: SHA-pinned action references, no persisted checkout credentials, and a default-deny permissions model with per-job grants.

## Definition of Done

Two GitHub Actions workflow files that provide automated quality gates on PRs and main pushes:

1. **ci.yml** — Sequential pipeline: format check → lint → compile check → test (with coverage upload to Codecov) → release build → dependency review. Uses mise.toml-driven tool versions (read versions, pass to setup-node/setup-rust), Swatinem/rust-cache + manual pnpm cache, concurrency groups with cancel-in-progress, SHA-pinned actions, minimal permissions (`permissions: {}` at workflow level), test artifact upload on all runs (`if: always()`). Includes PR title validation via amannn/action-semantic-pull-request for squash-merge safety.

2. **zizmor.yml** — Path-filtered workflow (triggers only on `.github/workflows/` changes) that lints workflow YAML files with the official zizmor-action, uploading SARIF results to GitHub's Security tab.

Both workflows follow the security posture established across the project's other repositories: SHA-pinned `uses:` references, `persist-credentials: false` on all checkouts, `permissions: {}` at workflow level with per-job grants.

## Acceptance Criteria

### ci-pipeline.AC1: Backend quality checks catch violations
- **ci-pipeline.AC1.1 Success:** PR with clean Rust code passes backend job
- **ci-pipeline.AC1.2 Failure:** PR with `cargo fmt` violation fails backend job
- **ci-pipeline.AC1.3 Failure:** PR with clippy warning (pedantic) fails backend job
- **ci-pipeline.AC1.4 Failure:** PR with compilation error fails backend job
- **ci-pipeline.AC1.5 Failure:** PR with failing test fails backend job

### ci-pipeline.AC2: Frontend quality checks catch violations
- **ci-pipeline.AC2.1 Success:** PR with clean frontend code passes frontend job
- **ci-pipeline.AC2.2 Failure:** PR with Biome/Prettier formatting violation fails frontend job
- **ci-pipeline.AC2.3 Failure:** PR with ESLint error in .svelte file fails frontend job
- **ci-pipeline.AC2.4 Failure:** PR with svelte-check type error fails frontend job
- **ci-pipeline.AC2.5 Failure:** PR with failing vitest test fails frontend job

### ci-pipeline.AC3: Path filtering skips unaffected stacks
- **ci-pipeline.AC3.1 Success:** Frontend-only PR skips backend job, backend-result gate passes
- **ci-pipeline.AC3.2 Success:** Backend-only PR skips frontend job, frontend-result gate passes
- **ci-pipeline.AC3.3 Success:** PR touching shared config (.mise.toml, justfile, ci.yml) triggers both stacks
- **ci-pipeline.AC3.4 Success:** Push to main always runs both stacks regardless of paths

### ci-pipeline.AC4: Coverage and test artifacts are available
- **ci-pipeline.AC4.1 Success:** Backend coverage report appears in Codecov with `backend` flag
- **ci-pipeline.AC4.2 Success:** Frontend coverage report appears in Codecov with `frontend` flag
- **ci-pipeline.AC4.3 Success:** Test artifacts are uploaded even when tests fail
- **ci-pipeline.AC4.4 Success:** Test artifacts have distinct names per job (no collision)

### ci-pipeline.AC5: PR-specific checks enforce conventions
- **ci-pipeline.AC5.1 Failure:** PR adding a dependency with known moderate+ vulnerability fails dependency-review
- **ci-pipeline.AC5.2 Failure:** PR with non-conventional title fails PR title check
- **ci-pipeline.AC5.3 Success:** PR with conventional title and clean dependencies passes pr-checks

### ci-pipeline.AC6: Release build validates compilation
- **ci-pipeline.AC6.1 Success:** `cargo build --release -p atc-server` succeeds in CI
- **ci-pipeline.AC6.2 Failure:** Release-only compilation error (e.g., feature gate issue) is caught

### ci-pipeline.AC7: Zizmor lints workflow files
- **ci-pipeline.AC7.1 Success:** Modifying a workflow file triggers the zizmor workflow
- **ci-pipeline.AC7.2 Success:** Findings appear in the GitHub Security tab as code scanning alerts
- **ci-pipeline.AC7.3 Edge:** Non-workflow file changes do not trigger zizmor

### ci-pipeline.AC8: Security posture
- **ci-pipeline.AC8.1 Success:** All `uses:` references are SHA-pinned with version comments
- **ci-pipeline.AC8.2 Success:** All checkouts use `persist-credentials: false`
- **ci-pipeline.AC8.3 Success:** Workflow-level permissions are `{}`; grants are per-job only
- **ci-pipeline.AC8.4 Success:** Concurrent runs on same branch cancel in-progress

## Glossary

- **mise / `.mise.toml`**: A developer tool version manager. The `.mise.toml` file at the repo root pins exact versions of Rust, Node, pnpm, and other tools. CI reads this file so that workflow tool versions stay in sync with local development without being duplicated.
- **Swatinem/rust-cache**: A GitHub Action that caches the Cargo registry and compiled build artifacts between CI runs, significantly reducing Rust build times.
- **dorny/paths-filter**: A GitHub Action that inspects which files changed in a push or PR and outputs boolean flags. Used here to decide whether the backend or frontend stack needs to run.
- **Result gate job**: A job that runs unconditionally (`if: always()`) after a conditional job and its dependency. It passes if the conditional job was skipped (no relevant changes) or succeeded, and fails if the conditional job ran and failed. This pattern satisfies required status checks on branch protection rules even when a job is intentionally skipped.
- **SHA-pinned action**: A `uses:` reference in a GitHub Actions workflow that points to a specific commit hash rather than a mutable tag (e.g. `@v4`). Prevents a compromised or changed action tag from silently altering CI behavior.
- **`persist-credentials: false`**: An option on `actions/checkout` that prevents GitHub from writing a long-lived token to the local Git config. Limits the blast radius if a later step in the job is compromised.
- **`permissions: {}`**: Setting workflow-level permissions to an empty object denies all default GitHub token permissions. Individual jobs then grant only what they need (e.g., `contents: read`).
- **Concurrency group with `cancel-in-progress`**: A GitHub Actions feature that cancels a workflow run when a newer run for the same branch is triggered, avoiding redundant work from rapid pushes.
- **zizmor**: A static analysis tool for GitHub Actions workflow YAML that identifies security issues (e.g., script injection risks, overly broad permissions). Produces SARIF output.
- **SARIF**: Static Analysis Results Interchange Format. A JSON schema for reporting code scanning findings. GitHub's Security tab natively renders SARIF uploads as code scanning alerts.
- **Codecov flags**: A Codecov feature for projects with multiple coverage sources. Each upload carries a named flag (`backend`, `frontend`); Codecov merges them into a single combined report.
- **cargo-llvm-cov**: A Cargo subcommand that instruments Rust code using LLVM's coverage tooling and produces LCOV-format reports, which Codecov accepts.
- **LCOV**: A text-based coverage report format (`.info` files). Widely supported by coverage aggregation services including Codecov.
- **`cargo clippy` (pedantic)**: Rust's built-in linter. The flags `--all-targets --all-features -- -D warnings` enable all feature flags and all build targets, and promote every warning to a compilation error.
- **`svelte-check`**: A CLI tool that type-checks `.svelte` files using the TypeScript compiler. The standard Svelte equivalent of running `tsc --noEmit`.
- **Biome**: A fast Rust-based formatter and linter for JavaScript/TypeScript. Used here alongside Prettier (which handles `.svelte` files that Biome does not fully support).
- **vitest**: A Vite-native unit test runner for JavaScript/TypeScript, used here for the frontend test suite.
- **`actions/dependency-review-action`**: A GitHub Action that compares the dependency manifest before and after a PR and fails if any newly introduced dependency has a known vulnerability at or above the configured severity threshold.
- **`amannn/action-semantic-pull-request`**: A GitHub Action that validates that a PR title conforms to Conventional Commits format. Required here because this repository uses squash merges, making the PR title the final commit message.
- **`pnpm/action-setup` + `actions/setup-node` with `cache: 'pnpm'`**: The recommended GitHub Actions pattern for Node/pnpm projects. Using both actions together enables native pnpm lockfile-based cache restoration without reinstalling Corepack.
- **`--frozen-lockfile`**: A pnpm install flag that fails if the lockfile is out of date rather than silently updating it. Ensures CI installs exactly what was committed.

## Architecture

Two workflow files: `ci.yml` (main quality pipeline) and `zizmor.yml` (workflow security linter).

### ci.yml — Quality Pipeline

Single workflow file with six jobs:

1. **`changes`** — Runs dorny/paths-filter to detect which stacks changed. Outputs `backend` and `frontend` booleans. Triggers both stacks on shared config changes (`.github/workflows/ci.yml`, `justfile`, `.mise.toml`).

2. **`backend`** — Rust-only setup. Conditional on `changes.outputs.backend == 'true'` or `push` event. Sequential steps: format check (`cargo fmt --check`), lint (`cargo clippy --all-targets --all-features -- -D warnings`), compile check (`cargo check --workspace`), test with coverage (`cargo llvm-cov --workspace --lcov`), coverage upload to Codecov (flag: `backend`), test artifact upload (`if: always()`), release build (`cargo build --release -p atc-server`).

3. **`frontend`** — Node/pnpm-only setup. Conditional on `changes.outputs.frontend == 'true'` or `push` event. Sequential steps: format check (`biome check .`, `prettier --check '**/*.svelte'`), lint (`eslint '**/*.svelte'`), type check (`svelte-check`), test (`vitest run`), coverage upload to Codecov (flag: `frontend`), test artifact upload (`if: always()`).

4. **`pr-checks`** — Lightweight job, runs on PRs only. No build tools needed. Steps: `actions/dependency-review-action` (fail-on-severity: moderate), `amannn/action-semantic-pull-request` (validates PR title matches conventional commit format for squash-merge safety).

5. **`backend-result`** — Result gate job. Runs `if: always()`, needs `[changes, backend]`. Passes if no backend changes detected or backend job succeeded. Fails if backend changes detected and backend job failed. This is the required status check for branch protection.

6. **`frontend-result`** — Same pattern as `backend-result` for the frontend stack.

### zizmor.yml — Workflow Security Linter

Single-job workflow with native `paths:` trigger on `.github/workflows/**`. Runs the official `zizmorcore/zizmor-action` with `persona: regular` and `online-audits: true`. Uploads SARIF results to GitHub's Security tab. Not a required check — findings appear as security advisories for triage.

### Tool Setup Strategy

Tool versions are read from `.mise.toml` (single source of truth) and passed to official setup actions for native caching:

**Backend job:**
- Parse `.mise.toml` → extract Rust version
- `dtolnay/rust-toolchain` with extracted version, components: `clippy, llvm-tools-preview`
- `Swatinem/rust-cache` for automatic Cargo registry/target caching

**Frontend job:**
- Parse `.mise.toml` → extract Node and pnpm versions
- `pnpm/action-setup` with extracted pnpm version
- `actions/setup-node` with extracted Node version and `cache: 'pnpm'`
- `pnpm install --frozen-lockfile`

### Security Posture

- `permissions: {}` at workflow level, per-job grants only
- `persist-credentials: false` on all checkouts
- SHA-pinned `uses:` references with version comment (e.g., `# v6.0.2`)
- Concurrency groups: `${{ github.workflow }}-${{ github.ref }}` with `cancel-in-progress: true`

### dorny/paths-filter Configuration

```yaml
backend:
  - 'backend/**'
  - '.github/workflows/ci.yml'
  - 'justfile'
  - '.mise.toml'
frontend:
  - 'frontend/**'
  - '.github/workflows/ci.yml'
  - 'justfile'
  - '.mise.toml'
```

Main branch pushes always run both stacks regardless of path detection, using: `if: needs.changes.outputs.backend == 'true' || github.event_name == 'push'`.

## Existing Patterns

Investigation of existing repositories revealed consistent CI patterns:

- **mise.toml as version authority** — all projects read tool versions from `.mise.toml` rather than hardcoding in workflow YAML. ATC follows this pattern.
- **SHA pinning with version comments** — universal across all repositories. Every `uses:` reference is SHA-pinned with a trailing `# vN.N.N` comment.
- **`persist-credentials: false`** — standard practice on all checkouts.
- **`permissions: {}`** — workflow-level deny-all with per-job grants.
- **Concurrency groups** — `${{ github.workflow }}-${{ github.ref }}` with cancel-in-progress for PRs.
- **dorny/paths-filter + result gates** — established in unquote for per-component conditional execution with required-checks compatibility. ATC adapts this pattern into a single workflow file.
- **pnpm/action-setup + setup-node cache** — cleaner than the corepack reinstall pattern. Used instead of mise-action for better native caching.
- **dependency-review-action** — used in containerfile-ts with `fail-on-severity: moderate`.
- **amannn/action-semantic-pull-request** — used in containerfile-ts, grounds, mcp-paprika for PR title validation.

New patterns introduced by this design:

- **zizmor** — workflow security linting is new across all repositories. ATC will be the first to adopt it.
- **cargo-llvm-cov** — Rust coverage tooling. Other projects use Go coverage or vitest coverage, not Rust-specific tools.
- **Codecov with dual flags** — uploading backend and frontend coverage separately with flag-based merging.

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: CI Workflow — Structure and Setup
**Goal:** Create `ci.yml` with triggers, permissions, concurrency, change detection, and tool setup steps for both jobs. No quality checks yet — just the skeleton that installs tools and caches dependencies.

**Components:**
- `.github/workflows/ci.yml` — workflow file with triggers, permissions, concurrency group
- `changes` job with dorny/paths-filter configuration
- `backend` job — checkout, parse mise.toml, rust-toolchain, rust-cache
- `frontend` job — checkout, parse mise.toml, pnpm/action-setup, setup-node, pnpm install
- `backend-result` and `frontend-result` gate jobs
- `pr-checks` job skeleton (checkout only)

**Dependencies:** None (first phase)

**Done when:** Pushing a branch triggers the workflow. Both jobs install tools and restore caches. Gate jobs report correct pass/skip status. Workflow YAML passes `zizmor` lint locally.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Quality Checks — Lint, Format, Type Check
**Goal:** Add format, lint, and type-check steps to both backend and frontend jobs.

**Components:**
- Backend steps: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo check --workspace`
- Frontend steps: `biome check .`, `prettier --check '**/*.svelte'`, `eslint '**/*.svelte'`, `svelte-check`

**Dependencies:** Phase 1 (tool setup)

**Done when:** A PR with a formatting violation or lint error fails the CI check. A clean PR passes.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Tests and Coverage
**Goal:** Add test execution with coverage collection and upload to Codecov.

**Components:**
- Backend: `cargo llvm-cov --workspace --lcov --output-path lcov.info`, Codecov upload with flag `backend`
- Frontend: `vitest run --coverage`, Codecov upload with flag `frontend`
- Test artifact upload (`if: always()`, retention 7 days, distinct names per job)
- `cargo-llvm-cov` added as a dev dependency or installed in CI

**Dependencies:** Phase 2 (quality checks)

**Done when:** Coverage reports appear in Codecov. Test artifacts are downloadable from the Actions run, including on failure. Both `backend` and `frontend` flags appear in Codecov's unified report.
<!-- END_PHASE_3 -->

<!-- START_PHASE_4 -->
### Phase 4: Release Build and PR Checks
**Goal:** Add release binary compilation and PR-specific checks (dependency review, PR title validation).

**Components:**
- Backend: `cargo build --release -p atc-server` (final step, after tests pass)
- `pr-checks` job: `actions/dependency-review-action` with `fail-on-severity: moderate`, `amannn/action-semantic-pull-request`

**Dependencies:** Phase 3 (tests)

**Done when:** Release build succeeds in CI. A PR adding a vulnerable dependency is flagged. A PR with a non-conventional title fails the check.
<!-- END_PHASE_4 -->

<!-- START_PHASE_5 -->
### Phase 5: Zizmor Workflow
**Goal:** Create `zizmor.yml` for workflow security linting with SARIF upload.

**Components:**
- `.github/workflows/zizmor.yml` — path-filtered workflow
- `zizmorcore/zizmor-action` with persona: regular, online-audits: true, SARIF upload
- Permissions: contents: read, security-events: write, actions: read

**Dependencies:** Phase 1 (ci.yml must exist to be linted)

**Done when:** Modifying a workflow file triggers the zizmor check. Findings appear in the repository's Security tab under Code Scanning.
<!-- END_PHASE_5 -->

## Additional Considerations

**Documents to Update:**
| Document | Reason |
|----------|--------|
| `CLAUDE.md` | Add CI workflow to project structure and documentation map |
| `CONTRIBUTING.md` | Add CI section explaining what runs on PRs, how to read results |
| `scripts/doc-mapping.sh` | Add mapping from `.github/workflows/` to CI architecture doc (if created) |

**Branch protection:** After CI is green, configure branch protection rules requiring `backend-result` and `frontend-result` as status checks. This is a manual GitHub settings step, not a code deliverable.

**Codecov setup:** Requires enabling the Codecov GitHub App on the repository and adding `CODECOV_TOKEN` as a repository secret. This is a one-time manual setup step.
