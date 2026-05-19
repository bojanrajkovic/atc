# Release Pipeline — Architecture

Last verified: 2026-05-18


## Purpose

The release pipeline automates versioning, artifact generation, and publication for the ATC project. It follows a two-phase design:

1. **Version management (release-please.yml)** — Monitors conventional commits on main and automatically creates/updates a release PR that bumps versions across all packages (7 Rust crates + frontend in lockstep), updates CHANGELOG files, and manages git tags. When merged, the release PR creates a git tag that triggers the release workflow. The seven Rust crates are: `atc-core`, `atc-github`, `atc-wire`, `atc-persist`, `atc-store-pg`, `atc-store-mem`, and `atc-server`. All seven are members of the `linked-versions` group, so they bump together on every release; the persistence split landed in issue #169 (ADR-0008) and the four newer crates (`atc-wire`, `atc-persist`, `atc-store-pg`, `atc-store-mem`) have been registered since.

2. **Artifact production and publication (release.yml)** — Triggered by git tags (v*), builds `atc-server` binaries for Linux (x86_64, aarch64) and macOS (Apple Silicon), creates a multi-arch Docker image, and publishes all artifacts with Sigstore attestation for supply chain security.

Both workflows integrate with the conventional commits framework to provide fully automated, auditable releases.

## Key Decisions

**Decision:** Two-phase release workflow (release-please + tag-triggered build)

**Alternatives considered:** Single monolithic workflow; event-driven on PR merge

**Rationale:** Separation allows release-please to manage versioning/tagging as a git concern, while the release workflow focuses purely on artifact production. The tag trigger ensures releases only occur when a human has reviewed and merged a release PR. This preserves the review gate while automating routine tasks.

---

**Decision:** Native multi-architecture builds — each platform runs on its native GitHub-hosted runner (`ubuntu-24.04` for linux/amd64, `ubuntu-24.04-arm` for linux/arm64, `macos-26` for aarch64-apple-darwin)

**Alternatives considered:** QEMU emulation under `docker buildx`; cross-compilation on a single host with a foreign-arch toolchain; separate per-platform workflows; pre-built base images

**Rationale:** Native compilation avoids QEMU's ~5× compile-time penalty and sidesteps the entire class of glibc-vs-musl C-toolchain mismatch failures that cross-compilation creates (see Decision below). `ubuntu-24.04-arm` (GitHub's pinned arm64 hosted runner, free for public-repo OSS minutes since 2024) is the right primitive once it's available; QEMU is reserved for the case where a target arch has no native runner, which is not our situation.

---

**Decision:** cargo-chef for Docker layer caching

**Alternatives considered:** Docker buildx with explicit cache; base image pre-built with dependencies

**Rationale:** cargo-chef efficiently caches Rust dependency compilation across builds by producing a "recipe" of dependencies. This keeps Docker build cache layers stable and predictable, reducing rebuild time when only application code changes. The planner stage manually enumerates every workspace member's `Cargo.toml` and stub src file (rather than relying on the workspace glob); this list must be kept in sync whenever workspace members are added or removed — failing to do so causes `cargo chef prepare` to abort with a missing-manifest error (as observed when issue #169 added four new crates without updating the Dockerfile).

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

**Decision:** No GitHub Actions cache on any release-pipeline build job

**Alternatives considered:** Per-platform cargo cache (`Swatinem/rust-cache` keyed by target triple) on `build-binaries`; GHA layer cache (`cache-from: type=gha`) on `build-container`

**Rationale:** Release artifacts are signed-and-attested binaries shipped to ghcr.io and GitHub Releases. zizmor's [`cache-poisoning`](https://docs.zizmor.sh/audits/#cache-poisoning) audit rule flags this exact pattern: "an attacker with access to a valid `GITHUB_TOKEN` can use it to poison the repository's GitHub Actions caches…When a release workflow then restores from these poisoned caches, it can retrieve malicious payloads, achieving code execution and potentially compromising artifacts before publication." `actions/cache` scopes by branch, but PR runs against any branch (including `main`) can write entries that a later tag build reads back through cache restore fallback. Cargo's registry cache in particular is a near-perfect injection surface (a poisoned crates.io index entry could swap a dependency to a malicious mirror without changing `Cargo.lock`). `build-binaries` accepts the cold cargo build (~3 minutes per matrix entry) as the price of trustworthy release artifacts. `build-container` compiles the regular multi-stage `Dockerfile` cold per release on each native runner; both matrix entries pay the cargo-chef plan-then-cook cost from scratch, again accepted as the price of cache-poisoning resistance. Renaming or scoping caches per-branch does not solve this: any contributor (or any compromised PR) could still write into a branch that the tag-triggered workflow falls back to.

