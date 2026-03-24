# Repository Bootstrap Design

## Summary

This phase establishes the engineering foundation for the ATC repository before any application code is written. It creates a private GitHub repository and layers in three categories of infrastructure in sequence: tool version management (mise pins Rust, Node, pnpm, just, and lefthook to exact versions), a unified task runner (a root `justfile` with stub recipes that will be filled in by later phases), and enforced developer conventions (lefthook wires three tiers of git hooks — pre-commit linting, commit-message validation via commitlint, and a pre-push gate that will eventually run tests and check documentation staleness). The three implementation phases map cleanly onto these categories, with each phase depending on the one before it.

The design emphasizes intentional scaffolding: hooks, scripts, and justfile recipes are installed and wired in Phase 1 even when they do nothing yet, so later phases can activate them by adding code rather than reconfiguring the toolchain. The documentation enforcement chain (editor-time Claude Code hooks, a commit-time advisory, and a pre-push blocking gate) follows the same pattern — the scripts exist and the hooks call them from the start, but `doc-mapping.sh` has empty mappings until Phase 2 adds the first architecture document. This approach keeps each subsequent phase focused on its own deliverables rather than revisiting bootstrap configuration.

## Definition of Done

Phase 1 establishes the ATC repository's engineering foundation — git hygiene, enforced conventions, tooling, and developer experience — before any application code exists. Every subsequent phase inherits these rules.

**Success looks like:**

1. **A private GitHub repo** (`bojanrajkovic/atc`) with a clean initial commit history
2. **Tool versions locked** via `.mise.toml` — `mise install` provisions Rust, Node, pnpm, and just
3. **Task runner works** — `just lint`, `just fmt`, `just check`, `just test`, `just dev`, `just build` all execute (stubs/no-ops where no code exists yet)
4. **Three-tier git hooks enforced** — `lefthook install` wires pre-commit (linting), commit-msg (commitlint), and pre-push (tests + doc-staleness check). All hooks glob-filtered; commitlint rejects non-conventional commits
5. **Project documentation** — `README.md` (product intro), `CLAUDE.md` (AI agent index), and `CONTRIBUTING.md` (human workflows + documentation conventions) provide enough context for a new contributor to get started
6. **Documentation conventions established** — CONTRIBUTING.md documents the five-layer documentation model, architecture doc template (4 required anchors + timestamp), ADR convention with retroactive annotation, and non-duplication rule
7. **Doc enforcement chain scaffolded** — `scripts/doc-mapping.sh` and `scripts/check-docs-lefthook.sh` exist and are wired into the pre-push hook, but inactive (empty mappings) until Phase 2 adds architecture docs
8. **Docs directory structure** — `docs/architecture-decisions/`, `docs/ideation/`, and `docs/design-plans/` directories exist
9. **Legal** — Apache-2.0 LICENSE file present
10. **Clean `.gitignore`** covering Rust, Node, IDE, and OS artifacts

## Acceptance Criteria

### repo-bootstrap.AC1: GitHub repo exists and is accessible
- **repo-bootstrap.AC1.1 Success:** `gh repo view bojanrajkovic/atc` returns repo metadata with visibility "private"
- **repo-bootstrap.AC1.2 Success:** Cloning via `git clone` works with user's GitHub credentials

### repo-bootstrap.AC2: Tool versions are provisioned correctly
- **repo-bootstrap.AC2.1 Success:** `mise install` completes without errors and provisions rust, node, just, lefthook
- **repo-bootstrap.AC2.2 Success:** `corepack enable` succeeds and `pnpm --version` returns the version from package.json's `packageManager` field
- **repo-bootstrap.AC2.3 Success:** Running `rustc --version`, `node --version`, `just --version`, `lefthook version` all return expected versions matching `.mise.toml`

