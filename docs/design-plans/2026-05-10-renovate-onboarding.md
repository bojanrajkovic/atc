# Renovate Onboarding + Pre-Flight Cleanups

## Context

The repository has been onboarded to Mend Renovate. PR #2 (`renovate/configure`) is open with a stub `renovate.json` containing only `extends: ["config:recommended"]`. Merging that as-is would auto-create ~51 dependency-update PRs against a configuration that does not match this project's policy: it would not pin SHA digests on Docker images, would not preserve the existing GitHub Actions SHA-pinning convention, would auto-merge without operational gating, and would skip several manager-specific concerns (pnpm catalog, mise quoted-key cargo tools, ts-rs cross-domain coupling, Cargo workspace dependencies).

Several artifacts adjacent to Renovate setup also need cleanup before the config can land cleanly:

- The pnpm catalog has stale `@radix-ui/*` entries (4 packages, no usage in `frontend/src/` or `frontend/e2e/` — confirmed by grep). Renovate already flagged a lookup failure on `@radix-ui/dialog` in the onboarding PR.
- TypeScript appears in two declaration paths with conflicting versions: catalog says `^6.0.2` (TypeScript 6.0.2 shipped March 23, 2026), `frontend/package.json` direct dep says `^5.9.3`. The lockfile resolves to 5.9.3 — the direct dep wins and the catalog entry is dead. The repo's source code, tooling, and tests all run against 5.9.3 today; a v6 migration is a separate decision (`tsconfig.json`, `tsc`-driven CI, and Svelte 5's TypeScript integration may need work). This PR consolidates the catalog to `5.9.3`; v6 migration is out of scope.
- Several deps already in the catalog (`@testing-library/svelte`, `jsdom`, `tailwindcss`, others) plus migration-eligible deps (`msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage`) are referenced via direct version ranges in `frontend/package.json` instead of `"catalog:"`. The author confirmed in this session that the divergence was unintentional and to consolidate.
- `.mise.toml:22` has `cargo-binstall = "latest"` which Renovate cannot manage (no concrete version to compare against).
- `compose.otel-dev.yaml:20` has `grafana/otel-lgtm:latest` (moving target Renovate can't track for version bumps).
- Two workflow files have inline trailing comments `# zizmor: ignore[superfluous-actions]` on `uses:` lines for `dtolnay/rust-toolchain` (`.github/workflows/ci.yml:77`, `.github/workflows/release-please.yml:86`). Renovate's documented behavior preserves `# vX.Y.Z` and `# ratchet:*` comments but is undocumented for arbitrary trailing comments — a real risk of clobbering on the next bump. We address this by adding file-level rules in `.zizmor.yml` as a Renovate-clobber fallback: the inline comment continues to suppress today, and the config-file rule keeps the audit silenced even if Renovate ever rewrites the SHA-pin comment in a way that strips it. (`.github/workflows/ci.yml:425` carries a `zizmor: ignore[secrets-outside-env]` comment on an `env:` line, which Renovate doesn't manage; no edit needed there.)
- The repository ruleset on `main` ("Protect Main", id 14343335) currently has zero required status checks. With `platformAutomerge: false` and Renovate's default `ignoreTests: false`, Renovate's own automerge loop already waits for green checks before merging — so the pre-ruleset gap is not "the bot blindly merges red PRs." The real gap is that *manual* merges from the GitHub UI (the author, or anyone with bypass) can complete with CI red. Adding required status checks is necessary for that human-side enforcement and consistent merge gating regardless of who's merging.
- The Mend Renovate GitHub App (App ID 2740, verified via `gh api /apps/renovate`) is not currently a bypass actor on the ruleset. This is not strictly blocking today (no CODEOWNERS file, 0 required approvals), but should be added while the ruleset is being modified to avoid surprises if approvals or code-owner review are tightened later.

The author chose to bundle all of this into a single PR on the `renovate/configure` branch.

## Definition of Done

The `renovate/configure` branch contains:

1. A `renovate.json` that:
   - Pins manifests exactly (Cargo `=X.Y.Z`, npm/pnpm strip caret), digest-pins Docker FROMs and GitHub Actions
   - Auto-merges minor + patch updates after a 3-day release-age delay; opens but does not auto-merge major updates
   - Uses Renovate's built-in automerge (not GitHub platform automerge), so `automergeSchedule` is honored
   - Routes runtime dep bumps as `fix(deps):` (release-please ships a patch) and dev/tooling bumps as `chore(deps):` (no release)
   - Treats security advisories (OSV + Dependabot) with bypassed schedule and zero release-age delay; non-major security PRs auto-merge, major security PRs require manual review
   - Groups monorepos that lack auto-detect coverage (Svelte ecosystem, tokio + tokio-util, tower + tower-http) and cross-manager pnpm/rust-toolchain pins
   - Special-cases `ts-rs` (no automerge, type-bridge with frontend), `sqlx` family (no automerge, compile-time SQL checking), opentelemetry-rust (no automerge, breaking 0.x minor bumps)
   - The four stale `@radix-ui/*` entries are removed from the catalog in the same PR (no defensive packageRule needed — the entries disappear with the catalog edit)
