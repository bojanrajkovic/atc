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

**Decision:** Linux jobs run on self-hosted ARC runners (`runs-on: gha-arc-atc`); cross-arch release-only jobs that the homelab cluster cannot serve stay on pinned GitHub-hosted runners
**Alternatives considered:** Stay on GitHub-hosted `ubuntu-24.04` everywhere; mix per-job ad-hoc; use a third-party Linux runner provider
**Rationale:** A homelab Kubernetes cluster runs the GHA Actions Runner Controller (ARC) with a scale set sized for amd64 Linux workloads, registering runners under the standard GitHub labels `self-hosted`, `linux`, `amd64`. Workflows target the runner pool via `runs-on: [self-hosted, linux, amd64]` rather than the scale set's name — labels are decoupled from the scale set's identity, so a future rename, multi-pool setup, or migration to a different cluster requires no workflow changes. Moving CI/release Linux jobs onto the homelab eliminates GitHub-hosted runner-minute consumption during the development phase (the project burned its full monthly free quota in early May 2026, motivating the move). The scale set's image is `ghcr.io/loupe-app/ci-runner` (Ubuntu 24.04 base) with `gcc/g++/make/perl/openssl`, `docker/dockerd/containerd`, sudo NOPASSWD, and the `runner` user pre-joined to the `docker` group — close to feature-parity with `ubuntu-24.04`, missing only `cmake`, `pkg-config`, and `mold`, all installed at workflow runtime via `sudo apt-get install`. Cross-arch jobs that need facilities the homelab cluster does not provide stay on pinned GitHub-hosted runners: `aarch64-apple-darwin` builds use `macos-26` (Tahoe; `macos-15`/`macos-14` are deprecating); arm64 Linux artifacts are produced via cross-compilation on the self-hosted amd64 pool (no QEMU; see release-pipeline.md). All GitHub-hosted runs-on labels are pinned to versioned tags (`macos-26`, `ubuntu-24.04-arm`) — never `*-latest` — so runner OS changes are reviewable in commits.

**Decision:** All cargo build/check/test invocations use `--locked`
**Alternatives considered:** No lockfile enforcement; only enforce in release builds
**Rationale:** `--locked` makes CI fail loudly when `Cargo.toml` is edited without committing the regenerated `Cargo.lock`, preventing dependency drift between developer machines, CI, and the released artifacts. Pairs with the release pipeline's bot-driven `Cargo.lock` refresh on release PRs (see release-pipeline.md), so the release PR's own CI passes under the same `--locked` rule. Applied to clippy, check, llvm-cov, and the release build; `cargo fmt` is exempt because it is not a build command.

**Decision:** Backend job sets `RUSTFLAGS="-C link-arg=-fuse-ld=mold"` and installs `mold` (alongside `cmake` and `pkg-config`) via apt in a single `Install build dependencies` step
**Alternatives considered:** Keep `lld` (default); limit lld thread count via `-Wl,--threads=1`; use `sccache`; pre-bake mold into the runner image
**Rationale:** `lld`'s parallel link stage materialises the full link graph across multiple threads simultaneously and exceeds available memory when linking large test binaries (`ws_tests`, `notify_listener_tests`). This produced reproducible `SIGBUS` (signal 7) linker crashes when CI ran on GitHub-hosted runners. `mold` uses a fundamentally different incremental linking approach with much lower peak memory, and is available in apt on Ubuntu 24.04 (the loupe-app/ci-runner base) without a third-party installer. `cmake` and `pkg-config` are bundled into the same step because `aws-lc-sys` (pulled by `tls-rustls-aws-lc-rs`) needs `cmake` at build time; bundling avoids a second sudo invocation. Setting `RUSTFLAGS` at the backend job level means every cargo invocation that links (`llvm-cov`, `cargo build --release`) uses `mold`; steps that do not link (`cargo fmt`, `cargo clippy`, `cargo check`) are unaffected. Local macOS development is unaffected since the flag is CI-only. Pre-baking the deps into the runner image is rejected because runtime apt-install costs ~10 s and avoids an inter-repo dependency on the runner image's release cadence.

**Decision:** Backend tests run via `cargo llvm-cov` (built-in `cargo test`) in CI, not `cargo llvm-cov nextest` — even though local `just test` uses `cargo nextest run`
**Alternatives considered:** Use `cargo llvm-cov nextest` in CI to match the local runner
**Rationale:** Local `cargo nextest run --workspace` is dramatically faster than `cargo test --workspace` (~9× on macOS/OrbStack with 18 cores) because nextest parallelizes across the workspace's ~25 PG-backed integration test binaries. In CI, however, the same change measured *slower* than `cargo llvm-cov`: the test step went from 5:28 to 8:07. The likely cause is the interaction between three CI-specific factors — (1) coverage instrumentation runs once per test process, and nextest's per-test process model (319 processes vs. ~30 binary processes under `cargo test`) multiplies that init cost, (2) GitHub-hosted runners have only 4 vCPUs, so the per-test overhead saturates the runner's compute, and (3) up to 4 concurrent testcontainers boots contend on the Docker daemon under coverage-instrumented load. The local 9× win does not survive these constraints. Local tooling (mise, justfile, lefthook, `backend/.config/nextest.toml`) keeps using nextest; CI keeps using `cargo llvm-cov`. Re-evaluate when GitHub-hosted runners gain more vCPUs or when coverage tooling supports merged per-binary profiling under nextest.

**Decision:** In the Backend job, `Verify generated TypeScript types are current` runs BEFORE `Run tests with coverage`
**Alternatives considered:** Keep coverage-then-verify order (original); `cargo clean` between steps; split verify-types into its own job
**Rationale:** The verify-types step runs `cargo test --workspace -- export_bindings`, a non-instrumented build of the workspace. The coverage step runs `cargo llvm-cov --workspace`, a coverage-instrumented build. The two builds use different `RUSTFLAGS`, so cargo cannot reuse artifacts between them — each produces its own ~3-9 GB of `target/` content. Reordering (verify-types first) keeps the peak smaller because instrumented artifacts layer on top of a smaller baseline `target/` rather than two stacked coverage builds. **Historical note:** when this job ran on GitHub-hosted `ubuntu-24.04`, a `jlumbroso/free-disk-space` step preceded the build steps to reclaim ~10–15 GB of pre-installed software (Android SDK, .NET, Haskell, Docker images) that the runner OS layered on top of the ~14 GB of free disk; the linker failed with "No space left on device" without it. The step was removed when the job moved to the self-hosted `[self-hosted, linux, amd64]` runner whose image starts with ~640 GB of free disk and no GitHub-hosted bloat to reclaim. Restore the step (and the order rationale stays) if returning to GitHub-hosted Linux runners.

## Boundaries

**Owns:** Workflow file definitions (.github/workflows/), test execution configuration, linting rules, security scanning configuration
**Does not own:** Application code (test by backend/frontend tests), deployment pipelines, artifact generation, secret management
**Prohibitions:** Never store secrets in workflow files — use GitHub Secrets. Never commit workflow files without matching updates to ci-pipeline.md. Never bypass the pre-push gate (no --no-verify).

## Files

- `.github/workflows/ci.yml` — Quality pipeline (linting, type-checking, testing, building); includes `helm` job with 2×5 kubeconform matrix and `helm-result` gate. Frontend artifact upload includes `coverage/lcov.info` and `test-results/frame-budget-trace.json` (the Tier 2 informational frame-budget trace produced by `e2e/frame-budget.test.ts`).
- `.github/workflows/zizmor.yml` — Workflow security linter
- `scripts/doc-mapping.sh` — Maps workflow file changes to this architecture doc
