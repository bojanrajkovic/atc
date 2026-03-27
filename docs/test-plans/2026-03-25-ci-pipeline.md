# Human Test Plan: CI Pipeline

**Implementation plan:** `docs/implementation-plans/2026-03-25-ci-pipeline/`
**Generated:** 2026-03-26

## Prerequisites

- GitHub repository with the branch containing `ci.yml` and `zizmor.yml` deployed to `.github/workflows/`
- Repository settings: branch protection enabled on `main` requiring status checks
- Repository secret `CODECOV_TOKEN` configured (required for AC4.1, AC4.2)
- Codecov GitHub App installed on the repository (required for AC4.1, AC4.2)
- Ability to create temporary branches and open PRs
- Access to the repository's Settings > Security > Code Scanning tab (required for AC7.2)
- Local clone with `scripts/verify-workflow-security.sh` executable (`chmod +x`)

```bash
# Verify automated tests pass before starting manual verification
bash scripts/verify-workflow-security.sh
```

---

## Phase 1: Path Filtering and Structure (AC3, AC8.4)

**Purpose:** Verify that `dorny/paths-filter` correctly routes jobs and that concurrency cancellation works.

| Step | Action | Expected |
|------|--------|----------|
| 1.1 | Create branch `test/ac3-frontend-only`. Make a trivial change to `frontend/src/app.css` (e.g., add a comment). Open a PR titled `test: verify frontend-only path filtering`. | PR opens successfully. |
| 1.2 | Wait for CI to complete. Navigate to the Actions tab for the PR. Inspect the "Detect Changes" job output for `paths-filter`. | `backend` output is `false`, `frontend` output is `true`. |
| 1.3 | Check the "Backend" job status. | Shows as "Skipped" (grey circle). |
| 1.4 | Check the "Backend Result" gate job. | Shows as "Passed" (green check). The `if` condition correctly identifies no backend changes and exits 0. **(AC3.1)** |
| 1.5 | Close PR. Create branch `test/ac3-backend-only`. Make a trivial change to `backend/Cargo.toml` (e.g., add a comment). Open a PR titled `test: verify backend-only path filtering`. | "Frontend" job is skipped. "Frontend Result" gate passes. "Backend" job runs. **(AC3.2)** |
| 1.6 | Close PR. Create branch `test/ac3-shared-config`. Modify `justfile` (e.g., add a comment). Open a PR titled `test: verify shared config triggers both stacks`. | Both "Backend" and "Frontend" jobs run (not skipped). **(AC3.3)** |
| 1.7 | Merge any one of the above PRs to `main` (or push directly to main). Navigate to Actions and find the push-triggered CI run. | Both "Backend" and "Frontend" jobs run regardless of which files were changed, because `github.event_name == 'push'` bypasses path filtering. **(AC3.4)** |
| 1.8 | Create branch `test/ac8-concurrency`. Push a commit. Immediately push a second commit (within seconds) to the same branch before the first run completes. Navigate to Actions tab. | The first CI run shows as "Cancelled". The second run proceeds to completion. **(AC8.4)** |

---

## Phase 2: Backend Quality Checks (AC1.1 -- AC1.4)

**Purpose:** Verify each backend quality gate fails on violations and passes on clean code.

| Step | Action | Expected |
|------|--------|----------|
| 2.1 | Open a PR with clean Rust code (no violations). This can be the branch from step 1.5 if it had clean code, or create a new branch with a trivial valid change to any `.rs` file. Wait for CI. | The "Backend" job passes. All steps (Check formatting, Lint (clippy), Compile check) show green. **(AC1.1)** |
| 2.2 | Create branch `test/ac1-fmt`. In any `.rs` file under `backend/`, add a malformatted line (e.g., `let     x=1;` with extra spaces). Push and open a PR titled `test: verify cargo fmt catches violations`. | The "Check formatting" step (`cargo fmt --check`) fails with a diff showing the formatting violation. The overall "Backend" job fails. **(AC1.2)** |
| 2.3 | Create branch `test/ac1-clippy`. In any `.rs` file, add `let x = 42;` without using `x` (and without `#[allow(unused)]`). Push and open a PR titled `test: verify clippy catches warnings`. | The "Lint (clippy)" step fails with `warning: unused variable: x` promoted to error by `-D warnings`. **(AC1.3)** |
| 2.4 | Create branch `test/ac1-compile`. In any `.rs` file, add `let x: String = 42;` (type mismatch). Push and open a PR titled `test: verify compile check catches errors`. | The "Compile check" step (`cargo check --workspace`) fails with a type error. **(AC1.4)** |

---

## Phase 3: Frontend Quality Checks (AC2.1 -- AC2.4)

**Purpose:** Verify each frontend quality gate fails on violations and passes on clean code.

