# Test Plan — Release Pipeline

**Implementation plan:** `docs/implementation-plans/2026-04-04-release-pipeline/`
**Generated:** 2026-04-06
**HEAD at generation:** `69a3381`

## Coverage Summary

- **Automated criteria:** 14 (workflow/Dockerfile static verifications)
- **Manual criteria:** 21 (require live GitHub Actions runs, ghcr.io pushes, and Sigstore verification)
- **Status:** PASS — all acceptance criteria have a verification path (automated or manual)

## Prerequisites

- Push access to `bojanrajkovic/atc`
- `gh` CLI authenticated; `docker` with `buildx` enabled; `zizmor` installed locally (`uvx zizmor` works)
- Local clone at `/home/brajkovic/Projects/atc/` on `main` after this PR merges
- Working tree clean

## Phase 0: Local Static Verification (no GitHub run needed)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 0.1 | `uvx zizmor .github/workflows/release.yml .github/workflows/release-please.yml` | Exit 0; no findings | AC7.5 |
| 0.2 | `grep -E '^\s+uses:' .github/workflows/release-please.yml .github/workflows/release.yml \| grep -v '@[0-9a-f]\{40\}'` | No output | AC7.1 |
| 0.3 | `grep -c 'persist-credentials: false' .github/workflows/release.yml` | `3` | AC7.2 |
| 0.4 | `head -20 .github/workflows/release.yml .github/workflows/release-please.yml \| grep 'permissions: {}'` | Two matches | AC7.3 |
| 0.5 | `grep -A3 'docker/login-action' .github/workflows/release.yml \| grep 'secrets.GITHUB_TOKEN'` | Two matches | AC7.4 |
| 0.6 | `grep 'mount=type=cache' Dockerfile \| wc -l` | `4` | AC8.2 |
| 0.7 | `grep 'type=gha' .github/workflows/release.yml` | Shows `scope=linux/amd64` and `scope=linux/arm64` | AC8.1 |
| 0.8 | `grep 'FROM gcr.io/distroless/cc-debian12' Dockerfile` | `FROM gcr.io/distroless/cc-debian12:nonroot` | AC5.1 |
| 0.9 | `grep -n 'pnpm build' Dockerfile` | Line hit in frontend stage | AC5.3 |
| 0.10 | `docker build -t atc-server-local .` | Exit 0 | AC5.4 |
| 0.11 | `docker run --rm -d --name atc-test -p 8080:8080 atc-server-local && sleep 2 && curl -sf http://localhost:8080/health; docker stop atc-test` | curl returns `{"status":"ok"}` | AC4.4 (local) |
| 0.12 | `curl -sf http://localhost:8080/ \| head -5` (re-run container if needed) | Returns Svelte SPA HTML | AC4.5 (local) |

## Phase 1: Release-Please PR (AC1.x)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 1.1 | Merge a `feat: ...` commit to main | `release-please.yml` workflow runs green | AC1.1 |
| 1.2 | Open the release PR created by the bot | Title contains version; diff shows minor bump in all 4 manifest files (`backend/crates/atc-core/Cargo.toml`, `backend/crates/atc-github/Cargo.toml`, `backend/crates/atc-server/Cargo.toml`, `frontend/package.json`); all four versions identical | AC1.1, AC1.4 |
| 1.3 | Push a follow-up `fix: ...` commit to main | Release PR updates | AC1.2 |
| 1.4 | Inspect PR diff for `CHANGELOG.md` | Contains entries for the feat and fix commits since last release | AC1.3 |
| 1.5 | Merge the release PR | A `v<version>` git tag appears under repo Tags; Releases page does NOT yet show a Release for this version | AC1.5 |

## Phase 2: Release Workflow (AC2.x, AC3.x)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 2.1 | Open Actions tab immediately after the merge in 1.5 | "Release" workflow has started, triggered by the `v*` tag | AC2.1 |
| 2.2 | Wait for `create-release` job | Green; GitHub Release created with body matching the `backend/crates/atc-server/CHANGELOG.md` entry | AC3.1 |
| 2.3 | Wait for `build-binaries` matrix | All three jobs green: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `aarch64-apple-darwin` | AC2.2, AC2.3, AC2.4 |
| 2.4 | Open the GitHub Release page | 3 archives present (one per target) plus a `.sha256` file for each | AC3.2, AC3.3 |
| 2.5 | `gh release download v<version> -p 'atc-server-x86_64-unknown-linux-musl*' -R bojanrajkovic/atc && tar -xzf atc-server-x86_64-unknown-linux-musl.tar.gz && ldd ./atc-server` | `not a dynamic executable` or `statically linked` | AC2.5 |
| 2.6 | (Optional caveat) Re-run a single matrix job after temporarily breaking it | Confirm Release exists but is missing one artifact — documents AC3.4 design limitation | AC3.4 |

