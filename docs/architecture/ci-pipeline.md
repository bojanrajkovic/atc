# CI Pipeline — Architecture

Last verified: 2026-04-08

## Purpose

The CI pipeline ensures code quality and security across the ATC project through two specialized workflows:

1. **Quality pipeline (ci.yml)** — Lints, type-checks, tests, and builds the Rust backend, Svelte frontend, and Helm chart. Runs on every pull request and push to main, with path-based filtering to only run affected stacks on PRs.
2. **Workflow security linter (zizmor.yml)** — Scans GitHub Actions workflow files for security violations, hardcoded secrets, unsafe ref pinning, and permission creep. Runs only when workflow files change.

Both workflows are gated by lefthook pre-push hooks at development time, preventing broken or insecure code from reaching the remote.

## Key Decisions

**Decision:** Separate workflow files for quality vs. security linting
**Alternatives considered:** Monolithic single workflow with all jobs
**Rationale:** Separation of concerns allows the security linter to trigger only on workflow changes (path filtering) rather than on every commit. This keeps CI feedback focused and reduces noise. Each workflow can be maintained and updated independently.

**Decision:** Path-based filtering on pull requests
**Alternatives considered:** Always run all checks, run on label, run based on file extensions
**Rationale:** PRs often touch only one stack (frontend or backend). Running only the affected stack's tests reduces CI time and feedback latency. Pushes to main always run both stacks to catch integration issues.

**Decision:** Helm job uses a 2 × 5 matrix (Kubernetes versions × test values files) for `helm template | kubeconform`
**Alternatives considered:** Single k8s version; inline bash loop instead of matrix; separate `helm lint` job
**Rationale:** A two-endpoint matrix (oldest supported, latest stable) catches API deprecations and removals without the combinatorial overhead of testing every minor version. Five values files correspond to the five distinct feature surfaces defined in Phase 4 (defaults, ingress, gateway, persistence, metrics) — exhaustive coverage without duplication. `helm lint` runs inside the matrix job rather than a separate pre-requisite job because it is fast (<1s) and the workflow complexity of a separate job outweighs the marginal redundancy of 10 lint runs.

**Decision:** kubeconform uses datreeio/CRDs-catalog as a supplemental schema location
**Alternatives considered:** Skip CRD validation; vendor CRD schemas into the repo; use kubeconform's built-in `--ignore-missing-schemas`
**Rationale:** The chart includes a `ServiceMonitor` (monitoring.coreos.com/v1) and an `HTTPRoute` (gateway.networking.k8s.io/v1) — both are CRDs absent from the upstream Kubernetes JSON schema repository. The datreeio/CRDs-catalog provides community-maintained schemas for these CRDs, enabling `-strict` mode without false negatives on custom resources. Vendoring schemas into the repo would require manual maintenance on each CRD version bump; the catalog URL is resolved at CI time and kept current by the catalog maintainers.

**Decision:** `helm-result` gate job translates skipped-to-passed for branch protection
**Alternatives considered:** No gate job (use the matrix job directly as required check); GitHub's "required checks can be skipped" setting
**Rationale:** GitHub branch protection does not distinguish between "job skipped" and "job failed" — both cause a required status check to block the PR. The `helm-result` gate pattern (already used for `backend-result` and `frontend-result`) reads the `changes` output and emits success when the job was intentionally skipped due to no path-filter match. This allows a Rust-only PR to pass all required checks without triggering helm validation.

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

- `.github/workflows/ci.yml` — Quality pipeline (linting, type-checking, testing, building); includes `helm` job with 2×5 kubeconform matrix and `helm-result` gate
- `.github/workflows/zizmor.yml` — Workflow security linter
- `scripts/doc-mapping.sh` — Maps workflow file changes to this architecture doc
