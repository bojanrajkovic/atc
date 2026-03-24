# Human Test Plan: Repo Bootstrap

**Generated from:** `docs/implementation-plans/2026-03-22-repo-bootstrap/`
**Date:** 2026-03-23

## Prerequisites

- macOS or Linux workstation
- `mise` installed and on PATH (`mise --version` returns a version)
- GitHub SSH credentials configured (`ssh -T git@github.com` succeeds)
- Terminal with working directory set to the project root
- Run `just setup` once before starting

## Phase 1: Repository and Tooling Foundation

| Step | Action | Expected |
|------|--------|----------|
| 1.1 | `git clone git@github.com:bojanrajkovic/atc.git /tmp/atc-clone-test` | Clone completes with exit 0 |
| 1.2 | `rm -rf /tmp/atc-clone-test` | Temp clone cleaned up |
| 1.3 | `mise install` | "all tools are installed" or installs without errors. Exit 0. |
| 1.4 | `rustc --version` | Contains `1.94.0` (matching `.mise.toml`) |
| 1.5 | `node --version` | `v24.14.0` (matching `.mise.toml`) |
| 1.6 | `just --version` | `just 1.47.1` (matching `.mise.toml`) |
| 1.7 | `lefthook version` | `2.1.4` (matching `.mise.toml`) |

## Phase 2: Developer Workflow

| Step | Action | Expected |
|------|--------|----------|
| 2.1 | `just setup` | All four steps complete: mise install, corepack enable, pnpm install, lefthook install. Exit 0. |
| 2.2 | `just lint` | "lint: no code to lint yet". Exit 0. |
| 2.3 | `just fmt` | "fmt: no code to format yet". Exit 0. |
| 2.4 | `just check` | "check: no code to check yet". Exit 0. |
| 2.5 | `just test` | "test: no tests to run yet". Exit 0. |
| 2.6 | `just dev` | "dev: no dev servers to start yet". Exit 0. |
| 2.7 | `just build` | "build: nothing to build yet". Exit 0. |
| 2.8 | `echo "feat: add new feature" \| pnpm exec commitlint` | Exit 0. Conventional message accepted. |
| 2.9 | `echo "bad message" \| pnpm exec commitlint` | Exit 1. Violations: "subject-empty", "type-empty". |
| 2.10 | `echo "fix(backend): resolve issue" \| pnpm exec commitlint` | Exit 0. Scoped commit accepted. |
| 2.11 | `ls .git/hooks/pre-commit .git/hooks/commit-msg .git/hooks/pre-push` | All three files listed. |

## Phase 3: Documentation Completeness

| Step | Action | Expected |
|------|--------|----------|
| 3.1 | Open `README.md`. Scan for links. | Links to `CONTRIBUTING.md` and `docs/` are present. |
| 3.2 | Open `CLAUDE.md`. Check for sections. | "Tech Stack", "Commands", "Documentation Map", "Documentation Framework" all present. |
| 3.3 | Open `CONTRIBUTING.md`. Check core sections. | "Prerequisites", "just setup", "Commit Conventions" all present. |
| 3.4 | In `CONTRIBUTING.md`, find "Five-Layer Documentation Model". | Section with table of all 5 layers. "Non-duplication rule" stated explicitly. |
| 3.5 | In `CONTRIBUTING.md`, find "Architecture Doc Template". | Four required anchors: Purpose, Key Decisions, Boundaries, Files. "Last verified" timestamp documented. |
| 3.6 | In `CONTRIBUTING.md`, find "ADR Convention". | ADR instructions with "Revised by ADR" retroactive annotation pattern. |

## End-to-End: Fresh Clone to First Commit

1. `git clone git@github.com:bojanrajkovic/atc.git /tmp/atc-e2e-test`
2. `cd /tmp/atc-e2e-test`
3. `just setup` — verify all four steps complete without errors
4. `just` (no args) — verify it lists all available recipes
5. `echo "test" > test-file.txt && git add test-file.txt`
6. `git commit -m "bad message"` — verify commitlint rejects it
7. `git commit -m "chore: add test file"` — verify commit succeeds
8. `git log --oneline -1` — verify message is "chore: add test file"
9. `cd ~ && rm -rf /tmp/atc-e2e-test`

## End-to-End: Doc-Staleness Gate

1. `scripts/check-docs-lefthook.sh` — verify exits 0
2. Open `scripts/doc-mapping.sh` — verify `get_doc_for_file` function exists with commented examples
3. Verify `lefthook.yml` has `pre-push` → `doc-staleness` → `scripts/check-docs-lefthook.sh`

## Human Verification Required

### AC5.2: Non-code files skip all linting hooks

1. `echo "test" > test-skip.toml`
2. `git add test-skip.toml`
3. `git commit -m "chore: test hook skip behavior"`
4. **Observe lefthook output** — clippy, rustfmt, biome, eslint-svelte should all show "skipped"
5. Commitlint (commit-msg hook) should still run and pass
6. Cleanup: `git reset HEAD~1 && rm test-skip.toml`

### AC5.3: Markdown-only change triggers commitlint but skips linters

1. `echo "" >> README.md`
2. `git add README.md`
3. `git commit -m "docs: test markdown-only hook behavior"`
4. **Observe lefthook output** — all four linting commands should show "skipped"
5. Commitlint should run and pass (conventional message)
6. Cleanup: `git reset HEAD~1 && git checkout README.md`

## Traceability Matrix

| AC | Automated | Manual Step |
|----|-----------|-------------|
| AC1.1 | `gh repo view` | 1.1 |
| AC1.2 | `git remote -v` | 1.1-1.2, E2E Clone |
| AC2.1 | `mise install` | 1.3 |
| AC2.2 | `pnpm --version` | 2.1 |
| AC2.3 | version commands | 1.4-1.7 |
| AC3.1 | `just setup` | 2.1 |
| AC3.2 | stub recipes | 2.2-2.7 |
| AC4.1 | commitlint valid | 2.8 |
| AC4.2 | commitlint invalid | 2.9 |
| AC4.3 | commitlint scoped | 2.10 |
| AC5.1 | hook files exist | 2.11 |
| AC5.2 | — | Human: AC5.2 |
| AC5.3 | — | Human: AC5.3 |
| AC5.4 | check-docs exits 0 | E2E Doc-Staleness |
| AC6.1-6.3 | grep checks | 3.1-3.3 |
| AC7.1-7.3 | grep checks | 3.4-3.6 |
| AC7.4-7.6 | `test -d` | automated only |
| AC8.1-8.3 | `test -x` + grep | E2E Doc-Staleness |
| AC9.1-9.3 | grep + check-ignore | automated only |