2. The cleanup edits, all on the same branch:
   - `frontend/pnpm-workspace.yaml`: remove four `@radix-ui/*` entries; replace dead `typescript: ^6.0.2` with `typescript: 5.9.3`; add catalog entries for `msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage` at versions matching what the lockfile currently resolves
   - `frontend/package.json`: replace direct version ranges with `"catalog:"` for the deps already in or being added to the catalog
   - `frontend/pnpm-lock.yaml`: regenerated via `pnpm install --no-frozen-lockfile`
   - `.mise.toml`: pin `cargo-binstall` to a concrete version (current latest at implementation time)
   - `compose.otel-dev.yaml`: pin `grafana/otel-lgtm` to a concrete tag
   - `.zizmor.yml`: add file-level `superfluous-actions` ignore rules for `ci.yml` and `release-please.yml` as a Renovate-clobber fallback for the inline `# zizmor: ignore[superfluous-actions]` comments on `dtolnay/rust-toolchain` lines (the inline comments remain on the `uses:` lines unchanged)
3. The PR description documents two GitHub-side actions the author runs after merging (these are API calls, not file edits):
   - PUT the ruleset to add four required status check contexts (`Backend Result`, `Frontend Result`, `Helm Result`, `PR Checks`)
   - PUT the ruleset to add the Mend Renovate App (App ID 2740) as an `Integration` bypass actor with `bypass_mode: "pull_request"`
4. CI passes on the branch.

**Explicit exclusion:** The ruleset PUT operations are NOT performed as part of the PR (rulesets are GitHub configuration, not file-based). They are documented in the PR description as a runbook for after-merge.

## Locked Decisions

