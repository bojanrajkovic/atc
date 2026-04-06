# Release Pipeline — Architecture

Last verified: 2026-04-06

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

**Decision:** GITHUB_TOKEN only for ghcr.io authentication

**Alternatives considered:** Personal access tokens (PATs); GITHUB_TOKEN with broader scopes

**Rationale:** GITHUB_TOKEN is generated per-job with minimal scopes and automatically revoked. PATs are long-lived and easier to misuse. GITHUB_TOKEN cannot write to other repos, reducing blast radius of any workflow compromise.

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
- Never use personal access tokens (PATs) for ghcr.io authentication (GITHUB_TOKEN only)
- Never commit release-please config without ensuring the `linked-versions` plugin is enabled (enforces lockstep versioning)

## Files

- `.github/workflows/release-please.yml` — Automated release PR creation with Conventional Commits bump detection
- `.github/workflows/release.yml` — Tag-triggered artifact build (binaries, Docker image) and publication
- `Dockerfile` — Multi-stage Rust build with cargo-chef caching; distroless runtime
- `.dockerignore` — Build context filter (excludes docs, git, CI configs, test files)
- `release-please-config.json` — Configures release-please plugins, version sync, and CHANGELOG generation
- `.release-please-manifest.json` — Tracks current versions for all packages