### repo-bootstrap.AC3: Task runner recipes execute
- **repo-bootstrap.AC3.1 Success:** `just setup` completes all four steps (mise install, corepack enable, pnpm install, lefthook install) without errors
- **repo-bootstrap.AC3.2 Success:** `just lint`, `just fmt`, `just check`, `just test`, `just dev`, `just build` all exit 0 with stub messages

### repo-bootstrap.AC4: Commit conventions are enforced
- **repo-bootstrap.AC4.1 Success:** A conventional commit (e.g., `feat: add feature`) is accepted
- **repo-bootstrap.AC4.2 Failure:** A non-conventional commit (e.g., `bad message`) is rejected by commitlint via lefthook's commit-msg hook
- **repo-bootstrap.AC4.3 Success:** Scoped commits (e.g., `fix(backend): resolve issue`) are accepted (free-form scopes)

### repo-bootstrap.AC5: Three-tier git hooks are configured
- **repo-bootstrap.AC5.1 Success:** `lefthook install` completes and wires pre-commit, commit-msg, and pre-push hooks
- **repo-bootstrap.AC5.2 Success:** Committing with no staged `.rs`, `.ts`, `.js`, or `.svelte` files skips all linting hooks (glob filter)
- **repo-bootstrap.AC5.3 Edge:** Committing only a `.md` file change triggers commitlint but skips all linting hooks
- **repo-bootstrap.AC5.4 Success:** Pre-push hook runs (stubs exit 0 in Phase 1 since no code exists)

### repo-bootstrap.AC6: Documentation is complete
- **repo-bootstrap.AC6.1 Success:** README.md exists with project overview, links to docs/ and CONTRIBUTING.md
- **repo-bootstrap.AC6.2 Success:** CLAUDE.md exists with tech stack, commands, doc pointers, documentation framework
- **repo-bootstrap.AC6.3 Success:** CONTRIBUTING.md exists with prerequisites, setup instructions, and commit conventions

### repo-bootstrap.AC7: Documentation conventions are established
- **repo-bootstrap.AC7.1 Success:** CONTRIBUTING.md documents the five-layer documentation model with non-duplication rule
- **repo-bootstrap.AC7.2 Success:** CONTRIBUTING.md documents the architecture doc template (4 required anchors + timestamp)
- **repo-bootstrap.AC7.3 Success:** CONTRIBUTING.md documents the ADR convention with retroactive annotation
- **repo-bootstrap.AC7.4 Success:** `docs/architecture-decisions/` directory exists
- **repo-bootstrap.AC7.5 Success:** `docs/ideation/` directory exists
- **repo-bootstrap.AC7.6 Success:** `docs/design-plans/` directory exists

### repo-bootstrap.AC8: Doc enforcement chain is scaffolded
- **repo-bootstrap.AC8.1 Success:** `scripts/doc-mapping.sh` exists and is executable
- **repo-bootstrap.AC8.2 Success:** `scripts/check-docs-lefthook.sh` exists, is executable, and exits 0 when no mappings are defined
- **repo-bootstrap.AC8.3 Success:** Pre-push hook calls `check-docs-lefthook.sh` (exits 0 — no mappings yet)

### repo-bootstrap.AC9: Legal and housekeeping
- **repo-bootstrap.AC9.1 Success:** LICENSE file contains Apache-2.0 text
- **repo-bootstrap.AC9.2 Success:** `.gitignore` excludes Rust targets, Node artifacts, IDE files, OS files, and mise local files
- **repo-bootstrap.AC9.3 Edge:** Running `cargo init` in a subdirectory does not produce a root-level `Cargo.lock` (covered by .gitignore)

## Glossary