- **Pinning style: exact, manifests + Docker digests.** Cargo manifests target `=X.Y.Z`, npm/pnpm strip caret prefixes, Docker FROMs (concrete tags) get appended `@sha256:...`. **Caveat on Cargo:** Renovate's cargo versioning module supports `getPinnedValue` returning `=X.Y.Z` even though `supportedRangeStrategies` still lists only `['bump', 'replace']` at the time of writing. Treat exact Cargo pinning as an implementation-time verification on the first cargo update — if it doesn't land as `=X.Y.Z` in `Cargo.toml`, fall back to `rangeStrategy: "bump"` for the cargo manager via a `matchManagers: ["cargo"]` rule.
- **Auto-merge gating: required status checks on the ruleset + 3-day release-age delay.**
- **Commit type: Renovate's default `:semanticPrefixFixDepsChoreOthers`.** Runtime deps → `fix(deps):` (release-please ships a patch), dev deps → `chore(deps):` (no release).
- **`platformAutomerge: false`.** Use Renovate's built-in automerge polling, not GitHub's "Enable auto-merge" feature. Makes the schedule window meaningful at the cost of ~30–60 min latency between green CI and merge.
- **Catalog consolidation in this PR.** The catalog/direct-version divergence was unintentional; this PR bundles the cleanup with Renovate config.
- **TypeScript reconciles to 5.9.3 (catalog), replacing the dead `^6.0.2` entry.** The lockfile resolves to 5.9.3, all repo code compiles against 5.9.3, and CI runs against 5.9.3. TypeScript 6.0.2 shipped March 23, 2026, but the v6 migration is its own initiative (tsconfig shifts, Svelte's TS integration, dependent type stubs); out of scope for this PR.
- **Mend Renovate App ID is 2740** (verified via `gh api /apps/renovate`). For ruleset bypass actors, `actor_type: "Integration"` takes the App ID, not the per-installation ID.
- **Required status check contexts:** `Backend Result`, `Frontend Result`, `Helm Result`, `PR Checks` — the four jobs that have `if: always()` aggregator semantics or run unconditionally on PRs. Verified against `.github/workflows/ci.yml` (lines 404, 428, 450, 472). The path-conditional children (`Backend`, `Frontend`, `Helm Lint & Unit`, `Helm Validate`, `Helm Install (kind + ct)`) are explicitly NOT marked required — they skip on path-filtered PRs and would block all merges when the relevant path isn't touched.

## Architecture

### `renovate.json` — top-level shape

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": [
    "config:best-practices"
  ],
  "timezone": "America/New_York",
  "schedule": ["before 9am every weekday"],
  "automergeSchedule": ["before 9am every weekday"],
  "prHourlyLimit": 3,
  "prConcurrentLimit": 6,
  "branchConcurrentLimit": 0,
  "dependencyDashboard": true,
  "assignees": ["bojanrajkovic"],
  "semanticCommits": "enabled",
  "rangeStrategy": "pin",
  "automerge": false,
  "platformAutomerge": false,
  "automergeStrategy": "squash",
  "minimumReleaseAge": "3 days",
  "osvVulnerabilityAlerts": true,
  "vulnerabilityAlerts": {
    "enabled": true,
    "schedule": ["at any time"],
    "labels": ["security"]
  },
  "lockFileMaintenance": {
    "enabled": true,
    "automerge": true,
    "schedule": ["before 5am every sunday"]
  },
  "packageRules": [/* see below */]
}
```

Non-default-choice rationale (Renovate doc cites embedded for verification):

- `extends: ["config:best-practices"]` — this preset bundles `config:recommended` plus `docker:pinDigests`, `helpers:pinGitHubActionDigests`, `:pinDevDependencies`, `abandonments:recommended`, `security:minimumReleaseAgeNpm`, and `:maintainLockFilesWeekly`. Our explicit `lockFileMaintenance` override sets the schedule to Sunday 5am (Monday morning triage) and `automerge: true` (mechanical safe regen); these merge with the preset's defaults. `abandonments:recommended` surfaces packages with no release for 12+ months on the dependency dashboard — useful visibility, not noisy on PRs. (https://docs.renovatebot.com/presets-config/)
- `platformAutomerge: false` makes `automergeSchedule` meaningful. (https://docs.renovatebot.com/key-concepts/automerge/)
- `branchConcurrentLimit: 0` — unlimited branches with limited PRs lets Renovate keep work in flight while throttling visible PRs.
- `assignees` only, no `reviewers` — Bojan is the only reviewer; requesting self-review is noise.

### No `customManagers` — native managers cover the surface

Renovate's native managers cover the surface; no `customManagers` are needed. The Dockerfile manager already expands `ARG <NAME>=<value>` defaults used in subsequent `FROM <image>:${NAME}-...` lines. The mise manager supports quoted-key `cargo:*` entries (depName: the full tool key, datasource: `crate`).

A consequence: the `rust-toolchain` cross-manager group below uses `matchManagers: ["mise", "dockerfile"]`. Whether mise's depName (`rust`) and dockerfile's depName (likely `rust` when expanding `FROM rust:...`) match precisely is **unverified** — to be confirmed on the first run after merge by inspecting the Mend dashboard. If they don't match, fall back to two separate Renovate PRs (acceptable: with `automerge: false` on the group, each is reviewed manually anyway).

### `packageRules` (ordered — later rules override earlier for matching deps)

1. **Major (any manager): no automerge, label `update:major`.**
2. **Minor + patch (any manager): automerge.** No `matchCurrentVersion: ">=1.0.0"` filter — Renovate's `isBreaking` already classifies pre-1.0 minor bumps as `updateType: major` for cargo (verified against `lib/modules/versioning/cargo/index.ts`'s `isBreaking()` by rust-research), which falls into rule 1.
3. **Pre-1.0 minor explicit override (cargo + npm): no automerge.** Belt-and-braces — npm classification of 0.x minor as breaking is less certain than cargo's.
4. **Pin / digest updates: automerge.** (no release age — pin and digest updates are mechanical.)
5. **devDependencies minor + patch: automerge.** Includes pnpm catalog devDeps via `matchDepTypes: ["devDependencies", "pnpm.catalog.default"]`.
6. **Cargo specifics:**
   - `sqlx`, `sqlx-core`, `sqlx-macros`: no automerge (compile-time SQL checking; minor bumps may shift macro behavior).
   - `axum`, `axum-core`, `axum-extra`, `axum-macros` patch: automerge (already covered by rule 2; explicit for axum's high-stakes changelog history).
   - `ts-rs` any update: no automerge (type bridge to frontend; even minor/patch could regenerate types). CI's "Verify generated TypeScript types are current" step (`.github/workflows/ci.yml` lines 107–117 — `git diff --exit-code frontend/src/lib/types/generated/`) catches drift, so `automerge: false` is sufficient. `dependencyDashboardApproval` is not needed. **Remediation when CI fails:** Renovate does not run `just types` itself (no Rust toolchain in its env), so a ts-rs bump that changes generated output will land a PR with stale `frontend/src/lib/types/generated/`. The CI failure message instructs the operator to `git pull`, run `just types` locally, commit the regenerated files, and push the result back to the Renovate branch.
   - opentelemetry-rust monorepo (matched by `matchSourceUrls`): no automerge (0.x ecosystem upgrades are coordinated breaking changes).
   - `tokio` + `tokio-util` custom group "tokio ecosystem", patch automerged.
   - `tower` + `tower-http` custom group "tower ecosystem".
7. **Frontend / npm specifics:**
   - Svelte ecosystem custom group (svelte, svelte-check, svelte-eslint-parser, eslint-plugin-svelte, prettier-plugin-svelte, @sveltejs/vite-plugin-svelte, @testing-library/svelte) — automerge minor + patch. No Renovate auto-group preset exists for these. **No forced `commitMessagePrefix`** — Renovate uses `fix(deps):` only when the group's update set includes a runtime member (`svelte`), and `chore(deps):` otherwise. This avoids a release-please patch release for dev-tooling-only updates inside the group.
   - Svelte ecosystem major: no automerge.
   - `tailwind-merge` major: no automerge (separate org from `tailwindcssMonorepo` preset).
   - `typescript`, `vite`, `eslint` major: no automerge.
   - Playwright custom group (`@playwright/test` + `@bgotink/playwright-coverage`): automerge minor + patch.
8. **CI/CD specifics:**
   - github-actions major: no automerge.
   - github-actions minor / patch / digest: automerge.
   - dockerfile + docker-compose minor / patch / digest: automerge with `pinDigests`.
   - dockerfile + docker-compose major: no automerge.
   - mise minor / patch: automerge.
   - mise major: no automerge.
9. **Cross-manager groups:**
   - `"rust-toolchain"` (mise rust pin + native dockerfile ARG default): no automerge. `matchManagers: ["mise", "dockerfile"]`, `matchDepNames: ["rust"]`. Cross-manager depName matching is unverified — to be confirmed on first Renovate run. If depNames don't align, the result is two separate PRs (one mise, one dockerfile); both are manual-merge anyway under this rule. Cargo `rust-version` (MSRV) deliberately not included — separate decision tracked manually.
   - `"pnpm-version"` (mise pnpm pin + npm `packageManager` field on both root and `frontend/package.json`): automerge minor / patch (Renovate regenerates `pnpm-lock.yaml` as part of the PR; the 3-day age gate still applies). Major no automerge — pnpm v10 → v11 warrants manual review with potential `packageManager` field migration.
10. **Security override (last — broadest applicability):**
    - `matchCategories: ["security"]`: automerge with `minimumReleaseAge: null`, `schedule: ["at any time"]`, and `automergeSchedule: ["at any time"]`. Both fields are needed: `schedule` controls branch/PR creation, `automergeSchedule` controls when Renovate's automerge poll will fire. Without the latter, security PRs would still wait for the weekday-morning automerge window to actually merge.
    - `matchCategories: ["security"], matchUpdateTypes: ["major"]`: `automerge: false` (overrides line above for major security PRs only).

### `vulnerabilityAlerts` and `osvVulnerabilityAlerts`

Both Dependabot (GitHub Security Advisory database) and OSV (which aggregates RustSec for cargo) are enabled. The `vulnerabilityAlerts` block sets schedule override and labels; automerge behavior comes from the security `packageRules` above so it can be tiered (major manual, non-major automerged).

### `lockFileMaintenance`

Weekly Sunday morning regen of `Cargo.lock` and `pnpm-lock.yaml`. Catches transitive yanks and security advisories that don't surface as direct-dep updates. With `rangeStrategy: pin`, manifests are stable and `lockFileMaintenance` is the only path for transitive bumps.

`lockFileMaintenance.automerge: true` is set explicitly. Top-level `automerge: false` would otherwise leave lockfile PRs sitting open: they're not classified as `updateType: minor` or `patch` (lockfile maintenance is its own depType), so they wouldn't pick up the global minor/patch automerge rule. Setting `automerge: true` here is consistent with the DoD's "auto-merges minor + patch updates" intent — lockfile maintenance is mechanically safe (transitive bumps gated by the same CI as direct-dep PRs).

### Cleanup edits paired with `renovate.json`

| File | Edit |
|------|------|
| `frontend/pnpm-workspace.yaml` | Remove `@radix-ui/dialog`, `@radix-ui/primitive`, `@radix-ui/react-progress`, `@radix-ui/react-toggle`. Replace the dead `typescript: ^6.0.2` entry with `typescript: 5.9.3` (no caret per `rangeStrategy: pin`). Add catalog entries for `msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage` at versions matching what the lockfile currently resolves. |
| `frontend/package.json` | Replace direct version ranges with `"catalog:"` for: `typescript`, `@testing-library/svelte`, `jsdom`, `msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage`. |
| `frontend/pnpm-lock.yaml` | Regenerate via `pnpm install` after manifest edits. |
| `.mise.toml` | `cargo-binstall = "latest"` → `cargo-binstall = "<concrete version>"` (look up current latest at implementation time via `gh release list --repo cargo-bins/cargo-binstall --limit 1`). |
| `compose.otel-dev.yaml` | `image: grafana/otel-lgtm:latest` → `image: grafana/otel-lgtm:<concrete tag>` (look up current latest via `gh release list --repo grafana/docker-otel-lgtm --limit 1` or Docker Hub). |
| `.zizmor.yml` | Add `rules.superfluous-actions.ignore: [ci.yml, release-please.yml]`. Belt-and-braces against Renovate ever clobbering the inline `# zizmor: ignore[superfluous-actions]` comments on the two `dtolnay/rust-toolchain` `uses:` lines. The inline comments stay in place (unmodified from main) so the audit is still suppressed even before Renovate touches anything. |
| `.github/workflows/ci.yml` helm-install job (around line 350) | Add a "Read Rust + Node versions from `.mise.toml`" step before the `docker build` step — extract `tools.rust` and `tools.node` via `mise config get` (mirroring the existing setup-node / rust-toolchain extraction at ci.yml:71–75 and 169–171). Pass via an `env:` block on the build step (`RUST_VERSION` / `NODE_VERSION`), then reference as `${RUST_VERSION}` / `${NODE_VERSION}` inside the `run:` shell — `docker build --build-arg "RUST_VERSION=${RUST_VERSION}" --build-arg "NODE_VERSION=${NODE_VERSION}"`. The env-var passthrough (rather than direct `${{ ... }}` expansion inside `run:`) keeps zizmor's `template-injection` audit happy. The Dockerfile's ARG defaults remain as fallbacks for standalone `docker build`. |
| `renovate.json` | Replace stub with the full config above. |

