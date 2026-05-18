# Public-flip undo list: revert private-only optimizations before going public

Issue: [#74](https://github.com/bojanrajkovic/atc/issues/74)

## Context

The atc repo runs in a private-development phase heading toward v1.0. During that phase PR #73 introduced several optimizations tied to a homelab-hosted Actions Runner Controller (ARC) scale set: self-hosted Linux runners across all workflows, a custom Ubuntu runner image (`ghcr.io/<owner>/atc-runner`) that pre-bakes apt deps, amd64-only cross-compilation for the `aarch64-unknown-linux-musl` release binary, and a thin `Dockerfile.release` for cross-arch container builds. The K8s CronJob `gha-arc-personal/image-updater` (defined in `.github/runner/k8s/image-updater.yaml`) keeps the runner image's `:latest` tag rolling weekly.

Before flipping the repo to public visibility, every one of those private-only choices must be reverted. GitHub's own guidance is explicit:

> We recommend that you only use self-hosted runners with private repositories. This is because forks of your public repository can potentially run dangerous code on your self-hosted runner machine.

Codex flagged this in PR #73's review (P1, three instances). The mitigation is to flip everything back to GitHub-hosted runners *before* accepting any fork PR. Reverting in a single PR limits the risk window: the moment the repo is public, only GitHub-hosted runners should be reachable.

**Verified ground state (2026-05-17):**

- **21 hardcoded** `runs-on: [self-hosted, linux, amd64]` entries (10 in `ci.yml`, 6 in `release.yml`, 3 in `release-please.yml`, 1 in `zizmor.yml`, 1 in `runner-image.yml`). Plus `release.yml build-binaries` uses `${{ matrix.runner }}` where 2 of 3 matrix entries also resolve to self-hosted. **The issue body's "15 of 19" is wrong; corrected counts enumerated below per file.**
- Backend/Frontend jobs in `ci.yml` have *no* `Install build dependencies` / `Install system libraries` steps — all build deps (mold, cmake, perl, pkg-config, libatomic1, Playwright chromium libs) are pre-baked into the custom runner image. The undo work is "add fresh," not "revert removed."
- `jlumbroso/free-disk-space` is *not currently present* in any workflow; it was removed when the Backend job moved to self-hosted. Must be re-added.
- `aarch64-unknown-linux-musl` is cross-compiled on amd64 via `taiki-e/setup-cross-toolchain-action` (not the manual `CARGO_TARGET_*_LINKER` + `gcc-aarch64-linux-gnu` pattern the issue mentions — the architecture doc explicitly records why that pattern failed: `aws-lc-sys` emits glibc-internal symbols incompatible with musl).
- `Dockerfile.release` (21 lines, distroless+COPY+ENTRYPOINT) and `Dockerfile.release.dockerignore` exist at the repo root.
- The shared-container-per-test-DB infrastructure is fully implemented in `backend/crates/atc-server/tests/integration/common/mod.rs:414-519` with explicit retry loops for the testcontainers reuse race, but **is not yet documented in `docs/architecture/ci-pipeline.md`** — a Decision entry is overdue.

## Definition of Done

1. Every workflow `runs-on:` resolves to a GitHub-hosted runner pinned to a specific OS version (`ubuntu-24.04`, `ubuntu-24.04-arm`, `macos-26`) — no `[self-hosted, linux, amd64]` anywhere, no `*-latest`.
2. CI Backend, Frontend, and release-pipeline jobs install the apt deps and disk-reclamation steps that GitHub-hosted runners need but the self-hosted image had pre-baked.
3. `aarch64-unknown-linux-musl` release binary builds natively on `ubuntu-24.04-arm`; `linux/arm64` container builds natively against the regular multi-stage `Dockerfile`. **No GHA cache** — the existing no-cache decision is preserved.
4. `.github/runner/`, `.github/workflows/runner-image.yml`, `Dockerfile.release`, `Dockerfile.release.dockerignore`, and `.github/actionlint.yaml`'s `amd64` allowlist are deleted.
5. `scripts/doc-mapping.sh` no longer routes anything to a removed path.
6. `docs/architecture/ci-pipeline.md` and `docs/architecture/release-pipeline.md` are updated so every Decision entry reflects the post-public-flip state; superseded private-phase rationale moves to a "Historical" sub-entry or is dropped.
7. The shared-container test infrastructure (testcontainers `reusable-containers` feature, retry loops, `just cleanup-test-pg`) is retained and gains a Decision entry in `docs/architecture/ci-pipeline.md`.
8. The `gha-arc-personal/image-updater` K8s CronJob is deleted live (`kubectl delete cronjob image-updater -n gha-arc-personal`) — recorded in the PR body.
9. The undo PR's CI run shows the Backend job lands ≤ 11:09 (the pre-PR-#73 baseline). If not, deviation is documented in the PR body.
10. A draft pre-release tag (e.g., `vX.Y.Z-rc1`) successfully exercises the full `release.yml` pipeline end-to-end after the undo lands.

## Locked Decisions

| Decision | Choice |
|---|---|
| Shared-container-per-test-DB test setup | **Keep** — and add a Decision entry to `ci-pipeline.md` since it's currently undocumented |
| `.github/runner/` infrastructure | **Delete entirely** (incl. K8s CronJob decommission via `kubectl delete`) |
| `Dockerfile.release` + `Dockerfile.release.dockerignore` | **Delete** |
| Validation approach | Time the Backend job vs. ≤ 11:09 baseline + cut a draft pre-release tag |
| Release pipeline GHA cache posture | **Preserve current no-cache decision** (`release-pipeline.md:66`). zizmor flags caching in artifact-pushing workflows. No `cache-from`/`cache-to` restoration. |
| K8s ref audit before CronJob delete | **Skip** — no other repos point at `gha-arc-atc` |

The fork-PR safety spot-check happens out-of-band post public-flip — it is not a gate on this PR.

## Architecture

### Workflow runner labels: `ubuntu-24.04` (amd64) and `ubuntu-24.04-arm` (native arm64)

Native arm64 via `ubuntu-24.04-arm` replaces the entire cross-compile pattern. GitHub introduced public arm64 Linux runners for free OSS minutes in 2024; `ubuntu-24.04-arm` is the pinned tag. This eliminates the `taiki-e/setup-cross-toolchain-action` extraction, the artifact-upload-download dance between `build-binaries` and `build-container`, and `Dockerfile.release` itself. The regular multi-stage `Dockerfile` runs unchanged on each native architecture.

Rejected: using QEMU emulation under `docker buildx` on amd64 only. QEMU compile speed is ~5× slower than native; native arm64 is the right primitive once it's available.

### `Install build dependencies` step shape (Backend job)

```yaml
- name: Install build dependencies
  run: |
    sudo apt-get update
    sudo apt-get install -y --no-install-recommends mold cmake pkg-config perl
```

`mold` is required (the architecture doc records reproducible `SIGBUS` linker crashes with `lld` on large test binaries). `cmake` is required by `aws-lc-sys`. `pkg-config` and `perl` are required by various sys crates. `cmake` and `perl` are already on the github-hosted ubuntu-24.04 default install — the apt-install is a portability no-op in that case but cheap insurance. `mold` is the only strictly mandatory new package.

### `Free up runner disk` step (Backend job)

Re-added at the top of the Backend job, pinned to SHA `54081f138730dfa15788a46383842cd2f914a1be` (the pre-PR-#73 pin; verified extant upstream). ci-pipeline.md's Historical note records that without it, the coverage build fails with "No space left on device" on github-hosted runners due to ~10–15 GB of pre-installed software (Android SDK, .NET, Haskell, Docker images). Renovate's `helpers:pinGitHubActionDigests` will refresh the pin subsequently.

### Frontend job system libraries

`pnpm exec playwright install --with-deps chromium` is the canonical Playwright-blessed way to install chromium runtime libs on github-hosted ubuntu-24.04 with sudo NOPASSWD. The Frontend job already invokes Playwright; the `--with-deps` flag adds the apt step inline. **Preferred over a separate `Install system libraries` step**: keeps Playwright's dep list canonical (no manual list to drift).

### Release pipeline: native arm64 restoration

In `release.yml`:

- `build-binaries.aarch64-unknown-linux-musl` matrix entry: `runner: ubuntu-24.04-arm`. Drop `taiki-e/setup-cross-toolchain-action` step entirely. Drop the `actions/upload-artifact` step that fed `build-container`. `taiki-e/upload-rust-binary-action` continues to compile natively.
- `build-binaries.x86_64-unknown-linux-musl` matrix entry: `runner: ubuntu-24.04` (was self-hosted). Both linux matrix entries get an `Install musl toolchain` step: `sudo apt-get install -y --no-install-recommends musl-tools musl-dev` (gated on `contains(matrix.target, 'musl')`). `musl-tools` provides the `musl-gcc` wrapper, `musl-dev` provides the C library development headers that `aws-lc-sys` and other sys-deps consume. `taiki-e/upload-rust-binary-action` auto-installs the cargo target; no explicit `rustup target add` step needed.
- `build-container.linux/arm64` matrix entry: `runner: ubuntu-24.04-arm`. Drop `needs: build-binaries`, drop `actions/download-artifact`, drop the chmod step. Restore `file: Dockerfile` (the regular multi-stage one). Restore `build-args: RUST_VERSION` + `NODE_VERSION` — but only after adding a versions-read step that populates them from `.mise.toml`. **Do NOT restore GHA cache** (decision preserved; see Locked Decisions).
- `build-frontend`: add `Install Node runtime libraries` step (`sudo apt-get install -y --no-install-recommends libatomic1`). The runner-image rationale (`.github/runner/Dockerfile` line 38) records that Node 25 binary linkage needs `libatomic1`, and `libatomic1` is not in the github-hosted ubuntu-24.04 default inventory. If the executor empirically confirms the dep is present at implementation time, the step can be dropped — but keep it as the default.

### Architecture doc edits

`docs/architecture/ci-pipeline.md`:

- Replace the "Linux jobs on self-hosted ARC runners" Decision with a "Linux jobs on GitHub-hosted runners" Decision. Preserve the prior text under a `**Historical:**` sub-block recording that the homelab pool ran for the private-dev phase. This is the only way to keep the rationale searchable without lying about current state.
- Delete the "Self-hosted ARC runner image" Decision outright — there is nothing operational left to document. A single line in the Historical sub-block above can reference it.
- Update the "mold linker" Decision: rationale shifts from "pre-baked in runner image, runtime apt is portability safety net" to "installed via `Install build dependencies` step on github-hosted ubuntu-24.04."
- Update the "Backend job target/ footprint reduction" Decision: drop the "moved off github-hosted" historical paragraph; re-promote the `jlumbroso/free-disk-space` rationale to current-state.
- **Add a new Decision** for "Shared Postgres container across backend tests" describing the testcontainers `reusable-containers` feature, the retry-on-409 loop, the per-test `CREATE DATABASE test_<pid>_<nanos>_<counter>` provisioning, and `just cleanup-test-pg`. Cross-reference `backend/crates/atc-server/tests/integration/common/mod.rs`.

`docs/architecture/release-pipeline.md`:

- Replace the "arm64 Linux artifacts cross-compiled" Decision with a "Native arm64 Linux artifacts via `ubuntu-24.04-arm`" Decision. The bulk of the existing rationale (the `aws-lc-sys` glibc/musl conflict, the `Dockerfile.release` trick) is irrelevant once native runners are back; move it into a `**Historical:**` sub-block.
- Update the "Native multi-architecture builds" Decision if its current state assumes the cross-compile pattern — likely just a sentence flip back to "each platform on its native runner."

### Doc-mapping cleanup

`scripts/doc-mapping.sh`:

```bash
.github/workflows/*|.github/runner/*|.github/runner/k8s/*|.github/actionlint.yaml)
    echo "docs/architecture/ci-pipeline.md"
```

After: drop `.github/runner/*` and `.github/runner/k8s/*` from the match list:

```bash
.github/workflows/*|.github/actionlint.yaml)
    echo "docs/architecture/ci-pipeline.md"
```

Also remove `Dockerfile.release|Dockerfile.release.dockerignore` from the line that maps Dockerfile-family files to `release-pipeline.md`.

## Implementation Steps

Topic-titled, in execution order. Single PR end-to-end; commits MAY be split per topic for review clarity but the PR squash-merges as one commit (see CONTRIBUTING.md § PR Conventions).

### Branch and plan-file commit

- Create branch `chore/public-flip-undo-list` off main.
- Land this design plan at `docs/design-plans/2026-05-17-public-flip-undo-list.md`.
- Commit `docs: add design plan for public-flip undo list (#74)`.
- **PR title note:** the squash-merged PR title should describe the full deliverable (e.g., `chore: revert private-dev optimizations for public flip (closes #74)`), not the design-doc commit. Per `CONTRIBUTING.md § Pull Requests`, the PR body becomes the squash commit body.

### ci.yml: flip all `runs-on:` + add the apt and disk steps

- Replace all 10 `runs-on: [self-hosted, linux, amd64]` lines with `runs-on: ubuntu-24.04`. Locations: `changes`, `backend`, `frontend`, `helm-lint`, `helm-validate`, `helm-install`, `pr-checks`, `backend-result`, `frontend-result`, `helm-result`.
- Add `Free up runner disk` step at the top of the `backend` job: `jlumbroso/free-disk-space@54081f138730dfa15788a46383842cd2f914a1be`.
- Add `Install build dependencies` step (mold cmake pkg-config perl) to the `backend` job, before any cargo step.
- In the `frontend` job, change the existing `playwright install` invocation to `playwright install --with-deps chromium` so chromium system libs are installed alongside the browser. No separate apt step needed.

`gh` CLI is pre-installed on github-hosted ubuntu-24.04 (per `actions/runner-images` tool manifest, 2026). The release-please workflow's `gh api` calls work out of the box; no install step needed.

### release.yml: native arm64 + restore regular Dockerfile

- Flip all standalone `runs-on:` to github-hosted: `create-release` → `ubuntu-24.04`, `build-frontend` → `ubuntu-24.04`, `merge-manifest` → `ubuntu-24.04`, `publish-helm-chart` → `ubuntu-24.04`, `publish-helm-pages` → `ubuntu-24.04`. `build-container` uses `runs-on: ${{ matrix.runner }}` (see matrix below).
- `build-binaries` matrix: `x86_64-unknown-linux-musl` → `runner: ubuntu-24.04`; `aarch64-unknown-linux-musl` → `runner: ubuntu-24.04-arm`. Both linux entries get a single `Install musl toolchain` step gated on `contains(matrix.target, 'musl')` that runs `sudo apt-get install -y --no-install-recommends musl-tools musl-dev`.
- Drop `taiki-e/setup-cross-toolchain-action` step.
- Drop the `actions/upload-artifact` step at the end of `build-binaries` (was only there to feed `build-container`).
- `build-container` matrix entries: `linux/amd64` → `runner: ubuntu-24.04`, `linux/arm64` → `runner: ubuntu-24.04-arm`. Drop `needs: build-binaries` entirely (no replacement; pre-PR-#73 ran with no `needs:` clause). Drop the `if: ${{ !cancelled() }}` guard (was only needed for matrix-failure tolerance). Drop `actions/download-artifact` and chmod steps. In the `docker/build-push-action` invocation: change `file: Dockerfile.release` back to `file: Dockerfile`; re-add `build-args: RUST_VERSION=... NODE_VERSION=...`. **Do NOT restore `cache-from`/`cache-to`** — the current no-cache decision at `release-pipeline.md:66` is preserved.
- **Re-add a versions-read step** to `build-container` that reads `RUST_VERSION` and `NODE_VERSION` from `.mise.toml` and exports them for the build-args. Without this step, the build-args restoration is moot — the regular `Dockerfile` requires both vars and its defaults are stale relative to `.mise.toml`.
- `build-frontend`: re-add `Install Node runtime libraries` step (`sudo apt-get install -y --no-install-recommends libatomic1`).

### release-please.yml + zizmor.yml: flip `runs-on:`

- `release-please.yml`: 3 jobs (`release-please`, `refresh-lockfile`, `sync-helm-app-version`) → `ubuntu-24.04`.
- `zizmor.yml`: 1 job → `ubuntu-24.04`.

### actionlint.yaml: drop the `amd64` allowlist

Delete `.github/actionlint.yaml` if `self-hosted-runner.labels` becomes empty (the only entry is `amd64`).

### Delete runner-image infrastructure

- `git rm -rf .github/runner/` (Dockerfile, CLAUDE.md, AGENTS.md symlink, .dockerignore, k8s/image-updater.yaml).
- `git rm .github/workflows/runner-image.yml`.
- `git rm Dockerfile.release Dockerfile.release.dockerignore`.

### Update `scripts/doc-mapping.sh`

- Remove `.github/runner/*|.github/runner/k8s/*` from the workflow-mapping case branch.
- Remove `Dockerfile.release|Dockerfile.release.dockerignore` from the Dockerfile-family case branch.

### Update architecture docs

- `docs/architecture/ci-pipeline.md`:
  - Replace "Linux jobs on self-hosted ARC runners" Decision with a current-state "GitHub-hosted runners" Decision; the old rationale moves under `**Historical (private-dev phase, 2026-04 to 2026-05):**`.
  - Delete the "Self-hosted ARC runner image" Decision; collapse into the Historical sub-block.
  - Update the "mold linker" Decision rationale to reflect runtime apt-install rather than pre-baked.
  - Update the "Backend job target/ footprint reduction" Decision: promote `free-disk-space` back to current-state rationale.
  - Add a new "Shared Postgres container across backend tests" Decision. Include: testcontainers `reusable-containers` feature pin, retry-on-409 race in container start (the testcontainers reuse logic's inspect-then-create is not atomic), per-test `CREATE DATABASE test_<pid>_<nanos>_<counter>` provisioning, **failure mode if `just cleanup-test-pg` is not run**: stale `test_*` databases accumulate at ~10 MB each over heavy test sessions; the `cleanup-test-pg` recipe is the cleanup primitive but not automatic.
- `docs/architecture/release-pipeline.md`:
  - Replace "arm64 Linux artifacts cross-compiled" Decision with "Native arm64 via `ubuntu-24.04-arm`"; preserve the cross-compile rationale under `**Historical:**`.
  - Audit "Native multi-architecture builds" Decision and reword if it has private-phase artifacts.
  - Audit "Frontend built once and shared" Decision and drop any references to `Dockerfile.release` from its rationale text.
  - **Preserve** the "No GitHub Actions cache on release pipeline" Decision — this is intentionally retained (zizmor-compliance with artifact-pushing workflows). If the existing rationale text references self-hosted runners or `Dockerfile.release`, update the wording to current state without flipping the Decision.
  - Update "Boundaries" section: drop any `Dockerfile.release*` references.
  - Update "Files" section: drop the `Dockerfile.release` and `Dockerfile.release.dockerignore` entries.

### Validate via CI, a pre-merge release-pipeline exercise, and a post-merge draft pre-release

- Push the branch; open the PR. Capture the Backend job's elapsed time from the CI run.
- If ≤ 11:09: note in PR body "Backend job: M:SS, ≤ 11:09 baseline ✓".
- If > 11:09: document the actual time and the likely cause (e.g., free-disk-space overhead, network variability, runner pool busy).
- **Pre-merge release-pipeline exercise.** Before merging the PR, trigger a dry-run via a temporary `vX.Y.Z-rc1` tag pushed to the PR branch. Confirm: both `build-binaries` matrix entries succeed natively on `ubuntu-24.04` / `ubuntu-24.04-arm`; both `build-container` matrix entries succeed against `Dockerfile` with the regenerated `RUST_VERSION`/`NODE_VERSION` build-args; cold builds without GHA cache complete within an acceptable window (no hard SLA — just verify they don't hit the 6-hour job timeout). Delete the dry-run tag/release after.
- After the PR merges, cut the real draft pre-release tag (`vX.Y.Z-rc1` per release-please conventions) to confirm end-to-end against `main` once more. Confirm linux/amd64 + linux/arm64 binaries publish and both container manifests push.

### K8s decommission (out-of-band, post-merge)

After the PR merges:

```bash
kubectl delete cronjob image-updater -n gha-arc-personal
```

Record completion in the PR body or a follow-up comment. The repo-side deletion of `.github/runner/k8s/image-updater.yaml` does not affect the live cluster — manifest is not Argo/Flux-managed; it was applied directly.

## Acceptance Criteria

- **AC1** — `git grep -F 'self-hosted, linux, amd64' -- .github/workflows/` returns zero hits. Scope is restricted to `.github/workflows/` because `.github/actionlint.yaml`'s `amd64` allowlist removal is independently checked in AC5, and `.github/runner/Dockerfile` has been deleted entirely (so cannot contain the string by AC5).
- **AC2** — `.github/workflows/ci.yml` Backend job contains both an `Install build dependencies` step (matching `mold` apt-install) and a `Free up runner disk` step using `jlumbroso/free-disk-space`. Frontend job's playwright invocation includes `--with-deps`.
- **AC3** — `.github/workflows/release.yml` `aarch64-unknown-linux-musl` matrix entry resolves to `runner: ubuntu-24.04-arm` and contains no `taiki-e/setup-cross-toolchain-action` or `actions/upload-artifact` step.
- **AC4** — `release.yml build-container` `linux/arm64` matrix entry resolves to `runner: ubuntu-24.04-arm`, omits `needs: build-binaries`, references `file: Dockerfile` (not `Dockerfile.release`), and has `build-args: RUST_VERSION=... NODE_VERSION=...` populated from a preceding versions-read step. **No `cache-from` / `cache-to` directives** — the no-cache decision is preserved. `grep -F 'cache-from: type=gha' .github/workflows/release.yml` returns zero hits in `build-container`.
- **AC5** — `.github/runner/`, `.github/workflows/runner-image.yml`, `Dockerfile.release`, `Dockerfile.release.dockerignore` do not exist (`git ls-files` reports absent). `.github/actionlint.yaml` either has no `amd64` entry or is deleted entirely.
- **AC6** — `grep '\.github/runner' scripts/doc-mapping.sh` returns zero hits; `grep 'Dockerfile.release' scripts/doc-mapping.sh` returns zero hits.
- **AC7** — `docs/architecture/ci-pipeline.md` has a current-state "GitHub-hosted runners" Decision and no "self-hosted ARC runners" Decision in its current-state position (must be under a Historical sub-block). `docs/architecture/release-pipeline.md` has a "Native arm64 via `ubuntu-24.04-arm`" Decision; the cross-compile rationale survives only under a Historical sub-block.
- **AC8** — `backend/crates/atc-server/Cargo.toml` retains `testcontainers = { version = "=0.27.3", features = ["reusable-containers"] }`. The `justfile` still has the `cleanup-test-pg` recipe. `docs/architecture/ci-pipeline.md` has a new "Shared Postgres container" Decision entry. `backend/crates/atc-server/tests/integration/common/mod.rs` `start_pg()` is unchanged.
- **AC9** — CI on the PR head passes (lint + check + test + build). The Backend job's elapsed time is recorded in the PR body, with deviation from 11:09 explained if positive.
- **AC10** — **Pre-merge:** `release.yml` runs end-to-end against the PR branch (via a temporary rc tag pushed to the branch); both `build-binaries` matrix entries and both `build-container` matrix entries succeed. **Post-merge:** the same exercise repeats against `main` via a real `vX.Y.Z-rc1` tag, producing amd64 + arm64 musl binaries and multi-arch container manifest.
- **AC11** — `kubectl get cronjob image-updater -n gha-arc-personal` returns "not found" after the PR merges. Confirmation recorded in PR body or follow-up.

## Documents to Update

| Doc | Change |
|---|---|
| `docs/architecture/ci-pipeline.md` | Five Decision edits: "self-hosted ARC runners" → "GitHub-hosted"; delete "Self-hosted ARC runner image"; update "mold linker" rationale; update "Backend job target/" historical-vs-current framing; add new "Shared Postgres container" Decision. |
| `docs/architecture/release-pipeline.md` | (1) Replace "arm64 Linux artifacts cross-compiled" Decision with "Native arm64 via `ubuntu-24.04-arm`". (2) Audit "Native multi-architecture builds" Decision. (3) Audit "Frontend built once and shared" Decision — drop `Dockerfile.release` references. (4) **Preserve** "No GitHub Actions cache on release pipeline" Decision — only update rationale wording if it references self-hosted runners or `Dockerfile.release`. (5) Update Boundaries — drop `Dockerfile.release*`. (6) Update Files — drop `Dockerfile.release` + `.dockerignore` entries. |
| `scripts/doc-mapping.sh` | Remove `.github/runner/*\|.github/runner/k8s/*` from the workflow-family case branch. Remove `Dockerfile.release\|Dockerfile.release.dockerignore` from the Dockerfile-family case branch. |
| `CLAUDE.md` (root) | No change — references `docs/architecture/` generically; no runner-specific content. |
| `.github/CLAUDE.md` | Audit. If it referenced `.github/runner/`, drop. |
| `backend/CLAUDE.md` / domain `CLAUDE.md`s | No change — none currently reference the runner image. |

## Out of Scope

- **Maintain a self-hosted runner for a specific workflow (e.g., perf-tests).** Delete entirely — if a future need arises, it's a separate issue.
- **Fork-PR safety spot-check.** Happens out-of-band post public-flip, not gated by this PR.
- **Release-pipeline cross-compile fallback playbook.** Tracked in issue #75.
- **Renovate-style automated updates to the now-deleted `.github/runner/Dockerfile`.** No follow-up needed since the file is gone; any Renovate rules scoped to that path will become no-ops.

## Glossary

- **ARC** — Actions Runner Controller; the Kubernetes operator that runs self-hosted GitHub Actions runners as pods.
- **`gha-arc-personal`** — The K8s namespace hosting the homelab ARC scale set.
- **`gha-arc-atc`** — The AutoscalingRunnerSet inside `gha-arc-personal` that serves this repo's `[self-hosted, linux, amd64]` label.
- **`Dockerfile.release`** — The slim release-only Dockerfile used during cross-compile (FROM distroless + COPY pre-built binary). Deleted in this PR.
- **`ubuntu-24.04-arm`** — GitHub-hosted arm64 Linux runner tag, pinned.
- **"Pre-PR-#73 baseline (11:09)"** — The Backend job's elapsed time on the last main-branch CI run before PR #73 landed. Source: PR #73's body, pre-merge run dated 2026-05-08.
