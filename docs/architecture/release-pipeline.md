# Release Pipeline — Architecture

Last verified: 2026-04-08

## Purpose

The release pipeline automates versioning, artifact generation, and publication for the ATC project. It follows a two-phase design:

1. **Version management (release-please.yml)** — Monitors conventional commits on main and automatically creates/updates a release PR that bumps versions across all packages (3 Rust crates + frontend in lockstep), updates CHANGELOG files, and manages git tags. When merged, the release PR creates a git tag that triggers the release workflow.

2. **Artifact production and publication (release.yml)** — Triggered by git tags (v*), builds `atc-server` binaries for Linux (x86_64, aarch64) and macOS (Apple Silicon), creates a multi-arch Docker image, and publishes all artifacts with Sigstore attestation for supply chain security.

Both workflows integrate with the conventional commits framework to provide fully automated, auditable releases.

## Key Decisions

**Decision:** Two-phase release workflow (release-please + tag-triggered build)

**Alternatives considered:** Single monolithic workflow; event-driven on PR merge

**Rationale:** Separation allows release-please to manage versioning/tagging as a git concern, while the release workflow focuses purely on artifact production. The tag trigger ensures releases only occur when a human has reviewed and merged a release PR. This preserves the review gate while automating routine tasks.

---

**Decision:** Native multi-architecture builds via GitHub Actions runners

**Alternatives considered:** QEMU emulation; separate per-platform workflows; pre-built base images

**Rationale:** Native compilation avoids QEMU overhead and produces genuinely native binaries. GitHub Actions provides native aarch64 runners at comparable cost. Reduces build time and complexity, improving reliability.

---

**Decision:** cargo-chef for Docker layer caching

**Alternatives considered:** Docker buildx with explicit cache; base image pre-built with dependencies

**Rationale:** cargo-chef efficiently caches Rust dependency compilation across builds by producing a "recipe" of dependencies. This keeps Docker build cache layers stable and predictable, reducing rebuild time when only application code changes.

---

**Decision:** distroless runtime image

**Alternatives considered:** Alpine, Debian slim, scratch

**Rationale:** Distroless images are minimal (no shell, package manager, or unused binaries), reducing attack surface and image size. Security scanning tools perform better on distroless because there are no system packages to audit. The image contains only the application and its runtime dependencies.

---

**Decision:** Sigstore attestation via actions/attest

**Alternatives considered:** GPG signing; unsigned artifacts; SLSA provenance only

**Rationale:** Sigstore attestation provides SLSA provenance records (build environment, inputs, outputs) verifiable via GitHub's trust root. Consumers can verify that artifacts were built by this repo's CI, not compromised. Actions/attest provides zero-ceremony integration with GitHub Actions runners.

---

**Decision:** taiki-e action ecosystem for release management

**Alternatives considered:** Custom shell scripts; cargo-release; other tooling

**Rationale:** taiki-e's cross and upload-release actions provide reliable, well-maintained multi-arch Rust builds and release asset publishing. They handle platform-specific complexities (musl libc, native flags) transparently. Well-integrated with the Rust ecosystem.

---

**Decision:** Per-platform GitHub Actions cache scopes

**Alternatives considered:** Global cache; no caching; cache per job type

**Rationale:** GitHub Actions assigns cache writes per platform/runner type. Explicitly scoping caches per platform (linux-x86_64, linux-aarch64, macos) prevents cache misses from cross-platform differences and maximizes hit rates. Rust's target-specific build artifacts are inherently platform-sensitive.

---

**Decision:** GITHUB_TOKEN for ghcr.io and binary upload; GitHub App installation token for release-please

**Alternatives considered:** GITHUB_TOKEN everywhere; personal access token (PAT) for release-please