`.github/workflows/ci.yml:425` (`GITHUB_TOKEN: ... # zizmor: ignore[secrets-outside-env]`) is on an `env:` line, not a `uses:` line — Renovate doesn't manage it; no edit needed.

The runner Dockerfile (`.github/runner/Dockerfile`) uses a concrete `FROM ghcr.io/actions/actions-runner:2.333.0` with no ARG indirection. Renovate's dockerfile manager handles it natively. No edit needed.

### Ruleset operations (post-merge runbook in PR description)

Ruleset PUT is a full-replacement endpoint. Two safety properties matter:

1. **Narrow the PUT body to documented writable fields only.** `gh api .../rulesets/14343335` returns response-only fields (`id`, `node_id`, `created_at`, `updated_at`, `_links`, `current_user_can_bypass`, `source`, `source_type`) that the PUT endpoint either rejects or silently ignores. Strip them before PUT to avoid `422 Unprocessable Entity` surprises.
2. **Make the jq mutations idempotent.** Append the required-status-checks rule and Renovate bypass actor *only if not already present*. Re-running the runbook (e.g., after a partial failure) must not produce duplicate entries.

```bash
mkdir -p /tmp/atc-ruleset
gh api /repos/bojanrajkovic/atc/rulesets/14343335 > /tmp/atc-ruleset/current.json

# Strip response-only fields; idempotently append the new rule + bypass actor:
jq '
  # Strip response-only fields
  {name, target, enforcement, conditions, rules, bypass_actors}
  |
  # Add required_status_checks rule iff no rule of that type already exists
  (if any(.rules[]; .type == "required_status_checks") then . else
    .rules += [{
      "type": "required_status_checks",
      "parameters": {
        "strict_required_status_checks_policy": false,
        "do_not_enforce_on_create": false,
        "required_status_checks": [
          {"context": "Backend Result"},
          {"context": "Frontend Result"},
          {"context": "Helm Result"},
          {"context": "PR Checks"}
        ]
      }
    }]
  end)
  |
  # Add Renovate Integration bypass iff not already present
  (if any(.bypass_actors[]; .actor_type == "Integration" and .actor_id == 2740) then . else
    .bypass_actors += [{
      "actor_id": 2740,
      "actor_type": "Integration",
      "bypass_mode": "pull_request"
    }]
  end)
' /tmp/atc-ruleset/current.json > /tmp/atc-ruleset/new.json

# Review the diff visually before sending:
diff <(jq -S . /tmp/atc-ruleset/current.json) <(jq -S . /tmp/atc-ruleset/new.json)

# PUT after review:
gh api --method PUT /repos/bojanrajkovic/atc/rulesets/14343335 \
  --input /tmp/atc-ruleset/new.json
```