| Step | Action | Expected |
|------|--------|----------|
| 3.1 | Open a PR with clean frontend code. Wait for CI. | The "Frontend" job passes. All steps (Check formatting, Lint, Type check) show green. **(AC2.1)** |
| 3.2 | Create branch `test/ac2-format`. In a `.ts` file under `frontend/src/`, introduce a Biome formatting violation (e.g., use `var` instead of `let/const`, or inconsistent semicolons depending on Biome config). Push and open a PR. | The "Check formatting (Biome)" step fails. **(AC2.2)** |
| 3.3 | Create branch `test/ac2-eslint`. In a `.svelte` file, introduce an ESLint violation (e.g., use an undeclared variable in the `<script>` block). Push and open a PR. | The "Lint (ESLint -- Svelte files)" step fails with the ESLint error. **(AC2.3)** |
| 3.4 | Create branch `test/ac2-typecheck`. In a `.svelte` file, introduce a TypeScript type error (e.g., assign a number to a string-typed variable). Push and open a PR. | The "Type check (svelte-check)" step fails with the type error. **(AC2.4)** |

---

## Phase 4: Test Execution and Coverage (AC1.5, AC2.5, AC4)

**Purpose:** Verify tests run, coverage uploads, and artifacts are available even on failure.

| Step | Action | Expected |
|------|--------|----------|
| 4.1 | Create branch `test/ac1-test-fail`. Add a failing `#[test]` to any backend test file (e.g., `#[test] fn always_fails() { assert!(false); }`). Push and open a PR. | The "Run tests with coverage" step in the "Backend" job fails. **(AC1.5)** |
| 4.2 | On the failed run from 4.1, navigate to the run's "Artifacts" section at the bottom of the summary page. | `test-results-backend` artifact is present despite the job failure (`if: always()` ensures this). **(AC4.3 -- backend)** |
| 4.3 | Create branch `test/ac2-test-fail`. Add a failing vitest test to any frontend test file (e.g., `test('always fails', () => { expect(true).toBe(false); })`). Push and open a PR. | The "Run tests with coverage" step in the "Frontend" job fails. **(AC2.5)** |
| 4.4 | On the failed run from 4.3, check artifacts. | `test-results-frontend` artifact is present despite job failure. **(AC4.3 -- frontend)** |
| 4.5 | On any successful CI run where both stacks executed (e.g., the shared-config PR from step 1.6), check the artifacts section. | Two separate artifacts appear: `test-results-backend` and `test-results-frontend`. No name collision. **(AC4.4)** |
| 4.6 | After a successful CI run with backend tests, navigate to the Codecov dashboard for the repository. | The `backend` flag appears with coverage data from `backend/lcov.info`. **(AC4.1)** |
| 4.7 | After a successful CI run with frontend tests, navigate to the Codecov dashboard. | The `frontend` flag appears with coverage data from `frontend/coverage/lcov.info`. **(AC4.2)** |

---

## Phase 5: PR-Specific Checks (AC5)

**Purpose:** Verify dependency review and PR title validation.

| Step | Action | Expected |
|------|--------|----------|
| 5.1 | Open a PR with a non-conventional title such as "Added CI pipeline" (no type prefix). Wait for the "PR Checks" job. | The "Validate PR title" step (`amannn/action-semantic-pull-request`) fails, reporting the title does not match conventional commit format. **(AC5.2)** |
| 5.2 | Edit the PR title to a valid conventional format, e.g., `ci: add CI pipeline`. Re-run the "PR Checks" job (or push a new commit to trigger a new run). | The "Validate PR title" step passes. The "Dependency review" step also passes (no new vulnerable deps). **(AC5.3)** |
| 5.3 | Create branch `test/ac5-vuln-dep`. In `frontend/package.json`, add a dependency with a known moderate+ vulnerability (e.g., an old version of `lodash` with known CVEs: `"lodash": "4.17.15"`). Run `pnpm install` to update the lockfile. Push and open a PR with a valid conventional title. | The "Dependency review" step (`actions/dependency-review-action` with `fail-on-severity: moderate`) fails, listing the vulnerable dependency. **(AC5.1)** |

---

## Phase 6: Release Build (AC6)

**Purpose:** Verify release compilation is checked in CI.

| Step | Action | Expected |
|------|--------|----------|
| 6.1 | On any successful backend CI run, inspect the "Release build" step. | The step `cargo build --release -p atc-server` completes successfully (green check). **(AC6.1)** |
| 6.2 | Create branch `test/ac6-release-only`. In any backend source file, add `#[cfg(not(debug_assertions))] compile_error!("release-only test");`. Push and open a PR. | The "Release build" step fails with the compile error, while the earlier "Compile check" step (which runs in debug mode) passes. This proves release-only errors are caught. **(AC6.2)** |

---

## Phase 7: Zizmor Workflow Security Linting (AC7)

**Purpose:** Verify zizmor triggers on workflow changes and reports to Security tab.

| Step | Action | Expected |
|------|--------|----------|
| 7.1 | The initial push that adds `zizmor.yml` to `.github/workflows/` itself should trigger the "Workflow Security" workflow. Navigate to Actions tab after that push. | A "Workflow Security" workflow run appears, triggered by the change to files under `.github/workflows/`. **(AC7.1)** |
| 7.2 | After the zizmor run completes, navigate to Security tab > Code Scanning in the repository. | Zizmor findings (if any) are listed. If the workflows are clean, the tool reports zero findings but the analysis result is still recorded. **(AC7.2)** |
| 7.3 | Push a commit that modifies ONLY non-workflow files (e.g., a backend source file or `README.md`). Navigate to Actions tab. | The "Workflow Security" workflow does NOT appear in the list for that push event. Only the "CI" workflow runs. **(AC7.3)** |