- **ATC**: The project being bootstrapped — Actions Traffic Control, a real-time GitHub Actions dashboard.
- **mise**: A polyglot version manager (analogous to `asdf`) that installs and activates exact versions of language runtimes and CLI tools defined in `.mise.toml`.
- **Corepack**: A Node.js built-in shim manager that reads the `packageManager` field in `package.json` and provisions the exact pnpm version specified.
- **just**: A command runner (similar to `make` but without build-graph semantics) used as the unified CLI; all developer commands are `just <recipe>`.
- **stub recipe**: A `just` recipe that prints a message and exits 0 without doing real work; used so CI-callable targets exist before any code lands.
- **lefthook**: A Git hooks manager that reads `lefthook.yml` and wires hook scripts at git hook trigger points.
- **glob filter**: A file-pattern constraint on a lefthook hook command; if no staged file matches the glob, the command is skipped entirely.
- **commitlint**: A JavaScript tool that validates git commit messages against Conventional Commits format.
- **Conventional Commits**: A commit message specification (`<type>(<scope>): <description>`) enabling automated changelog generation.
- **three-tier hooks**: The lefthook topology: pre-commit (linting), commit-msg (message validation), pre-push (tests + doc gate).
- **clippy**: The official Rust linter.
- **rustfmt**: The official Rust code formatter.
- **Biome**: A fast JavaScript/TypeScript linter and formatter, successor to Rome.
- **`doc-mapping.sh`**: Shell script mapping source file paths to their architecture docs; read by all three enforcement chain layers.
- **`check-docs-lefthook.sh`**: Pre-push script that blocks if source files were modified without updating their architecture doc.
- **documentation enforcement chain**: Three-layer system (editor-time Claude Code hook → commit-time advisory → pre-push blocking gate) preventing architecture doc staleness.
- **five-layer documentation model**: docs/architecture/ (implemented features), CONTRIBUTING.md (human workflows), root CLAUDE.md (AI index), per-directory CLAUDE.md (directive extracts), docs/ideation/ (planned features).
- **non-duplication rule**: Each piece of information has exactly one home across the five layers.
- **required anchor sections**: Four mandatory sections every architecture doc must contain: Purpose, Key Decisions, Boundaries, Files.
- **ADR**: Architecture Decision Record in `docs/architecture-decisions/` recording significant architectural choices.
- **retroactive annotation**: Inline callout (`> **Revised by ADR-NNN:** ...`) added to existing documents when a new ADR supersedes described behavior.
- **PostToolUse hook**: Claude Code extension point that fires after every Edit/Write tool call; triggers editor-time documentation reminders.
- **SHA pinning**: Referencing GitHub Actions at full commit SHAs rather than mutable tags, preventing supply-chain attacks.
- **Renovate**: Automated dependency update tool; bumps `.mise.toml` versions and keeps SHA pins current via `helpers:pinGitHubActionDigests`.
- **Loupe**: Existing project from which several conventions are adopted (documentation model, enforcement chain, ADR convention, SHA pinning).

## Architecture

### Tooling Stack

Four tools, each with a single responsibility, managed by two systems:

| Tool | Managed by | Responsibility |
|------|-----------|----------------|
| Rust (stable) | mise | Backend language |
| Node (latest) | mise | Frontend runtime |
| pnpm | Corepack (via Node) | JS package manager |
| just | mise | Task runner (unified CLI) |
| lefthook | mise | Git hooks |
| commitlint | pnpm (root package.json) | Commit message validation |

**Separation principle:** mise manages all non-JS binaries. npm/pnpm manages JS libraries only. No mixing — lefthook and just are binaries, not JS tools, so they belong in mise.

### Version Pinning

Exact versions pinned in `.mise.toml` (e.g., `node = "24.14.0"`, `rust = "1.94.0"`). No lockfile — `.mise.lock` is gitignored. Renovate bumps `.mise.toml` directly and can automerge without lockfile sync friction.

pnpm version pinned via `"packageManager"` field in root `package.json`. Corepack provides the exact version after `corepack enable`.

### Git Hooks Architecture

