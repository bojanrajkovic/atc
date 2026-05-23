# ADR 0011 — GitHub Actions + release-please for the release toolchain

Date: 2026-05-23
Status: Accepted

Last verified: 2026-05-23

## Context

ATC ships multiple versioned surfaces: seven Rust crates (the `atc-*` workspace members), a Svelte frontend, and a Helm chart. All of them must move in lockstep — a tag that represents `atc-server v0.1.0` should name the same version across every installable surface, and the changelog the GitHub Release surfaces should trace directly to the commits that produced the release.

The design constraints that shaped the toolchain choice were:

- **Conventional Commits are already enforced** at the commit-msg hook level. Any release tooling that can read conventional commits to derive version bumps gets that discipline for free.
- **Multi-surface lockstep.** Seven Rust crates plus a frontend and a Helm chart need a single version number, not nine independently moving versions.
- **Release artifacts span two build systems.** The standalone binary is a Rust build; the container image is a Docker multi-stage build; the Helm chart is a Helm package. A single release trigger must fan out to all three.
- **Sigstore attestation** must cover binaries, the container manifest, and the Helm chart tarball so consumers can verify provenance via `gh attestation verify`.

## Decision

### Conventional commits as version input

Commit type determines version bump: `feat:` → minor, `fix:` → patch, `feat!:` or `BREAKING CHANGE:` → major. `chore(deps):` and other non-release-triggering types do not produce a version bump. This mapping is enforced by release-please and agrees with the Renovate configuration, which prefixes runtime bumps as `fix(deps):` (patch release) and tooling bumps as `chore(deps):` (no release).

### release-please as version manager

`release-please` runs on every push to `main`. It reads the conventional commit log since the last release, computes the next version for each package, and opens or updates a single release PR that bumps `Cargo.toml` / `package.json` / `Chart.yaml` version fields, refreshes each crate's `CHANGELOG.md`, and updates the version-tracker manifest.

All Rust crates and the frontend version in lockstep via the `linked-versions` plugin — the highest-precedence bump across any member sets the version for the whole group. The Helm chart versions independently (chart changes may not coincide with application changes).

Merging the release PR creates the `v*` tag that triggers the release workflow. The release PR is also the human review surface for the changelog before it ships.

A companion job on the same workflow refreshes `Cargo.lock` after release-please bumps the crate versions (so `cargo build --locked` continues to pass), and a second companion job syncs the Helm chart's `appVersion` field to the new application version. Both jobs commit to the release PR branch under a dedicated GitHub App identity so those commits trigger CI status checks (commits from `GITHUB_TOKEN` do not trigger downstream workflows, which is GitHub's loop-prevention rule).

### Tag-triggered release workflow

Pushing a `v*` tag triggers the release workflow, which runs in dependency order:

1. **Create GitHub Release** — extracts the relevant CHANGELOG section and creates the GitHub Release.
2. **Build frontend** — compiles the Svelte SPA once and shares the output artifact with every binary build. `atc-server` embeds the frontend at compile time, so the binary builds cannot run from a clean checkout without it.
3. **Build binaries** — a matrix builds `atc-server` for three targets (x86_64 Linux musl, aarch64 Linux musl, aarch64 macOS). Linux targets compile natively on GitHub-hosted runners (x86_64 on `ubuntu-24.04`, aarch64 on `ubuntu-24.04-arm`) to avoid the cross-compilation complexity of `aws-lc-sys` against musl. macOS targets build on `macos-26`.
4. **Build container image** — each platform builds natively; a merge job combines the per-platform digests into a multi-arch manifest list published to `ghcr.io`.
5. **Publish Helm chart** — packages and pushes to the OCI registry at `ghcr.io/bojanrajkovic/charts` and to a classic HTTP Helm repo via GitHub Pages.

Pre-release tags (tags containing a hyphen, e.g. `-rc1`) skip Sigstore attestation and Helm chart publishing. This avoids the GitHub API restriction on attestations in private user-owned repos during the pre-public phase, and prevents rc versions from occupying slots in the Helm release channel.

### Multi-arch image build