## Phase 3: Container & Multi-Arch (AC4.x)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 3.1 | Wait for `build-container` matrix and `merge-manifest` jobs | All green | AC4.1 |
| 3.2 | `docker pull ghcr.io/bojanrajkovic/atc:<version>` | Pull succeeds | AC4.1 |
| 3.3 | `docker manifest inspect ghcr.io/bojanrajkovic/atc:<version>` | Output lists both `linux/amd64` and `linux/arm64` | AC4.3 |
| 3.4 | Visit GitHub Packages page for the repo | Tags exist for `<version>`, `<major>.<minor>`, `<major>`, `latest` | AC4.2 |
| 3.5 | `docker run -d --name atc-pkg -p 8080:8080 ghcr.io/bojanrajkovic/atc:<version> && sleep 2 && curl -sf http://localhost:8080/health` | Returns `{"status":"ok"}` | AC4.4 |
| 3.6 | `curl -sf http://localhost:8080/ \| head -5` then `docker stop atc-pkg && docker rm atc-pkg` | Returns Svelte SPA HTML | AC4.5 |

## Phase 4: Attestation (AC6.x)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 4.1 | In Release workflow logs, open each `build-binaries` job's "Attest binary" step | Each shows Sigstore attestation creation | AC6.1 |
| 4.2 | Open `merge-manifest` job's "Attest container image" step | Shows attestation creation with `push-to-registry: true` | AC6.2 |
| 4.3 | `gh attestation verify ./atc-server-x86_64-unknown-linux-musl.tar.gz -R bojanrajkovic/atc` | Exit 0; valid attestation | AC6.3 |
| 4.4 | `gh attestation verify oci://ghcr.io/bojanrajkovic/atc:<version> -R bojanrajkovic/atc` | Exit 0; valid attestation | AC6.4 |

## Phase 5: Cache Effectiveness (AC8.3)

| # | Action | Expected | AC |
|---|--------|----------|----|
| 5.1 | Re-run the `build-container` matrix from the Actions UI on the same tag | Compare wall-clock to step 3.1; second run noticeably faster due to GHA cache hits on dependency layers | AC8.3 |

## End-to-End Scenario: First Live Release

Linear walk-through validating the full pipeline from feat commit to verified container:

1. Merge `feat: enable release pipeline` to main.
2. Confirm release-please opens PR with synced versions across 4 manifest files and CHANGELOG entry.
3. Merge release PR; observe tag created, no Release yet.
4. Watch Actions: `create-release` → `build-binaries` (3 targets) → `build-container` (2 platforms) → `merge-manifest` all complete green.
5. From the Release page, download Linux musl binary, verify static linking and attestation.
6. Pull container by `latest`, `<version>`, `<major>.<minor>`, `<major>` tags individually to confirm metadata-action tagging.
7. Inspect manifest for both architectures.
8. Run container, hit `/health` and `/` to confirm healthcheck and embedded SPA.
9. Verify image attestation via `gh attestation verify`.
10. Re-run container build matrix; confirm cache-driven speedup.

## Traceability Matrix

| AC | Automated | Manual |
|----|-----------|--------|
| AC1.1 | — | 1.1, 1.2 |
| AC1.2 | — | 1.3 |
| AC1.3 | — | 1.4 |
| AC1.4 | — | 1.2 |
| AC1.5 | — | 1.5 |
| AC2.1 | — | 2.1 |
| AC2.2 | — | 2.3 |
| AC2.3 | — | 2.3 |
| AC2.4 | — | 2.3 |
| AC2.5 | release.yml matrix rustflags + musl-tools | 2.5 |
| AC3.1 | — | 2.2 |
| AC3.2 | — | 2.4 |
| AC3.3 | — | 2.4 |
| AC3.4 | — | 2.6 (caveat) |
| AC4.1 | — | 3.2 |
| AC4.2 | — | 3.4 |
| AC4.3 | release.yml merge-manifest | 3.3 |
| AC4.4 | Dockerfile EXPOSE/ENTRYPOINT | 0.11, 3.5 |
| AC4.5 | — | 0.12, 3.6 |
| AC5.1 | Dockerfile `FROM` | 0.8 |
| AC5.2 | Dockerfile chef stages | 0.10 |
| AC5.3 | Dockerfile `pnpm build` | 0.9 |
| AC5.4 | Dockerfile self-contained | 0.10 |
| AC6.1 | release.yml attest step | 4.1 |
| AC6.2 | release.yml attest step | 4.2 |
| AC6.3 | — | 4.3 |
| AC6.4 | — | 4.4 |
| AC7.1 | both workflows SHA-pinned | 0.2 |
| AC7.2 | release.yml checkouts | 0.3 |
| AC7.3 | both workflows top-level | 0.4 |
| AC7.4 | release.yml login-action | 0.5 |
| AC7.5 | both workflows | 0.1 |
| AC8.1 | release.yml cache scopes | 0.7 |
| AC8.2 | Dockerfile mount caches | 0.6 |
| AC8.3 | — | 5.1 |