Lefthook manages three tiers of hooks (adopted from Loupe's two-tier pattern, extended with E2E):

**`pre-commit`** — runs linters in parallel, glob-filtered:
- `clippy` (glob: `*.rs`)
- `rustfmt` (glob: `*.rs`)
- `biome` (glob: `*.{ts,js}`)
- `eslint-svelte` (glob: `*.svelte`)

All four auto-skip when no matching staged files exist. In Phase 1 they are no-ops. They activate naturally as Phase 2 adds code — no `lefthook.yml` changes required between phases.

**`commit-msg`** — runs commitlint to validate conventional commit format. Active from Phase 1.

**`pre-push`** — the strongest gate, runs before code reaches the remote:
- Unit tests (cargo test + pnpm vitest)
- E2E tests (pnpm playwright — diverges from Loupe which keeps E2E CI-only)
- Doc-staleness check via `scripts/check-docs-lefthook.sh` (scaffolded in Phase 1, inactive until Phase 2 adds architecture docs)

The pre-push hook approximately mirrors CI, giving high confidence before push. All stubs in Phase 1.

### Documentation Enforcement Chain

Three-layer graduated defense-in-depth (adopted from Loupe):

1. **Editor-time** — Claude Code `PostToolUse` hook fires on every Edit/Write, maps the modified file to its architecture doc via `scripts/doc-mapping.sh`, emits an in-session reminder. Primary enforcement loop for AI-assisted development.
2. **Commit-time advisory** — Claude Code hook fires on git commit/add, maps staged files to docs, emits non-blocking context.
3. **Pre-push blocking gate** — `scripts/check-docs-lefthook.sh` performs a branch-scoped diff against `origin/main` and blocks with exit code 1 if source files were modified but their architecture doc was not.

All three layers read from `scripts/doc-mapping.sh`, which maps source file paths to their canonical architecture doc. In Phase 1, `doc-mapping.sh` is scaffolded with empty mappings — the hooks exist but do nothing until Phase 2 adds the first architecture doc and mapping entries.

### Task Runner Design

Root `justfile` provides the unified CLI. Phase 1 recipes are stubs that print a message and exit 0 (success). Stubs become real implementations as subsequent phases add code.

| Recipe | Phase 1 | Phase 2+ |
|--------|---------|----------|
| `just setup` | mise install, corepack enable, pnpm install, lefthook install | Same |
| `just lint` | Stub (no code) | cargo clippy + pnpm lint |
| `just fmt` | Stub (no code) | cargo fmt + pnpm fmt |
| `just check` | Stub (no code) | cargo check + pnpm build |
| `just test` | Stub (no code) | cargo test + pnpm test |
| `just dev` | Stub (no code) | Parallel Vite + Axum |
| `just build` | Stub (no code) | cargo build --release |

Stubs exit 0 so CI (Phase 4) can call `just lint` on a repo with no code without failing.

### Documentation Architecture

Five-layer model with explicit non-duplication rule (adopted from Loupe):

1. **`docs/architecture/`** — canonical source of truth for implemented features. Every doc follows the required-anchor template (see below). All implementation details live here.
2. **`CONTRIBUTING.md`** — human workflows only: conventional commits, PR process, dev setup, architecture doc template, ADR convention.
3. **`CLAUDE.md` (root)** — AI agent index. Compact pointers to docs/, commands, project status. Admonition to keep docs/ updated, not this file.
4. **`<subdir>/CLAUDE.md`** (Phase 2+) — directive extracts for high-risk directories. Created only where agents make costly mistakes. Each references its canonical source: "Derived from `docs/architecture/<topic>.md`".
5. **`docs/ideation/`** — living documents for planned-but-unbuilt features. When a feature ships, the ideation doc archives and the architecture doc becomes the source of truth.

**Non-duplication rule:** Each piece of information has exactly one home. Do not duplicate content across layers. A lean CLAUDE.md that points agents to the right doc ensures they find accurate current information rather than cached-and-incorrect summaries.

**`README.md`** stands apart — glossy product intro, not technical reference. Links to docs/ for architecture, CONTRIBUTING.md for workflows.

### Architecture Doc Template Convention

Every architecture doc must include four required anchor sections (adopted from Loupe):

- **Purpose** — what this component does and why it exists
- **Key Decisions** — architectural choices with rejected alternatives and rationale
- **Boundaries** — what this component owns, what it does NOT own, explicit prohibitions
- **Files** — which source files this doc covers

Plus module-specific middle sections as needed (Architecture, Data Model, Schema, Contracts, Invariants). All docs carry a **"Last verified: YYYY-MM-DD"** timestamp.

This template is documented in Phase 1 (in CONTRIBUTING.md) even though the first architecture doc is written in Phase 2.

### ADR Convention

Architecture Decision Records live in `docs/architecture-decisions/` (adopted from Loupe). When a significant architectural decision changes, create an ADR with full context and rationale. Existing documents that describe the superseded behavior carry inline callouts: `> **Revised by ADR-NNN:** ...`. This keeps historical documents readable while marking what changed.

## Existing Patterns

No existing code — this is a greenfield repository. The following patterns are adopted from the [Loupe project](https://github.com/loupe-app/loupe):

- **Five-layer documentation model** with non-duplication rule ([PR #262](https://github.com/loupe-app/loupe/pull/262))
- **Architecture doc template** with four required anchors (Purpose, Key Decisions, Boundaries, Files) + "Last verified" timestamp
- **Three-layer documentation enforcement chain** (Claude Code hooks + pre-push gate + doc-mapping.sh)
- **Three-tier lefthook hooks** (pre-commit, commit-msg, pre-push) — extended from Loupe's two-tier pattern to include frontend E2E in pre-push
- **ADR convention** with retroactive annotation of superseded documents
- **GitHub Actions SHA pinning** with Renovate `helpers:pinGitHubActionDigests` for automated maintenance

## Implementation Phases

<!-- START_PHASE_1 -->
### Phase 1: Repository Initialization

**Goal:** Git repo exists on GitHub with license, ignore rules, and tool version management.

**Components:**
- GitHub repo `bojanrajkovic/atc` (private) created via `gh repo create`
- `LICENSE` — Apache-2.0
- `.gitignore` — Rust (`target/`), Node (`node_modules/`, `dist/`), IDE (`.idea/`, `.vscode/`), OS (`.DS_Store`), tooling (`.mise.local.toml`, `mise.lock`)
- `.mise.toml` — exact versions for rust, node, just, lefthook
- `docs/architecture-decisions/` — empty directory (ADR convention documented in CONTRIBUTING.md)
- `docs/ideation/` — empty directory (ideation doc convention documented in CONTRIBUTING.md)
- `docs/design-plans/` — directory for design plans (this document lives here)

**Dependencies:** None (first phase)

**Done when:** `gh repo view bojanrajkovic/atc` succeeds. `mise install` provisions all four tools. `corepack enable` makes pnpm available.
<!-- END_PHASE_1 -->

<!-- START_PHASE_2 -->
### Phase 2: Task Runner and Git Hooks

**Goal:** Unified CLI via justfile, enforced commit conventions via lefthook + commitlint.

**Components:**
- `justfile` — `setup`, `lint`, `fmt`, `check`, `test`, `dev`, `build` recipes (stubs for lint/fmt/check/test/dev/build)
- `lefthook.yml` — three-tier hooks: `pre-commit` (parallel linting, glob-filtered) + `commit-msg` (commitlint) + `pre-push` (unit tests + E2E + doc-staleness, all stubs)
- `package.json` — root-level, `private: true`, commitlint devDependencies, `packageManager` field for pnpm
- `.commitlintrc.mjs` — extends `@commitlint/config-conventional`, free-form scopes
- `pnpm-lock.yaml` — generated by `pnpm install`
- `scripts/doc-mapping.sh` — scaffolded with empty mappings (maps source paths → architecture docs)
- `scripts/check-docs-lefthook.sh` — pre-push doc-staleness gate (scaffolded, exits 0 when no mappings exist)

**Dependencies:** Phase 1 (repo + mise)

**Done when:** `just setup` completes successfully. `lefthook install` wires all three hook tiers. A non-conventional commit (e.g., `bad message`) is rejected by commitlint. `just lint` runs and exits 0.
<!-- END_PHASE_2 -->

<!-- START_PHASE_3 -->
### Phase 3: Documentation

**Goal:** README, CLAUDE.md, and CONTRIBUTING.md provide enough context for a new contributor (human or AI) to understand the project and start working.

**Components:**
- `README.md` — glossy product intro: what ATC is, why it exists, screenshot placeholder, links to docs/ and CONTRIBUTING.md
- `CLAUDE.md` — AI agent index: project status, tech stack one-liner, commands, doc pointers, documentation framework explanation, admonition to keep docs/ current
- `CONTRIBUTING.md` — human workflows: prerequisites (mise), `just setup`, conventional commits, PR guidelines, architecture doc template (4 required anchors + timestamp), ADR convention, non-duplication rule, five-layer doc model

**Dependencies:** Phase 2 (justfile recipes exist to reference in docs)

**Done when:** All three documents exist, reference correct commands, and provide a coherent onboarding path. Documentation conventions (architecture doc template, ADR format, non-duplication rule) are documented in CONTRIBUTING.md. A reader can go from zero to `just setup` by following the docs.
<!-- END_PHASE_3 -->

## Additional Considerations

**Design plan guidance (`.ed3d/design-plan-guidance.md`):** Informs future *design* sessions — architectural principles and conventions that affect brainstorming and design decisions:

1. **Phases evolve justfile recipes** — stubs become real implementations as code lands. Each phase updates only the recipes relevant to its deliverables.
2. **Lefthook hooks are pre-configured** — new phases should NOT need to modify `lefthook.yml` unless adding an entirely new tool category.
3. **Module-level CLAUDE.md files are directive extracts** — only for high-risk directories. Always reference canonical source in `docs/architecture/`. Derive from architecture docs when agents encounter sharp edges — do not pre-create speculatively.
4. **Non-duplication rule** — each piece of information has exactly one home across the five documentation layers. CLAUDE.md and README.md point to docs/, they don't duplicate content.
5. **Architecture docs use the required-anchor template** — Purpose, Key Decisions (with rejected alternatives), Boundaries, Files, plus module-specific sections. All carry a "Last verified: YYYY-MM-DD" timestamp.
6. **Design plans include a "Documents to Update" table** — before coding, list every architecture doc, CLAUDE.md, and skill file that must change alongside the implementation.
7. **ADRs carry retroactive annotations** — when creating an ADR, annotate all existing documents that describe superseded behavior with `> **Revised by ADR-NNN:** ...`

**Implementation plan guidance (`.ed3d/implementation-plan-guidance.md`):** Informs future *implementation* plans — tool-specific instructions and actions for task writers:

1. **Never pin library versions** — always use `pnpm add <pkg>` or `cargo add <crate>` to pull latest stable at execution time. Do not hardcode versions in task descriptions.
2. **Update doc-mapping.sh when adding architecture docs** — every new architecture doc needs a corresponding entry in `scripts/doc-mapping.sh` mapping source paths to the doc. The doc enforcement chain depends on this.
3. **GitHub Actions use SHA-pinned references** — all `uses:` references pin to full commit SHAs. Renovate's `helpers:pinGitHubActionDigests` preset keeps them current automatically.
4. **ADR implementation includes annotation sweep** — when implementing an ADR, sweep all existing documents and tests for superseded behavior. Annotate docs with `> **Revised by ADR-NNN:** ...` and retire or revise affected tests.

**Patterns noted for future phases (from Loupe analysis):**
- **Phase 5 (Release):** Two-phase release workflow — release-please manages changelog only (`skip-github-release: true`); separate tag-triggered workflow creates GitHub Release + builds binary.
- **Phase 5 (Release):** Renovate automerge non-major updates, group by ecosystem (Cargo, Svelte, GitHub Actions). Add `helpers:pinGitHubActionDigests` preset.