The author reviews the `diff` output before the PUT to confirm only the two intended additions appear.

## Implementation Phases

1. **Worktree the `renovate/configure` branch.** New worktree at `.claude/worktrees/renovate-configure` so this work is isolated from the issue-80 worktree. Immediately run `just setup` inside the new worktree to install lefthook hooks (git worktrees don't inherit hooks — `CLAUDE.md` invariant).
2. **Determine concrete versions.** Look up current latest for `cargo-binstall` and `grafana/otel-lgtm`. Note them in commit messages.
3. **Apply cleanup edits in dependency order:**
   - Edit `frontend/pnpm-workspace.yaml` (catalog churn).
   - Edit `frontend/package.json` (consolidate to `"catalog:"`).
   - Run `pnpm install --no-frozen-lockfile` from `frontend/` to regenerate `frontend/pnpm-lock.yaml` (`--no-frozen-lockfile` is explicit; pnpm's default behavior in CI/non-interactive environments may differ).
   - Edit `.mise.toml` (`cargo-binstall` pin).
   - Edit `compose.otel-dev.yaml` (`grafana/otel-lgtm` pin).
   - Add `.zizmor.yml` rules for `superfluous-actions` (file-level ignore for `ci.yml` and `release-please.yml`). Inline `# zizmor: ignore[superfluous-actions]` comments on the two `dtolnay/rust-toolchain` `uses:` lines stay unchanged from main.
   - Edit `.github/workflows/ci.yml` helm-install job: insert a step that reads `tools.rust` and `tools.node` from `.mise.toml` via `mise config get`, then thread them through `--build-arg` on the `docker build` command via an `env:` block on the build step (avoids zizmor's `template-injection` audit). Pattern mirrors the existing rust-toolchain / setup-node extraction at ci.yml:71–75 and 169–171.
4. **Write the new `renovate.json`** — replace the 6-line stub with the full config.
5. **Local verification:** Run `just lint`, `just check`, `just test`, `just test-e2e` (per `feedback_run_e2e_tests_for_frontend_changes.md` — `pnpm-lock.yaml` regen counts as a frontend change). Run `mise exec actionlint -- actionlint .github/workflows/*.yml` to lint the workflow edits (mise provides `actionlint = "1.7.12"`; `just lint` does NOT include it). Defer `renovate.json` schema validation to Renovate's first post-merge run (validator install would violate the no-pip/no-npm-install convention; Mend doesn't run a config-validation CI check on `renovate/configure` PRs).
6. **Commit and push.** Conventional commits per `CONTRIBUTING.md`. Suggested per-logical-unit commit messages:
   - `chore(deps): consolidate frontend pnpm catalog and prune stale entries`
   - `chore: pin cargo-binstall and grafana/otel-lgtm to concrete versions`
   - `chore(ci): move zizmor ignore comments off Renovate-managed action lines`
   - `chore: configure Renovate with project-tuned automerge and pinning policy`
7. **Update PR #2** — title rewritten to reflect the full deliverable (e.g., `chore: configure Renovate and consolidate frontend pnpm catalog`); body rewritten as the squash commit body; ruleset runbook in the body; test plan posted as the first PR comment.

CI runs on the PR. Once green, Bojan reviews and merges. After merge: Bojan applies the two ruleset PUTs from the runbook, then Renovate's next scheduled run picks up the new config.

## Acceptance Criteria

AC1. `frontend/pnpm-workspace.yaml` has no `@radix-ui/*` entries; the typescript catalog entry is `5.9.3` (no caret per `rangeStrategy: pin` policy applied retroactively to the cleanup); catalog includes `msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage` at lockfile-resolved versions.

AC2. `frontend/package.json` references `typescript`, `@testing-library/svelte`, `jsdom`, `msw`, `@types/node`, `@playwright/test`, `@bgotink/playwright-coverage` via `"catalog:"` (no direct version ranges).

AC3. `frontend/pnpm-lock.yaml` is regenerated and consistent with the manifest changes; `pnpm install --frozen-lockfile` succeeds from `frontend/`.

AC4. `.mise.toml` has `cargo-binstall = "<concrete version>"` (no `"latest"`).

AC5. `compose.otel-dev.yaml` references `grafana/otel-lgtm:<concrete tag>` (no `:latest`).

AC6. `.zizmor.yml` carries `rules.superfluous-actions.ignore` covering `ci.yml` and `release-please.yml`. The two `dtolnay/rust-toolchain` `uses:` lines retain their inline `# zizmor: ignore[superfluous-actions]` trailing comments (unchanged from main); the `.zizmor.yml` rules are a fallback in case Renovate later rewrites those comments.

AC6b. `.github/workflows/ci.yml` helm-install job extracts `tools.rust` and `tools.node` from `.mise.toml` via `mise config get` (step writes to `$GITHUB_OUTPUT`), then references them on the subsequent `docker build` step via an `env:` block (`RUST_VERSION: ${{ steps.tool-versions.outputs.rust }}`, `NODE_VERSION: ${{ steps.tool-versions.outputs.node }}`) and consumes them as `${RUST_VERSION}` / `${NODE_VERSION}` inside the `run:` shell — avoiding zizmor's `template-injection` audit that fires on direct `${{ ... }}` expansion inside `run:` blocks. After this change, a `mise.toml` rust/node bump that lands without a corresponding Dockerfile ARG default bump still produces a CI build using the mise-pinned version (the ARG default is fallback-only).

AC7. `renovate.json` matches `https://docs.renovatebot.com/renovate-schema.json` shape (no `_comment` fields, all keys are valid Renovate config). Mend's hosted Renovate does NOT run a config-validation CI check on the `renovate/configure` onboarding branch (that flow uses `renovate/reconfigure`), so schema verification is deferred to Renovate's first run after merge — the dependency dashboard issue Renovate creates will surface any config parse errors. Local pre-push schema sanity is via visual inspection against the schema URL plus the editor's JSON-schema validation (VS Code etc.). If a config syntax error is found post-merge, it's a one-line fix-up PR.

AC8. `renovate.json` contains:
- `extends: ["config:best-practices"]` (bundles `config:recommended`, `docker:pinDigests`, `helpers:pinGitHubActionDigests`, `:pinDevDependencies`, `abandonments:recommended`, `security:minimumReleaseAgeNpm`, `:maintainLockFilesWeekly`)
- `platformAutomerge: false`, `automerge: false` at top level (per-rule overrides)
- `rangeStrategy: "pin"`
- `automergeSchedule` matches `schedule`
- `minimumReleaseAge: "3 days"`
- `osvVulnerabilityAlerts: true` and `vulnerabilityAlerts.enabled: true`
- `lockFileMaintenance` with `enabled: true`, `automerge: true`, Sunday schedule
- **No `customManagers` field** (native managers cover the surface; see Architecture)
- The packageRules in the order described in Architecture, including cross-manager groups (rust-toolchain using `matchManagers: ["mise", "dockerfile"]`, pnpm-version using `["mise", "npm"]`), Svelte ecosystem custom group (no forced `commitMessagePrefix`), ts-rs / sqlx / opentelemetry-rust special-cases, and the security override pair with `automergeSchedule: ["at any time"]` on the catch-all rule

AC9. CI passes on the PR (lint, type-check, test, build, helm checks, PR title check).

AC10. The PR description includes the post-merge ruleset runbook: fetch the ruleset, **strip response-only fields** (`id`, `node_id`, `created_at`, `updated_at`, `_links`, `current_user_can_bypass`, `source`, `source_type`), **idempotently** append the four required status check contexts (`Backend Result`, `Frontend Result`, `Helm Result`, `PR Checks`) and the Renovate bypass actor (`actor_id: 2740, actor_type: "Integration", bypass_mode: "pull_request"`), `diff` against the original, then PUT. Re-running the runbook is a no-op if the rule and actor are already present.

AC11. The PR title reflects the full deliverable (not just the first commit) per the squash-merge convention.

AC12 (post-merge, not gating PR merge): After Bojan applies the ruleset PUTs and Renovate's next run completes, the dependency dashboard issue exists, and the first batch of Renovate PRs respects the new policy (no major automerge; minor / patch automerge after 3-day delay; security PRs labeled).

## Documents to Update

| Doc | Update |
|-----|--------|
| `CLAUDE.md` (top-level) | No update — Renovate is operational tooling, not architecture. |
| `CONTRIBUTING.md` | Add a short "Dependency updates" subsection under or near Commit Conventions explaining Renovate's commit-prefix mapping (`fix(deps):` runtime → release-please patch; `chore(deps):` dev → no release) and how it interacts with release-please. Brief — point readers to `renovate.json` for the full policy. |
| `docs/architecture/ci-pipeline.md` | Add a "Dependency updates" section pointing to `renovate.json` and describing the auto-merge gating (required status checks + 3-day age + Renovate's polling). Cross-link to this design plan in `docs/design-plans/`. |
| `docs/architecture/release-pipeline.md` | No update — `release-please.yml` is unchanged from main once we use the `.zizmor.yml` fallback approach (inline `# zizmor: ignore[superfluous-actions]` stays as-is), so the doc-staleness gate at `scripts/check-docs-lefthook.sh:54` doesn't fire. |
| `scripts/doc-mapping.sh` | Add mapping `renovate.json` → `docs/architecture/ci-pipeline.md` so the doc-staleness gate fires when Renovate config changes. |
| `docs/design-plans/2026-05-10-renovate-onboarding.md` | This plan, copied from `~/.claude/plans/jaunty-spinning-wand.md` after approval. |

## Implementation Guidance

`docs/implementation-guidance.md` is the runtime contract for execution; read it before writing any code (general rule for all design-plan-driven work).

Project memory feedback files that bite for this scope:

- `feedback_pr_title_convention.md` — PR title reflects full deliverable, not just first commit. **Critical for this PR.**
- `feedback_test_plans.md` — test plan goes as the first PR comment, not in the PR description body.
- `feedback_pr_body_convention.md` — PR body is the squash commit body; write as "what will be implemented" at design time, update to "what was implemented" at finish.
- `feedback_verify_just_recipes_before_citing.md` — verify just recipe names before referencing them in commit messages or PR text. (Already verified `just lint`, `just check`, `just test`, `just test-e2e`, `just types` exist.)
- `feedback_run_e2e_tests_for_frontend_changes.md` — `pnpm-lock.yaml` regen counts as a frontend change. Run `just test-e2e` before pushing.
- `feedback_no_pip_install_in_agents.md` — do NOT run `pip install`, `npm install -g`, `cargo install` from any subagent.
- `feedback_use_just_test_or_nextest.md` — use `just test` or `cargo nextest run`, never bare `cargo test`.
- `feedback_author_not_user.md` — "author" for Bojan-as-developer, "operator" for deployment voice, "user" only for live conversation party. Applied throughout this plan.
- `feedback_plans_in_repo_no_review_artifacts.md` — when copying this plan to `docs/design-plans/`, strip drafty annotations (e.g., "open question", "TODO", "previous draft").
- `feedback_dont_assume_dep_minimalism.md` — Renovate intentionally adds operational complexity in exchange for security/freshness; do not frame "fewer deps" as a virtue in commit text or PR body.
- `feedback_stash_before_reset.md` — if a `git reset --hard` is needed during the worktree work, stash any uncommitted edits first.
- `feedback_verify_lefthook_installed.md` — run `just setup` in the new worktree at start; lefthook hooks aren't inherited.

## Out of Scope

- **GitHub ruleset edits** (required status checks + bypass actor) — documented as a post-merge runbook, performed via `gh api`. Not a file edit; happens on GitHub config, not in the repo.
- **TypeScript v6 migration** — TypeScript 6.0.2 shipped March 23, 2026, but the repo runs on 5.9.3 and the migration involves `tsconfig.json` shifts, Svelte 5 TS integration, and dependent type stubs. The dead `^6.0.2` catalog entry is reconciled to `5.9.3`; migration is its own initiative.
- **Migrating other non-catalog deps that are stable standalone** — `@bgotink/playwright-coverage` migrates because it pairs with `@playwright/test` (Playwright group). No other "courtesy" migrations.
- **Self-hosted Renovate** — repo uses Mend's hosted Renovate. No change.
- **`abandonments:recommended` preset** — defaulted to omit (solo-maintainer noise from 12-month-no-release issues). One-line PR to add later if useful.
- **Excluding `@biomejs/biome` from broad dev-dep automerge** — defaulted to keep biome in automerge (formatting changes are cosmetic; CI catches behavior). Reversible.
- **First-pass Renovate PRs after the config merges** — auto-handled by Renovate. No human action expected unless something fails. The "Pin Dependencies" PR Renovate may auto-create on first run is part of normal onboarding.

## Glossary

- **Mend Renovate App** — the hosted Renovate GitHub App at https://github.com/apps/renovate, App ID 2740. Distinct from self-hosted Renovate.
- **Pre-1.0 minor as breaking** — Renovate's `isBreaking()` for cargo (and effectively for npm) reclassifies a 0.x → 0.(x+1) update as `updateType: major`, matching SemVer's "0.y.z is unstable" rule.
- **Aggregator job (`*-result`)** — a CI job with `if: always()` that depends on path-filtered children and reports a stable success/skip context regardless of whether the children ran. Required-status-check contexts must use these aggregators, not the conditional children.
- **Catalog (pnpm)** — the centralized version map in `pnpm-workspace.yaml`'s `catalog:` block. Workspace package.json files reference `"catalog:"` to inherit; Renovate's npm manager updates the catalog entries (depType `pnpm.catalog.default`).