Platform images build natively (no QEMU emulation) and push to the registry under platform-specific tags. A merge job calls `docker buildx imagetools create` to assemble the multi-arch manifest list and attach the final semver tags. Only the manifest digest is attested, not the per-platform images.

### Sigstore attestation

`actions/attest-build-provenance` attests each binary archive, the multi-arch container manifest, and the Helm chart tarball. Attestations are verifiable via `gh attestation verify` against the repository. This ties every artifact to the specific Actions run and commit SHA that produced it without requiring a separate key-management infrastructure.

## Rejected alternatives

### Raw `cargo-release`

`cargo-release` is an author-driven tool: the developer runs it locally, it bumps versions, commits, tags, and pushes. It handles Rust workspaces well, but the release is a single developer action rather than a PR-gated event. The release PR in the chosen approach serves double duty as a changelog-review surface — the developer sees the generated changelog before it ships and can edit it. With `cargo-release`, the changelog is whatever the tool generates at push time; there is no review gate. The audit trail is also thinner: the release commit exists, but there is no associated PR with labels, CI checks, or a review thread.

### `semantic-release` (Node ecosystem)

`semantic-release` reads conventional commits and automates the release lifecycle, but it is Node-first. Extending it to manage Cargo workspace versions, Helm chart fields, and `Cargo.lock` refresh requires community plugins or custom scripts for each surface. The `release-please` `linked-versions` plugin handles the multi-surface lockstep natively, and its Rust release type knows Cargo workspace layout. `semantic-release` was designed for single-package Node projects; adapting it to this workspace would replicate most of what release-please already provides.

### Custom scripts in-tree

A bespoke shell or Python script could drive version bumps, CHANGELOG generation, tag creation, and artifact builds with full control. The maintenance burden scales with the number of release surfaces: each new artifact type (new binary target, new chart distribution channel, attestation step) is a hand-maintained script block. release-please and the tag-triggered workflow encapsulate well-tested patterns for each of these surfaces; bespoke scripts would re-implement them with less test coverage and more drift risk as upstream tooling evolves.

### Manual tagging + GitHub Release UI

Tagging manually and creating releases through the GitHub UI is the baseline from which all automation departs. Without automation: version fields drift from the git tag unless the author remembers to bump them; CHANGELOGs are written by hand and tend to omit or misattribute changes; the conventional-commit discipline enforced at the hook level has no downstream consumer, making it purely cosmetic. This option was never seriously considered for a project with conventional-commit enforcement already in place.

## Consequences

- **One release PR per release cycle.** The single PR model (rather than per-package PRs) keeps the release surface legible. The tradeoff is that the linked-versions group moves together even when only one crate has changed — patch releases to `atc-core` bump all crates to the same version.
- **Helm chart versions independently.** The Helm chart is excluded from the linked-versions group, so chart-only changes (values schema additions, template fixes) produce a chart release without bumping the application version.
- **`Cargo.lock` is a release artifact.** The lockfile-refresh companion job means the release PR always has a `--locked`-compatible `Cargo.lock` before it merges. Reviewers see the full lockfile diff alongside the version bumps.
- **GitHub App token required for release PRs.** `GITHUB_TOKEN`-created commits and PRs do not trigger downstream workflows (GitHub loop-prevention rule). A dedicated GitHub App mints installation tokens for release-please and the companion jobs so CI runs on the release PR branch.
- **Attestation is repository-scoped.** Verification requires the repository reference (`-R bojanrajkovic/atc`). If the repository moves or is renamed, existing attestations cannot be re-anchored.
- **Pre-release artifacts are unattested.** The private-repo attestation restriction means rc/dev builds carry no verifiable provenance. This is acceptable for pre-release cycles; the restriction dissolves when the repository goes public.

## References

- [CONTRIBUTING.md § Releases](../CONTRIBUTING.md) — human-facing description of the release flow and version-bump rules
- [release-please documentation](https://github.com/googleapis/release-please) — linked-versions plugin, manifest config
- [Sigstore / `gh attestation verify`](https://docs.github.com/en/actions/security-guides/using-artifact-attestations-to-establish-provenance-for-builds)