**Rationale:** GITHUB_TOKEN is generated per-job with minimal scopes and auto-revoked, making it the right choice for ghcr.io pushes and binary uploads where the bearer only needs short-lived write to the current repo. **However**, commits and PR events created by GITHUB_TOKEN do not trigger downstream workflows (GitHub's loop-prevention rule), which would leave the release PR with no CI status checks. release-please.yml therefore mints an installation token for the org-wide releaser GitHub App via `actions/create-github-app-token` (`vars.RELEASER_BOT_CLIENT_ID` + `secrets.RELEASER_BOT_PRIVATE_KEY`) and passes it to the release-please action, so the release PR is opened under the bot identity and PR-level workflows actually run on it. The bot is preferred over a PAT because installation tokens are short-lived (1 hour), are not tied to any individual user, and are scoped per-installation rather than per-account.

---

**Decision:** Frontend is built once and shared with the binary matrix via artifact

**Alternatives considered:** Build the frontend inline in each binary matrix job; ship a stub `frontend/dist` (as `ci.yml` does); restructure rust-embed to be optional

**Rationale:** `atc-server` embeds `frontend/dist` at compile time via `rust-embed` (see `backend/crates/atc-server/src/assets.rs`), so any release build from a clean checkout fails or ships an empty SPA. Rather than building the frontend three times in the binary matrix (one per target triple), `release.yml` has a dedicated `build-frontend` job that runs once on `ubuntu-24.04`, uploads `frontend/dist` as a workflow artifact, and is added as a `needs:` of `build-binaries`. Each matrix job downloads the artifact into `frontend/dist/` before invoking `cargo build`. The `build-container` job is unaffected — its Dockerfile has its own frontend stage that builds from source.

---

**Decision:** pnpm version is pinned via the `packageManager` field in `frontend/package.json`

**Alternatives considered:** Pass `PNPM_VERSION` as a Docker build arg sourced from `.mise.toml`; rely on Corepack's default pnpm (whatever the active Node release ships); add `pnpm/action-setup` everywhere

**Rationale:** Setting `"packageManager": "pnpm@<version>"` in `frontend/package.json` gives every Corepack-enabled environment a single source of truth for the pnpm version: the Dockerfile frontend stage (which already copies `frontend/package.json` into the build context), the `build-frontend` job in `release.yml`, and any local dev that uses Corepack. Renovate's npm manager auto-bumps this field with no additional configuration, so the pin stays current without manual maintenance. `.mise.toml` retains its own `pnpm` pin for local-dev tool provisioning; Renovate's mise manager keeps that one in sync independently. The version part of the field updates reliably; integrity hashes (`pnpm@x.y.z+sha512.…`) are an open issue in Renovate, so we deliberately don't include one.

---

**Decision:** Helm chart release-please integration (2026-04-08)

**Rationale:** The Helm chart at `deploy/helm/atc` is registered as a fifth release-please package with `release-type: helm`. It is deliberately excluded from the `linked-versions` plugin so that its version can evolve independently (chart-only fixes, template improvements, and values schema changes should not force an app version bump, and vice versa).

`Chart.yaml`'s `appVersion` field is kept in sync with the linked app version via the `sync-helm-app-version` bot job in `release-please.yml`. The job runs after `refresh-lockfile`, reads the current `backend/crates/atc-server` value from `.release-please-manifest.json` using `jq`, and rewrites `appVersion` via `sed`. It is idempotent — a `git diff --quiet` guard skips the commit when the value is already correct.

**Rejected alternatives:**
- *Chart in linked-versions group:* Rejected. Chart version must evolve independently per the Definition of Done. Coupling the chart version to the app version would force chart-only releases to match app semver, defeating the purpose of a separately versioned chart.
- *`extra-files` JSONPath for appVersion:* Rejected. release-please's `extra-files` with a JSONPath expression can only write a static version string — it cannot dynamically resolve "the current linked app version." The bot job reads the actual resolved value from the manifest at runtime.
- *Manual appVersion edits:* Rejected. Operator error surface — a reviewer could merge a release PR where `appVersion` is stale and the chart would advertise the wrong app version to Helm users.

---

**Decision:** Bot-driven `Cargo.lock` refresh on release PRs, paired with `--locked` everywhere

**Alternatives considered:** Skip `--locked` and let lockfiles self-heal on next build; keep the `cargo-workspace` plugin (which would update `Cargo.lock` automatically); refresh the lockfile manually before merging release PRs

**Rationale:** Reproducible release artifacts require `--locked` builds, but release-please bumps crate versions in `Cargo.toml` without touching `Cargo.lock` (we dropped the `cargo-workspace` plugin because of release-please issue #2589 — the plugin hardcodes `Cargo.toml` at the repo root and our workspace lives at `backend/Cargo.toml`, with no flag to override). Without intervention, every release PR would fail its own `--locked` CI. The `refresh-lockfile` job in release-please.yml runs after release-please, mints its own installation token, checks out the release PR branch under the bot identity, runs `cargo update --workspace` in `backend/` (which only rewrites workspace member entries — not transitive deps), and pushes the refreshed lockfile back. The bot-attributed push triggers downstream CI, so the release PR ends up with green `--locked` status checks. `--locked` is enforced in ci.yml, release.yml's binary build, and the Dockerfile.

---

**Decision:** Dockerfile runtime is `gcr.io/distroless/cc-debian13:nonroot` (UID 65532)

**Alternatives considered:** root image + chart-level runAsUser override; Alpine slim image with explicit USER directive

**Rationale:** The Dockerfile uses `gcr.io/distroless/cc-debian13:nonroot` with an explicit `USER 65532:65532` directive. The design plan for Phase 6 (written 2026-04-08) assumed the Dockerfile was still on the root `cc-debian13` tag and that Phase 6 would flip it to `:nonroot`. This assumption was out of date: the Dockerfile was already on `:nonroot` with an explicit `USER 65532:65532` directive prior to Phase 6 landing. Phase 6 treats this as verified-correct rather than a change. The `USER 65532:65532` line is redundant with the `:nonroot` tag's baked-in identity but is harmless and is left in place. Removing it would be a cosmetic change with no security benefit. The rejected alternative of setting `runAsUser: 65532` only in the Helm chart's `podSecurityContext` would achieve runtime non-root behavior but would create drift between the image's declared identity and its Kubernetes-enforced identity. If the image is ever run outside Kubernetes (e.g., bare Docker), it would run as root. The `:nonroot` tag eliminates this class of misconfiguration entirely.

---

**Decision:** Helm chart published to `oci://ghcr.io/<owner>/charts/atc` via tag-triggered `release.yml` with Sigstore attestation

**Alternatives considered:** GitHub Pages + chart-releaser; publish on every push to main; unsigned artifacts

**Rationale:** The `publish-helm-chart` job added to `release.yml` packages the chart with `helm package`, pushes to ghcr.io OCI with `helm push`, and generates a Sigstore build-provenance attestation via `actions/attest-build-provenance`. The job is gated on `needs: [create-release, build-container, merge-manifest]`, ensuring a chart artifact is never published unless the corresponding container image succeeded. Publishing is tag-triggered (matching the rest of `release.yml`) rather than on every push to main, which guarantees chart versions correspond to tagged application releases. The rejected alternative of `helm/chart-releaser-action` publishes to a GitHub Pages branch and maintains a classic HTTP chart index — this is valid and may be added in a future issue, but it requires additional workflow and branch setup. OCI publishing is sufficient for Phase 6 and integrates cleanly with the existing ghcr.io registry used for container images. The chart tarball is attested via Sigstore, providing SLSA provenance records verifiable by consumers.

## Boundaries

**Owns:**

- `.github/workflows/release-please.yml` — Automated version bumping and tagging
- `.github/workflows/release.yml` — Artifact build and publication
- `Dockerfile` — Multi-stage container build with dependency caching and distroless runtime
- `.dockerignore` — Docker build context filter (excludes large/irrelevant files)
- `release-please-config.json` — release-please manifest (version sync, changelog paths, bump rules)
- `.release-please-manifest.json` — Version tracker for release-please

**Does not own:**

- CI pipeline (ci.yml, zizmor.yml — see ci-pipeline.md)
- Application code
- Helm chart publishing (future)
- Deployment automation (future)
- Supply chain scanning beyond Sigstore attestation

**Prohibitions:**

- Never create a GitHub Release from release-please (use `skip-github-release: true`; the release.yml workflow creates releases instead)
- Never use QEMU emulation for multi-arch builds (use native GitHub Actions runners only)
- Never use personal access tokens (PATs) anywhere in the release pipeline — use GITHUB_TOKEN for ghcr.io/binary upload, and the releaser GitHub App installation token (via `actions/create-github-app-token`) for any operation that needs to trigger downstream workflows
- Never commit release-please config without ensuring the `linked-versions` plugin is enabled (enforces lockstep versioning)
- Never re-add the `cargo-workspace` plugin to `release-please-config.json` without first verifying release-please issue #2589 is fixed; the plugin hardcodes `Cargo.toml` at the repo root and will fail because our workspace lives at `backend/Cargo.toml`
- Never remove `--locked` from CI, release.yml, or the Dockerfile without also removing the bot-driven `Cargo.lock` refresh job (the two are paired — one without the other booby-traps release PRs)

## Files

- `.github/workflows/release-please.yml` — Automated release PR creation with Conventional Commits bump detection
- `.github/workflows/release.yml` — Tag-triggered artifact build (binaries, Docker image) and publication
- `Dockerfile` — Multi-stage Rust build with cargo-chef caching; distroless runtime
- `.dockerignore` — Build context filter (excludes docs, git, CI configs, test files)
- `release-please-config.json` — Configures release-please plugins, version sync, and CHANGELOG generation
- `.release-please-manifest.json` — Tracks current versions for all packages