---

**Decision:** GITHUB_TOKEN for ghcr.io and binary upload; GitHub App installation token for release-please

**Alternatives considered:** GITHUB_TOKEN everywhere; personal access token (PAT) for release-please

**Rationale:** GITHUB_TOKEN is generated per-job with minimal scopes and auto-revoked, making it the right choice for ghcr.io pushes and binary uploads where the bearer only needs short-lived write to the current repo. **However**, commits and PR events created by GITHUB_TOKEN do not trigger downstream workflows (GitHub's loop-prevention rule), which would leave the release PR with no CI status checks. release-please.yml therefore mints an installation token for the org-wide releaser GitHub App via `actions/create-github-app-token` (`vars.RELEASER_BOT_CLIENT_ID` + `secrets.RELEASER_BOT_PRIVATE_KEY`) and passes it to the release-please action, so the release PR is opened under the bot identity and PR-level workflows actually run on it. The bot is preferred over a PAT because installation tokens are short-lived (1 hour), are not tied to any individual user, and are scoped per-installation rather than per-account. Each `create-github-app-token` call passes `permission-<scope>` inputs so the minted token carries only the scopes that step needs (release-please: `contents: write` + `pull-requests: write` + `issues: write` for autorelease label management, which goes through the shared issues API per the upstream action's README; the lockfile and Helm `appVersion` sync jobs: `contents: write`), rather than inheriting the App's full installation permission set — zizmor's [`github-app`](https://docs.zizmor.sh/audits/#github-app) audit enforces this least-privilege shape.

---

**Decision:** Frontend is built once and shared with the binary matrix via artifact

**Alternatives considered:** Build the frontend inline in each binary matrix job; ship a stub `frontend/dist` (as `ci.yml` does); restructure rust-embed to be optional

**Rationale:** `atc-server` embeds `frontend/dist` at compile time via `rust-embed` (see `backend/crates/atc-server/src/assets.rs`), so any release build from a clean checkout fails or ships an empty SPA. Rather than building the frontend three times in the `build-binaries` matrix (one per target triple), `release.yml` has a dedicated `build-frontend` job that runs once, uploads `frontend/dist` as a workflow artifact, and is added as a `needs:` of `build-binaries`. Each `build-binaries` matrix job downloads the artifact into `frontend/dist/` before invoking `cargo build`. `build-container` runs the regular multi-stage `Dockerfile`, which has its own internal frontend-build stage — it doesn't consume the `build-frontend` artifact, but it does benefit from native arm64 compilation (no QEMU pnpm penalty) on the `ubuntu-24.04-arm` runner.

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

**Rationale:** The Dockerfile uses `gcr.io/distroless/cc-debian13:nonroot` with an explicit `USER 65532:65532` directive. The `USER 65532:65532` line is redundant with the `:nonroot` tag's baked-in identity but is harmless and is left in place — removing it would be a cosmetic change with no security benefit. The rejected alternative of setting `runAsUser: 65532` only in the Helm chart's `podSecurityContext` would achieve runtime non-root behavior but would create drift between the image's declared identity and its Kubernetes-enforced identity. If the image is ever run outside Kubernetes (e.g., bare Docker), it would run as root. The `:nonroot` tag eliminates this class of misconfiguration entirely.

---

**Decision:** Helm chart published via two parallel channels — OCI on `ghcr.io/<owner>/charts/atc` (Sigstore-attested) and a classic HTTP repo on GitHub Pages (`https://<owner>.github.io/<repo>/charts`)

**Alternatives considered:** OCI only; GitHub Pages only; publish on every push to main; unsigned artifacts; chart index at the gh-pages branch root vs. a `charts/` subpath

**Rationale:** Both channels are tag-triggered from `release.yml` so chart versions always correspond to tagged application releases, and they stay in lockstep on the same tag rather than drifting between distribution surfaces. The split exists because the two surfaces serve different consumer profiles:

- **OCI (`publish-helm-chart`)** — the canonical channel for OCI-native workflows. Packages the chart with `helm package`, pushes to ghcr.io OCI with `helm push`, and generates a Sigstore build-provenance attestation via `actions/attest-build-provenance`. Gated on `needs: [create-release, build-container, merge-manifest]`, so a chart artifact is never published unless the corresponding container image succeeded. Integrates cleanly with the existing ghcr.io registry used for container images.
- **GitHub Pages (`publish-helm-pages`)** — the recommended channel for consumers without GHCR authentication (a `helm repo add` URL works against any laptop or CI without registry credentials). Uses `helm/chart-releaser-action` to package the chart, attach a `.tgz` to a per-chart `atc-<version>` GitHub Release, and refresh `index.yaml` on the `gh-pages` branch. Gated on `needs: publish-helm-chart` so the OCI channel must succeed first; this preserves the existing supply-chain ordering (chart only lands publicly after its container image and OCI artifact are in place). The action is pinned to the SHA of `helm/chart-releaser-action@v1.7.0`, with the underlying `cr` binary explicitly pinned to `v1.8.1` (matching the `helm-cr` tool in `.mise.toml`) so local dry-runs and CI agree.

`mark_as_latest: false` on the chart-releaser invocation prevents the per-chart `atc-<version>` release from clobbering the canonical `v<version>` release that `taiki-e/create-gh-release-action` already creates with the binaries. `fetch-depth: 0` on the checkout is required by chart-releaser, which diffs against prior tags. Permissions are `contents: write` only — Pages serves passively from the `gh-pages` branch, so no `pages: write` scope is needed.

**`charts/` subpath layout (deploy/helm/cr.yaml):** `index.yaml` lives at `gh-pages/charts/index.yaml`, not the branch root. The action takes `pages-index-path: charts` from `deploy/helm/cr.yaml` (passed via the `config` input) and `cr index` writes the index file under that subdirectory. `.tgz` artifacts continue to live on per-chart GitHub Releases — the `urls:` field inside `index.yaml` points there regardless of where `index.yaml` itself sits, so only one path moves.

The subpath layout is a forward-compatibility decision: it reserves the gh-pages branch root for a future static docs site at `https://<owner>.github.io/<repo>/`, served from the same Pages instance. Once consumers run `helm repo add atc https://<host>/<repo>/charts`, that URL becomes a public contract — moving the chart index later would break every operator who already configured it. Picking the `/charts` subpath before any consumer adopts the chart is essentially free; renaming after adoption is not.

The same shape carries through to a custom domain: if `<owner>.github.io/<repo>` is later remapped to `https://atc.example.com/`, the chart index becomes `https://atc.example.com/charts` (the host changes, the subpath does not). chart-releaser preserves any `CNAME` file at the gh-pages branch root across runs because `cr` uses `git worktree add` to check out the existing tree and `git add <specific paths>` to stage only the modified `charts/` files — the root `CNAME` is never touched. (Verified against `helm/chart-releaser` `pkg/git/git.go` and `pkg/releaser/releaser.go` at v1.8.1.) Operators can therefore add a custom domain at any time without coordinating with the release pipeline.

**Manual prerequisite:** GitHub Pages must be enabled with source = `gh-pages` branch in repo Settings → Pages. chart-releaser does not provision Pages itself — the first release run will create the `gh-pages` branch, but the operator must wire it to Pages once.

The Sigstore attestation lives only on the OCI channel — `actions/attest-build-provenance` writes the attestation against a published OCI subject path, which the GitHub Pages tarball does not have. Consumers who need SLSA provenance verification should pull from OCI; the Pages channel is the auth-free convenience surface.

---

**Decision:** Native arm64 Linux artifacts via `ubuntu-24.04-arm` — both the `aarch64-unknown-linux-musl` binary and the `linux/arm64` container image build natively on the GitHub-hosted arm64 runner; the regular multi-stage `Dockerfile` is the single canonical entry point for every container build (no parallel "release-only" Dockerfile)

**Alternatives considered:** Cross-compile on amd64 (the private-dev pattern, see Historical below); QEMU emulation in `docker buildx`; pay-for-minutes private GitHub-hosted runners; pre-built base images

**Rationale:** `ubuntu-24.04-arm` is GitHub's pinned arm64 hosted runner, free for public-repo OSS minutes since 2024. Native compilation eliminates the entire C-toolchain mismatch class (the failure mode that broke earlier cross-attempts — see Historical) plus the multi-stage `Dockerfile`'s pnpm + cargo + frontend phases run at native speed rather than the ~5× QEMU penalty. On the binary side, `build-binaries`'s `aarch64-unknown-linux-musl` matrix entry runs on `ubuntu-24.04-arm` with `musl-tools` + `musl-dev` installed via apt; `taiki-e/upload-rust-binary-action` + `dtolnay/rust-toolchain` handle the rest. On the container side, `build-container`'s `linux/arm64` matrix entry also runs on `ubuntu-24.04-arm`; `docker/build-push-action` runs the regular `Dockerfile` against `--platform linux/arm64` natively (buildx is single-arch when the host arch matches the platform). The two pipeline halves no longer share artifacts — `build-container` doesn't `needs: build-binaries`, doesn't download anything from `build-binaries`, and rebuilds the binary inside the Dockerfile's cargo stage. The cost is double-compile (once for the GH Release binary, once inside the container) traded for pipeline simplicity and a single Dockerfile to maintain.

**Historical (private-dev phase, 2026-04 to 2026-05):** When Linux jobs ran on a homelab amd64-only ARC scale set, arm64 artifacts were produced via cross-compilation rather than native runners. `build-binaries`'s `aarch64-unknown-linux-musl` matrix entry used `taiki-e/setup-cross-toolchain-action` to extract a musl cross-toolchain from `ghcr.io/taiki-e/rust-cross-toolchain:aarch64-unknown-linux-musl1.2-dev-amd64` into `/usr/local`, exporting `CARGO_TARGET_*_LINKER`, `CC_*`, `CXX_*`, `AR_*`, `RANLIB_*` env vars so cargo + `taiki-e/upload-rust-binary-action` emitted a static-musl binary linked through `aarch64-unknown-linux-musl-gcc`. `build-container` then consumed each `linux-musl` binary via a `binary-${target}` workflow artifact (~12–15 MB per arch, `compression-level: 0`) and packaged it through a thin `Dockerfile.release` — a four-line `FROM gcr.io/distroless/cc-debian13:nonroot` + `COPY atc-server` + `USER` + `ENTRYPOINT` image with `Dockerfile.release.dockerignore` allowlisting only the binary. Both matrix entries ran on `[self-hosted, linux, amd64]`. The earlier cross-compile attempt that used the runner image's `gcc-aarch64-linux-gnu` (glibc-targeting) as the linker for a `+crt-static` musl build (issue #75) failed because `aws-lc-sys`'s C objects emit glibc-internal symbols (`__isoc23_sscanf`, `__memcpy_chk`, `__fprintf_chk`, `__isoc23_strtol`) that musl libc does not provide; the `setup-cross-toolchain-action` route sidestepped that by compiling C against musl headers from the start. All of this — `Dockerfile.release`, `Dockerfile.release.dockerignore`, the cross-toolchain action, and the binary-handoff artifact — was deleted in the public-flip undo (issue #74). The regular multi-stage `Dockerfile` is unchanged from the private-dev era.

## Boundaries

**Owns:**

- `.github/workflows/release-please.yml` — Automated version bumping and tagging
- `.github/workflows/release.yml` — Artifact build and publication
- `Dockerfile` — Multi-stage container build with dependency caching and distroless runtime; canonical for `docker build .` and for `build-container` in `release.yml`
- `.dockerignore` — Docker build context filter for `Dockerfile`; allowlist pattern (`backend/`, `frontend/`, `.git`); `.git` is included so vergen-gix can embed real commit metadata at build time
- `release-please-config.json` — release-please manifest (version sync, changelog paths, bump rules)
- `.release-please-manifest.json` — Version tracker for release-please

**Does not own:**

- CI pipeline (ci.yml, zizmor.yml — see ci-pipeline.md)
- Application code
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
- `Dockerfile` — Multi-stage Rust build with cargo-chef caching; distroless runtime; canonical for `docker build .` and the `build-container` matrix job in `release.yml`
- `.dockerignore` — Build context filter for `Dockerfile`; allowlist pattern (`backend/`, `frontend/`, `.git`); `.git` is included so vergen-gix can embed real commit metadata at build time
- `release-please-config.json` — Configures release-please plugins, version sync, and CHANGELOG generation
- `.release-please-manifest.json` — Tracks current versions for all packages
- `deploy/helm/cr.yaml` — chart-releaser config; pins `pages-index-path: charts` so the Helm index lives at `gh-pages/charts/index.yaml` and the gh-pages branch root stays available for a future docs site
