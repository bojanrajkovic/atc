# Contributing to ATC

## Prerequisites

- **[mise](https://mise.jdx.dev)** — Polyglot version manager. Installs and manages all tool versions.
- **[Docker](https://www.docker.com)** (or **[OrbStack](https://orbstack.dev)** on macOS) — Required to run `just test`. The backend test suite uses [testcontainers](https://testcontainers.com) to boot ephemeral PostgreSQL instances. If Docker is unavailable, testcontainers tests fail loudly (they do not silently skip).

All other tools (Rust, Node.js, pnpm, just, lefthook, sqlx-cli) are provisioned automatically by mise from `.mise.toml`.

## Getting Started

```bash
git clone git@github.com:bojanrajkovic/atc.git
cd atc
just setup    # Installs tools, enables corepack, installs deps, sets up git hooks
```

`just setup` runs four steps:
1. `mise install` — Provisions Rust, Node.js, just, and lefthook
2. `corepack enable` — Activates pnpm via Node's Corepack shim
3. `pnpm install` — Installs JavaScript dependencies (commitlint)
4. `lefthook install` — Wires pre-commit, commit-msg, and pre-push git hooks

## Development Commands

All commands go through the `just` task runner:

| Command | Description |
|---------|-------------|
| `just setup` | Bootstrap the development environment |
| `just lint` | Run all linters |
| `just fmt` | Format all code |
| `just check` | Type-check / compile-check |
| `just test` | Run all tests |
| `just dev` | Start development servers |
| `just build` | Production build |

## Running Tests

```bash
just test    # Runs all tests (requires Docker or OrbStack)
```

Test runner choice (nextest vs cargo test), the OrbStack `DOCKER_HOST` setup, what `just test` does and does not cover (Playwright E2E is separate via `just test-e2e`), and the backend-test foot-guns (`#[serial_test::serial]` for OTel, `OnceLock`-guarded exporter harness, no-source-grep-assertion rule) all live in [`docs/testing.md`](docs/testing.md).

## Inspecting OpenTelemetry Output Locally

ATC exports traces and metrics over OTLP/HTTP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set; with the env var unset the SDK is never initialized. To inspect what `atc-server` emits during local development:

1. Start the bundled all-in-one stack (Grafana otel-lgtm — OpenTelemetry collector, Tempo, Mimir, Loki, and a pre-wired Grafana UI in one container):

   ```bash
   just otel-dev-stack
   ```

2. Run the ATC dev servers with the exporter pointed at the collector. Set the env var in your shell, then run `just dev`:

   ```bash
   export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
   just dev
   ```

3. Open <http://localhost:3000> for the Grafana UI. The image ships with anonymous access enabled and Tempo/Mimir/Loki pre-configured as datasources, so you can navigate Explore -> Tempo to see spans, Explore -> Mimir to see metrics, and Explore -> Loki for whatever you forward there. Trigger any webhook or hit `/v1/state` to generate emissions.

4. When done, tear the stack down:

   ```bash
   just otel-dev-stack-stop
   ```

`just dev` itself does not start the observability stack — it remains backend + frontend dev servers only, with no Docker dependency. The stack is opt-in via the dedicated recipe.

## Updating SQL Queries

SQL queries in `atc-store-pg` (and any other workspace crate that uses sqlx macros) use the `sqlx::query!` / `sqlx::query_as!` macros for compile-time type checking. The Cargo workspace root is `backend/`, and the offline query cache lives at `backend/.sqlx/` — committed to the repository so CI can build without a live database.

**When to regenerate the cache:** any time you add, remove, or change a `query!` / `query_as!` macro call, or modify a migration in `backend/crates/atc-store-pg/migrations/`.

**How to regenerate:**

1. Start a local Postgres and apply migrations from `backend/`:
   ```bash
   docker run -d --rm --name atc-pg -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:17-alpine
   cd backend
   DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
     cargo sqlx migrate run --source crates/atc-store-pg/migrations
   ```

2. Regenerate the cache from the same `backend/` directory (the Cargo workspace root):
   ```bash
   DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
     cargo sqlx prepare --workspace -- --tests
   ```
   The `--tests` flag includes queries in `#[cfg(test)]` code. `--workspace` writes the cache to `backend/.sqlx/`.

3. Commit the updated `backend/.sqlx/` files in the **same commit** as the SQL change:
   ```bash
   git add backend/.sqlx/
   git commit -m "feat(server): <description of SQL change>"
   ```

**Why CI doesn't need `DATABASE_URL`:** sqlx 0.8 automatically uses the committed `backend/.sqlx/` offline cache when no `DATABASE_URL` is set in the build environment. The existing `.github/workflows/ci.yml` requires no changes.

## Commit Conventions

This project uses [Conventional Commits](https://www.conventionalcommits.org/). Every commit message must follow this format:

```
<type>(<scope>): <description>

[optional body]

[optional footer(s)]
```

**Types:** `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`

**Scopes:** Free-form. Use whatever scope makes sense for the change (e.g., `backend`, `frontend`, `api`, `auth`). No predefined list — scopes emerge organically.

**Examples:**
```
feat: add workflow run list endpoint
fix(frontend): correct timestamp display in run table
docs: update architecture doc for API module
chore: bump rust to 1.94.0
```

Commits are validated by commitlint via lefthook's commit-msg hook. Non-conventional commits are rejected automatically.

### Keep parentheses balanced on each line of a commit body

release-please parses every commit — subject **and** body — with a strict conventional-commits parser to build the changelog and decide version bumps. It reads a `word(` (an open paren glued to a token, like a function or macro call) as the start of a scope and expects the matching `)` before the line ends. A `(` left open at the end of a line, or a `something(...)` call split across lines, makes the parser throw — and release-please then **silently drops the entire commit**: no changelog entry, no version bump.

In practice:

- Keep each line's parentheses balanced. Don't end a line on an open `name(`, and don't split a call or macro like `overriding(vec![ … ])` or `process::exit(1)` across lines.
- Prose parentheticals are fine as long as they open and close on the same line — `(self-healing)` is OK; a `(` that wraps to the next line is not.
- This governs the squash-merge body too: the PR description becomes the commit message on `main`, so the same rule applies to any PR description that pastes in code snippets.

This is a known release-please limitation with no lenient-parse option ([googleapis/release-please#2564](https://github.com/googleapis/release-please/issues/2564)); authoring discipline is the fix.

### Atomic commits

Each logical change is one commit, even when a sub-phase contains many of them. A reviewer should be able to scan a commit's diff in seconds and understand what landed. PR-tab summaries describe what shipped *overall*; the commit history is what someone running `git blame` will read months later.

### Dependency updates

Renovate manages dependency updates with conventional-commit prefixes that cooperate with release-please:

- `fix(deps):` — runtime dependency bump (release-please publishes a patch release).
- `chore(deps):` — dev/tooling dependency bump (no release).

Minor/patch updates auto-merge after a 3-day release-age delay; major updates require manual review. Security advisories bypass the delay. The full policy lives in `renovate.json`.

### Dependency Philosophy

Prefer best-in-class, batteries-included libraries over hand-rolled minimalism. "Fewer dependencies" is a neutral fact, not a virtue — rank options by ergonomics, maintenance health, and fit, not by dep count. Adding a well-maintained crate or package to get good out-of-box behavior is a fair trade. When proposing architectural alternatives, do not write "(Recommended)" next to an option whose main advantage is minimalism.

## Planning-Artifact Labels

Design plans, ADRs, and implementation tickets use numbering schemes to coordinate work — phases (`Phase 2c`, `Sub-Phase 4`), acceptance criteria (`AC2.1`, `AC10.3`), test sequences (`T1`, `T6b`, `T11`), and bare ADR refs (`per ADR-0005`). These belong in the **historical record only**: ADRs, design plans, ideation, commit messages, CHANGELOG, and the `Last verified:` line at the top of CLAUDE.md / AGENTS.md files. They must NOT survive into current-state artifacts.

**Why:** a future maintainer reading a test failure or scanning a comment will not have the design plan in their head. A test name like `t11_concurrent_same_entity_commits_in_seq_order` or a comment `// AC6.7: reconnect silence during buffered drain` couples the code to a planning document and adds nothing the behavioral text alone does not already say.

**Strip from:**

- **Test function and file names** — `phase_NX_*`, `ac<N>_<M>_*`, and `t<N>[a-z]?_*` prefixes. Rename to describe the invariant being verified.
- **Code comments** — module docs, doc comments on items, inline comments, section banners (e.g., `// ===== ... (AC5.1–AC5.4) =====`). Describe what the code does, not which planning artifact it satisfies.
- **Test report labels** — `describe(...)` / `test.describe(...)` strings. Behavioral text after the prefix usually already exists; preserve it and drop the tag.
- **Module-level docstrings that enumerate test cases** (e.g., `T1 — does X / T2 — does Y`) — rewrite as a description of what the file covers as a whole, not a numbered list.
- **User-visible strings** — chart-time `{{ fail }}`, `tracing::error!`, `NOTES.txt`, README, Prometheus metric description strings.
- **Architecture docs and CLAUDE.md / AGENTS.md** — these describe what IS, not what HAS BEEN. Planning-artifact references almost always live inside changelog-flavored content that itself doesn't belong; trim the content, not just the labels.

**Keep in:**

- ADRs (`docs/architecture-decisions/`), design plans (`docs/design-plans/`), ideation (`docs/ideation/`) — these documents *are* the historical record. Acceptance criteria belong in design plans by design; the numbers are useful inside the plan, they just shouldn't escape into the code.
- Commit messages and CHANGELOG.
- The `Last verified: YYYY-MM-DD (#N closed: …)` line at the top of CLAUDE.md / AGENTS.md (authorship metadata).
- Captured external history — e.g., webhook fixture commit messages from real GitHub Actions output (data, not authored content).
- Definitional references that explain what the term means rather than using it (e.g., the `(AC1, AC2, …)` parenthetical in `docs/planning-workflow.md`).

**Audit hint:** when stripping one class, sweep the others at the same time. The starter regex `rg 'Phase \d|AC[0-9]|\bT[0-9]+[a-z]?\b|fn (phase|ac|t)[0-9]'` catches all four common patterns; tune as new schemes appear.

**The pattern:** ask "is this artifact part of the current contract / current state, or part of the historical record?" If current, strip the planning-artifact reference (and probably the surrounding sentence — these refs usually accompany changelog narration that doesn't belong in a current-state doc). If historical, keep.

## Git Hooks

Three tiers of git hooks are enforced via lefthook:

### Pre-commit (Tier 1)
Runs linters in parallel, glob-filtered by file type:
- **clippy** — Rust linter (`.rs` files)
- **rustfmt** — Rust formatter (`.rs` files)
- **biome** — JS/TS linter (`.ts`, `.js` files)
- **eslint-svelte** — Svelte linter (`.svelte` files)

If no staged files match a linter's glob, the linter is skipped entirely.

### Commit-msg (Tier 2)
Validates commit messages against Conventional Commits format via commitlint.

### Pre-push (Tier 3)
The strongest gate — runs before code reaches the remote:
- Unit tests (cargo test + vitest)
- E2E tests (playwright)
- Documentation staleness check

## Continuous Integration

This project maintains two GitHub Actions workflows:

### CI Workflow (ci.yml)
Runs quality checks on every pull request and push to main:
- **On pull requests:** Path-filtered backend/frontend checks run only for modified stacks. Backend tests, lints, and build checks run for `.rs` changes. Frontend tests, lints, and build checks run for `.svelte`, `.ts`, `.js` changes. Always-run checks: dependency review (PRs only), PR title validation (conventional commit format).
- **On pushes to main:** Both backend and frontend checks always run (no path filtering).
- **Results:** Check the "Backend Result" and "Frontend Result" status checks to see detailed results for each stack.

### Workflow Security (zizmor.yml)
Lints all workflow files (`.github/workflows/**`) for security and correctness issues:
- **Trigger:** Activates only when workflow files change (not on every commit).
- **Results:** Findings appear in the repository's Security tab under "Code scanning" as security advisories. Not a required status check — use for proactive security improvement.
- **Coverage:** Scans for hardcoded secrets, unsafe ref pinning, overly permissive permissions, and other GitHub Actions best practices.

## Releases

Releases are automated via [release-please](https://github.com/googleapis/release-please) and a tag-triggered release workflow.

**How it works:**

1. Merge commits to `main` using [Conventional Commits](#commit-conventions) format
2. release-please automatically creates/updates a release PR that bumps versions and updates CHANGELOGs
3. When the release PR is merged, a `v*` git tag is created
4. The tag triggers the release workflow which:
   - Creates a GitHub Release from the CHANGELOG
   - Builds `atc-server` binaries for Linux (x86_64, aarch64) and macOS (Apple Silicon)
   - Builds and pushes a multi-arch Docker image to `ghcr.io/bojanrajkovic/atc`
   - Attests all artifacts via Sigstore

**Version bumping rules:**

| Commit prefix | Version bump | Example |
|--------------|-------------|---------|
| `feat:` | Minor (0.x.0) | New feature |
| `fix:` | Patch (0.0.x) | Bug fix |
| `feat!:` or `BREAKING CHANGE:` | Major (x.0.0) | Breaking change |

ATC releases as a single product: one version drives the seven Rust crates (inherited from `[workspace.package].version`), the frontend, and the Helm chart, and one bare `v<version>` tag triggers the release workflow.

**Container image:** `docker pull ghcr.io/bojanrajkovic/atc:latest`

**Verifying artifacts:**

```bash
# Verify a downloaded binary
gh attestation verify ./atc-server-x86_64-unknown-linux-musl.tar.gz -R bojanrajkovic/atc

# Verify the container image
gh attestation verify oci://ghcr.io/bojanrajkovic/atc:latest -R bojanrajkovic/atc
```

## Pull Requests

- Create a branch from `main`
- Make your changes with conventional commits
- Push — pre-push hooks run tests and doc-staleness checks
- Open a PR against `main`
- PRs require passing CI checks
- **Squash merges**: This repo uses squash merges. The PR description becomes the squashed commit body verbatim, so write it for someone running `git log` months later, not for a reviewer with the PR tab open.
- **Test plans**: Post the test plan as the **first comment** on the PR via `gh pr comment`. Never put it in the PR description. Never commit it as a `test-plan.md` or similar file in the repo — test plans are ephemeral review artifacts, not permanent content.
- **PR body shape**: Open with a one-or-two-sentence lead paragraph stating what shipped — **no `## Summary` header on the lead**, because the PR title *is* the summary and a `## Summary` heading inside a commit message reads weirdly in `git log`. After the lead, use `## Subsystem` headers to organize the detail (e.g. `## Domain model`, `## Persistence trait`, `## Frontend`). Do not use `## Test Plan`, `## Changes`, or other PR-tab-shaped sections — those belong in the first PR comment, not the body.
- **PR body voice**: When a design-plan PR is first created, write the body as "what will be implemented" — describe the planned architecture and approach. When finishing the branch (after implementation is complete), update the body to "what was implemented" with concrete details (function names, types, design decisions, how modules fit together). Organize by logical subsystem, not by phase or task number. Use implementation-focused prose, not checkbox summaries with test counts — the body becomes the squash commit body, so it should read like commit documentation.
- **PR title**: Conventional Commits format (`type(scope): description`), describing the full implementation deliverable — not the first commit. A design-doc PR for a `feat` lands as `feat:`, not `docs:`, because the squash merge makes the PR title the commit message on `main`.
- **Update the body before merge**: Fix-up commits that respond to review feedback (Codex, manual) refine the implementation; their content gets folded into the squash commit. If the refinement introduces new invariants, new APIs, or new operator-visible behavior, update the PR body to mention them before clicking merge — the body is the only durable record once the branch is gone.
- **Fix the class, not the instance**: When a code review flags a structural bug pattern, identify the underlying invariant and apply the fix to every site where it holds — not just the call sites the reviewer named. "The reviewer didn't flag it" is not a sufficient reason to skip a fix the structural analysis already implies; the architecture-doc note for the fix should state the invariant explicitly so future readers find the rule, not just the symptom.

---

## Documentation Conventions

The doc-system spec — six-layer model, architecture-doc guidance, ADR convention, terminology, committed design plans, directory-level CLAUDE.md discipline, observability authoring contract — lives in [`docs/documentation-system.md`](docs/documentation-system.md). CONTRIBUTING.md focuses on human workflows and conventions; the canonical home for documentation rules is that file.
