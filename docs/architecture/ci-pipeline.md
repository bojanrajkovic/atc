# CI Pipeline — Architecture

Last verified: 2026-05-06 (updated 2026-05-06 for Phase 4 helm matrix swap: persistence → multi-replica)

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

**Decision:** Helm validation is split across two jobs — `helm-lint` (single instance, runs `helm lint` + `helm unittest`) and `helm` (2 × 5 matrix of Kubernetes versions × test values files for `helm template | kubeconform`)
**Alternatives considered:** Single k8s version; inline bash loop instead of matrix; combine lint and matrix into one job; matrix-multiply lint over k8s versions
**Rationale:** A two-endpoint matrix (oldest supported, latest stable) catches API deprecations and removals without the combinatorial overhead of testing every minor version. Five values files correspond to the five distinct feature surfaces (defaults, ingress, gateway, multi-replica, metrics) — exhaustive coverage without duplication. `helm lint` and `helm unittest` are k8s-version-independent (they don't run against an API server), so they live in a single non-matrixed `helm-lint` job rather than running ten times across the matrix. The two jobs land under one `helm-result` gate so branch protection treats them as a single required check.

**Decision:** kubeconform uses datreeio/CRDs-catalog as a supplemental schema location
**Alternatives considered:** Skip CRD validation; vendor CRD schemas into the repo; use kubeconform's built-in `--ignore-missing-schemas`
**Rationale:** The chart includes a `ServiceMonitor` (monitoring.coreos.com/v1) and an `HTTPRoute` (gateway.networking.k8s.io/v1) — both are CRDs absent from the upstream Kubernetes JSON schema repository. The datreeio/CRDs-catalog provides community-maintained schemas for these CRDs, enabling `-strict` mode without false negatives on custom resources. Vendoring schemas into the repo would require manual maintenance on each CRD version bump; the catalog URL is resolved at CI time and kept current by the catalog maintainers.

**Decision:** `helm-result` gate job translates skipped-to-passed for branch protection
**Alternatives considered:** No gate job (use the matrix job directly as required check); GitHub's "required checks can be skipped" setting
**Rationale:** GitHub branch protection does not distinguish between "job skipped" and "job failed" — both cause a required status check to block the PR. The `helm-result` gate pattern (already used for `backend-result` and `frontend-result`) reads the `changes` output and emits success when the job was intentionally skipped due to no path-filter match. This allows a Rust-only PR to pass all required checks without triggering helm validation.

**Decision:** Zizmor findings are security advisories, not required status checks
**Alternatives considered:** Required status check, blocking gate, optional warning
**Rationale:** Zizmor findings are security improvement opportunities, not build blockers. Displaying them in the Security tab allows teams to triage and fix them as part of the security review process without blocking PRs on first occurrence.

**Decision:** All workflow jobs run on a pinned `ubuntu-24.04` runner (no `ubuntu-latest`)
**Alternatives considered:** Track `ubuntu-latest` so we get GitHub-managed image rolls automatically; pin to a SHA-tagged image
**Rationale:** `ubuntu-latest` rolls over to whichever Ubuntu image GitHub currently considers current — historically that has produced same-day breakage when the alias flips (e.g. `actions/checkout` deprecation behaviour, glibc bumps that bust cached binaries, distro tool-version changes). Pinning to `ubuntu-24.04` gives reproducible CI: the runner OS only changes when this repo updates the pin in a commit reviewable on its own. The release pipeline already used `ubuntu-24.04`; this aligns the rest of the matrix.

**Decision:** All cargo build/check/test invocations use `--locked`
**Alternatives considered:** No lockfile enforcement; only enforce in release builds
**Rationale:** `--locked` makes CI fail loudly when `Cargo.toml` is edited without committing the regenerated `Cargo.lock`, preventing dependency drift between developer machines, CI, and the released artifacts. Pairs with the release pipeline's bot-driven `Cargo.lock` refresh on release PRs (see release-pipeline.md), so the release PR's own CI passes under the same `--locked` rule. Applied to clippy, check, llvm-cov, and the release build; `cargo fmt` is exempt because it is not a build command.

**Decision:** Backend job sets `RUSTFLAGS="-C link-arg=-fuse-ld=mold"` and installs `mold` via apt
**Alternatives considered:** Keep `lld` (default); limit lld thread count via `-Wl,--threads=1`; use `sccache`
**Rationale:** `lld`'s parallel link stage materialises the full link graph across multiple threads simultaneously and exceeds available memory on GitHub-hosted runners when linking large test binaries (`ws_tests`, `notify_listener_tests`). This produced reproducible `SIGBUS` (signal 7) linker crashes. `mold` uses a fundamentally different incremental linking approach with much lower peak memory, and is available in apt on `ubuntu-24.04` without a third-party installer. Setting `RUSTFLAGS` at the backend job level means every cargo invocation that links (`llvm-cov`, `cargo build --release`) uses `mold`; steps that do not link (`cargo fmt`, `cargo clippy`, `cargo check`) are unaffected. Local macOS development is unaffected since the flag is CI-only.

**Decision:** Backend tests run via `cargo llvm-cov nextest` (cargo-nextest as the test runner), not `cargo llvm-cov` (built-in `cargo test`)
**Alternatives considered:** Keep built-in `cargo test`; introduce nextest only locally without changing CI
**Rationale:** `cargo test` runs each integration test binary serially within a `cargo test --workspace` invocation; with ~25 PG-backed integration test binaries each spinning up testcontainers, this serialization dominates wall-clock time. `cargo-nextest` runs each test in its own subprocess and parallelizes across binaries. Local benchmark: full backend test wall clock dropped from ~3:00 to ~32s (~5.6× speedup). The `ci` profile (`backend/.config/nextest.toml`) caps `test-threads = 4` to match GitHub-hosted runner vCPUs (avoids over-subscribing the Docker daemon under testcontainers contention) and enables one retry per test (`retries = { count = 1 }`) to absorb transient testcontainers boot flakes. `cargo-nextest` is installed via the existing `taiki-e/install-action` step alongside `cargo-llvm-cov` (one extra binary download, no separate step). Doctests (currently 0) are not run by `cargo llvm-cov nextest` by design — re-introduce `--doc` if doctests are added.

**Decision:** In the Backend job, `Verify generated TypeScript types are current` runs BEFORE `Run tests with coverage`, AND a `jlumbroso/free-disk-space` step runs at job start to reclaim pre-installed-software disk
**Alternatives considered:** Keep coverage-then-verify order (original); `cargo clean` between steps; split verify-types into its own job; use larger paid runners
**Rationale:** The verify-types step runs `cargo test --workspace -- export_bindings`, a non-instrumented build of the workspace. The coverage step runs `cargo llvm-cov --workspace`, a coverage-instrumented build. The two builds use different `RUSTFLAGS`, so cargo cannot reuse artifacts between them — each produces its own ~3-9 GB of `target/` content. Reordering (verify-types first) keeps the peak smaller because instrumented artifacts layer on top of a smaller baseline target/, but the coverage build alone still exceeded the GitHub-hosted runner's ~14 GB free disk (linker failed with "No space left on device"). The `free-disk-space` step removes Android SDK, .NET, Haskell/GHC, and Docker images that we don't use; this reclaims ~10-15 GB and gives the coverage build comfortable headroom. `tool-cache: false` is preserved so the Rust toolchain caching path stays intact; `large-packages: false` because the package-removal pass is slow and isn't necessary with the other reclamations. Tracking #55 for a deeper investigation (separate jobs, sccache, alternative runners).

## Boundaries

**Owns:** Workflow file definitions (.github/workflows/), test execution configuration, linting rules, security scanning configuration
**Does not own:** Application code (test by backend/frontend tests), deployment pipelines, artifact generation, secret management
**Prohibitions:** Never store secrets in workflow files — use GitHub Secrets. Never commit workflow files without matching updates to ci-pipeline.md. Never bypass the pre-push gate (no --no-verify).

## Files

- `.github/workflows/ci.yml` — Quality pipeline (linting, type-checking, testing, building); includes `helm` job with 2×5 kubeconform matrix and `helm-result` gate. Frontend artifact upload includes `coverage/lcov.info` and `test-results/frame-budget-trace.json` (the Tier 2 informational frame-budget trace produced by `e2e/frame-budget.test.ts`).
- `.github/workflows/zizmor.yml` — Workflow security linter
- `scripts/doc-mapping.sh` — Maps workflow file changes to this architecture doc