---

## End-to-End: Full PR Lifecycle

**Purpose:** Validate that a realistic PR experiences the complete CI pipeline correctly from open to merge.

1. Create a feature branch `feat/e2e-test` from `main`.
2. Make a small valid change in both `backend/` and `frontend/` (e.g., add a doc comment to a Rust file and a CSS comment to a Svelte file).
3. Open a PR with title `feat: end-to-end CI validation`.
4. **Observe:** "Detect Changes" runs and outputs `backend=true`, `frontend=true`.
5. **Observe:** Both "Backend" and "Frontend" jobs start.
6. **Observe:** "PR Checks" job runs (dependency review + PR title validation).
7. **Observe:** All jobs complete green (assuming clean code).
8. **Observe:** "Backend Result" and "Frontend Result" gates both pass.
9. **Observe:** Two artifacts (`test-results-backend`, `test-results-frontend`) appear.
10. **Observe:** Codecov receives both `backend` and `frontend` coverage reports.
11. Merge the PR to `main`.
12. **Observe:** The push-triggered CI run on `main` runs both stacks regardless of path filter output.
13. **Observe:** "PR Checks" job does NOT run on the push event (it has `if: github.event_name == 'pull_request'`).

---

## End-to-End: Security-Hardened Pipeline

**Purpose:** Validate that the security posture is maintained end-to-end across both workflow files.

1. Run `bash scripts/verify-workflow-security.sh` locally from the repository root.
2. **Observe:** Script exits 0, confirming AC8.1, AC8.2, AC8.3.
3. On any CI run, inspect the raw workflow logs for the checkout steps.
4. **Observe:** No credential persistence messages (confirming `persist-credentials: false` is effective at runtime, not just in YAML).
5. On any CI run, inspect the "Set up job" initialization for each job.
6. **Observe:** Each job's permissions are scoped to only what it needs (e.g., `contents: read` only, not `contents: write`).

---

## Traceability

| Acceptance Criterion | Automated Test | Manual Step |
|----------------------|----------------|-------------|
| AC1.1 Clean Rust PR passes | -- | Phase 2, step 2.1 |
| AC1.2 `cargo fmt` violation fails | -- | Phase 2, step 2.2 |
| AC1.3 clippy warning fails | -- | Phase 2, step 2.3 |
| AC1.4 Compilation error fails | -- | Phase 2, step 2.4 |
| AC1.5 Failing test fails backend | -- | Phase 4, step 4.1 |
| AC2.1 Clean frontend PR passes | -- | Phase 3, step 3.1 |
| AC2.2 Biome/Prettier violation fails | -- | Phase 3, step 3.2 |
| AC2.3 ESLint error fails | -- | Phase 3, step 3.3 |
| AC2.4 svelte-check type error fails | -- | Phase 3, step 3.4 |
| AC2.5 Failing vitest test fails | -- | Phase 4, step 4.3 |
| AC3.1 Frontend-only skips backend | -- | Phase 1, steps 1.1--1.4 |
| AC3.2 Backend-only skips frontend | -- | Phase 1, step 1.5 |
| AC3.3 Shared config triggers both | -- | Phase 1, step 1.6 |
| AC3.4 Push to main runs both | -- | Phase 1, step 1.7 |
| AC4.1 Backend coverage in Codecov | -- | Phase 4, step 4.6 |
| AC4.2 Frontend coverage in Codecov | -- | Phase 4, step 4.7 |
| AC4.3 Artifacts on failed run | -- | Phase 4, steps 4.2, 4.4 |
| AC4.4 Distinct artifact names | -- | Phase 4, step 4.5 |
| AC5.1 Vulnerable dep fails review | -- | Phase 5, step 5.3 |
| AC5.2 Non-conventional title fails | -- | Phase 5, step 5.1 |
| AC5.3 Conventional title passes | -- | Phase 5, step 5.2 |
| AC6.1 Release build succeeds | -- | Phase 6, step 6.1 |
| AC6.2 Release-only error caught | -- | Phase 6, step 6.2 |
| AC7.1 Workflow change triggers zizmor | -- | Phase 7, step 7.1 |
| AC7.2 Findings in Security tab | -- | Phase 7, step 7.2 |
| AC7.3 Non-workflow change skips zizmor | -- | Phase 7, step 7.3 |
| AC8.1 SHA-pinned uses with comments | `scripts/verify-workflow-security.sh` | E2E Security, step 1 |
| AC8.2 persist-credentials: false | `scripts/verify-workflow-security.sh` | E2E Security, step 1 |
| AC8.3 permissions: {} at workflow level | `scripts/verify-workflow-security.sh` | E2E Security, step 1 |
| AC8.4 Concurrent run cancellation | -- | Phase 1, step 1.8 |
