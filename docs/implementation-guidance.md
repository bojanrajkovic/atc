# Implementation Guidance

Last verified: 2026-06-04

## Purpose

Rules for implementing features in ATC. These apply after the user has triggered the plan handoff ("clear context and bypass permissions") and a fresh context is running in bypass-permissions mode. For the design and planning phase (Plan Mode behavior), see [`docs/planning-workflow.md`](planning-workflow.md).

## Rules

### 1. Work on the feature branch from the design plan

Implementation continues on the branch where the design plan was committed. Do not create a new branch. The PR title must describe the full deliverable (squash merge makes it the commit message on main). Test plans go in the PR's first comment — never in the PR description.

### 2. Use test-driven development for behavior-changing work

For behavior-changing work, write tests before writing implementation code. For each acceptance criterion in the design plan: write a failing test that captures the behavior, then write the minimum code to make it pass, then refactor. Do not write implementation first and tests after — tests written after implementation tend to confirm the implementation rather than the requirement.

For refactors that preserve behavior, the regression net must stay green throughout. Extending or restructuring tests is fine, but the "red phase" of TDD does not apply — behavior-preserving change has no new behavior to assert.

### 3. Never pin library versions

Always use `pnpm add <pkg>` or `cargo add <crate>` to pull the latest stable at execution time. Do not hardcode versions. For version research during planning, use local tools: `mise ls-remote <tool>`, `cargo search <crate>`, `npm view <pkg> version`.

### 4. Update doc-mapping.yaml when adding architecture docs

Every new architecture doc needs a corresponding entry in `scripts/doc-mapping.yaml` mapping source paths to the doc. The doc-staleness enforcement chain depends on this mapping.

### 5. GitHub Actions use SHA-pinned references

All `uses:` references pin to full commit SHAs. Renovate's `helpers:pinGitHubActionDigests` preset keeps them current automatically.

### 6. ADR implementation includes annotation sweep

When implementing an ADR, sweep all existing documents and tests for superseded behavior. Annotate docs with `> **Revised by ADR-NNN:** ...` and retire or revise affected tests.

### 7. Split large Rust test files by concern

When a Rust test file exceeds ~500 lines or covers more than two distinct concern areas, break it into submodules organized by concern (not implementation detail). Shared helpers go in `tests/mod.rs`; submodules import via `use super::*`; property tests stay in a top-level sibling file. See `backend/crates/atc-core/src/state_machine/tests/` for the reference pattern.

**TypeScript:** Do not split TypeScript test files by line count or concern count. Keep them cohesive.

### 8. Never hand-edit generated.ts

`frontend/src/lib/types/generated.ts` is produced by `ts-rs` from Rust structs via `just types`. Manual edits will be overwritten. To change a type, change the Rust struct and regenerate.

### 9. Never modify copied shadcn-svelte components for theming

`app.css` defines a CSS alias layer (`--background: var(--bg)`, etc.) that maps shadcn variable names to ATC's OKLCH tokens. Copied components work unmodified. If a shadcn component looks wrong, fix the alias layer, not the component source. This keeps `pnpm exec shadcn-svelte add <component>` working without post-copy patches.

### 10. Use MSW for network-level tests, direct calls for logic tests

`ConnectionManager` tests use `msw/node` to intercept `fetch()` and WebSocket at the network level. Store and `EventDispatcher` tests call methods directly (no MSW, no mocking). `EventDispatcher.flush()` bypasses `requestAnimationFrame` for synchronous assertions in tests.

### 11. Visual regression against playground reference

When implementing UI components, capture Playwright screenshots (`page.screenshot()`) and compare against the playground prototype at `docs/ideation/playground.html`. Applies to any step that produces visible UI.

### 12. Updating shadcn-svelte components

To update copied components when shadcn-svelte releases a new version, re-run `pnpm exec shadcn-svelte add <component>`. The CSS alias layer means no post-copy patching is needed. Check the shadcn-svelte changelog for breaking changes before updating.

### 13. Split Vitest tests into projects by environment

When some tests need browser mode (e.g., Svelte 5 `$effect` reliability) and others work under jsdom, use separate Vitest `projects` in the config. Don't mix environments — browser mode is slower, and mixing creates flaky tests. Use filename conventions (`.browser.test.ts` vs `.test.ts`) or directory-based `include` patterns to route tests.

### 14. Use implementation subagents when they pay for themselves

The orchestrating context reads the committed design plan from `docs/design-plans/` and coordinates. **Dispatch implementation subagents when:** (a) two or more genuinely independent file sets need parallel edit application, or (b) a step requires search/grep over ~15+ files where keeping the artifacts out of the main context is worth the dispatch overhead.

**Skip subagents when** the work is a coherent sequential edit chain — coordination overhead exceeds the parallelism gain, and reading back a subagent's output to apply edits manually is slower than editing inline.

The planning Claude may name steps as parallelizable in the design plan; the implementing Claude is the final arbiter of whether the parallelism is worth the dispatch.

### 15. Lefthook hooks are pre-configured

New implementation steps should NOT modify `lefthook.yml` unless adding an entirely new tool category. Run `just setup` at the start of any implementation session (especially after cloning or creating worktrees) to ensure hooks are installed. Verify with `ls .git/hooks/pre-commit` — a `.sample` file only means hooks are not wired.

### 16. Use project-specific researcher agents for investigation

For any read-only codebase or external-source investigation, use the agent preference order in [`docs/planning-workflow.md` § 1 Context Gathering](planning-workflow.md#1-context-gathering).

### 17. Strip planning-artifact labels from current-state artifacts

When writing tests, comments, architecture docs, or `CLAUDE.md` content, do not carry forward planning-artifact labels — phase numbers (`Phase 2c`), acceptance-criteria numbers (`AC2.1`), test-sequence numbers (`T1`, `T6b`), or bare ADR references. The behavioral description after the tag is almost always sufficient on its own. See `CONTRIBUTING.md` § "Planning-Artifact Labels" for the full convention, including what to strip, what to keep, and the audit-time grep.
