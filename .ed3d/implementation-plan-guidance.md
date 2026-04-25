# Implementation Plan Guidance

Tool-specific instructions and actions for implementation plan task writers in the ATC project.

## Rules

1. **Never pin library versions** — always use `pnpm add <pkg>` or `cargo add <crate>` to pull latest stable at execution time. Do not hardcode versions in task descriptions. Do not dispatch internet-researcher subagents to look up current versions — they hit rate limits and return stale data. For version research during planning, use local tools: `mise ls-remote <tool>`, `cargo search <crate>`, `npm view <pkg> version`.

2. **Update doc-mapping.sh when adding architecture docs** — every new architecture doc needs a corresponding entry in `scripts/doc-mapping.sh` mapping source paths to the doc. The doc enforcement chain depends on this.

3. **GitHub Actions use SHA-pinned references** — all `uses:` references pin to full commit SHAs. Renovate's `helpers:pinGitHubActionDigests` preset keeps them current automatically.

4. **ADR implementation includes annotation sweep** — when implementing an ADR, sweep all existing documents and tests for superseded behavior. Annotate docs with `> **Revised by ADR-NNN:** ...` and retire or revise affected tests.

5. **Split large test files by acceptance criteria** — when a test file exceeds ~500 lines or covers more than 2 distinct AC groups, break it into submodules organized by AC/concern area (not implementation detail). Shared helpers go in `tests/mod.rs`, submodules import via `use super::*`, property tests stay in a top-level sibling file. See `backend/crates/atc-core/src/store/tests/` for the reference pattern.

6. **Never hand-edit `generated.ts`** — `frontend/src/lib/types/generated.ts` is produced by `ts-rs` from Rust structs via `just types`. Manual edits will be overwritten. If a type needs changing, change the Rust struct and regenerate.

7. **Never modify copied shadcn-svelte component source for theming** — `app.css` defines a CSS alias layer (`--background: var(--bg)`, etc.) that maps shadcn variable names to ATC's OKLCH tokens. Copied components work unmodified. If a shadcn component looks wrong, fix the alias layer, not the component source. This ensures `pnpm exec shadcn-svelte add <component>` works without post-copy patching when updating or adding components.

8. **Use MSW for network-level tests, direct calls for logic tests** — ConnectionManager tests use `msw/node` to intercept `fetch()` and WebSocket at the network level. Store and EventDispatcher tests call methods directly (no MSW, no mocking). `EventDispatcher.flush()` bypasses `requestAnimationFrame` for synchronous assertions in tests.

9. **Visual regression against playground reference** — when implementing UI components, use Playwright screenshots (`page.screenshot()`) to capture rendered output and compare against the playground prototype (`docs/ideation/playground.html`). The `agent-browser` skill can automate visual comparison for more sophisticated checks. This applies to any phase that produces visible UI — capture a screenshot after implementation and verify it matches the validated design.

10. **shadcn-svelte component updates** — to update copied components when shadcn-svelte releases a new version, re-run `pnpm exec shadcn-svelte add <component>`. The CSS alias layer means no post-copy patching is needed. Check the shadcn-svelte changelog for breaking changes before updating.

11. **Split Vitest tests into projects by environment** — if some tests need browser mode (e.g., Svelte 5 `$effect` reliability) and others work under jsdom, use separate Vitest `projects` in the config. Don't run everything under one environment — browser mode is slower, and mixing environments creates flaky tests. Use filename conventions (`.browser.test.ts` vs `.test.ts`) or directory-based `include` patterns to route tests.

12. **Task-implementor reports must include `git show --stat HEAD~N..HEAD`** — every implementation report (whether one task or a bundled subcomponent) must include the literal output of `git show --stat HEAD~N..HEAD` (where N = number of commits made in this task) at the top of the "Verification Evidence" or "Commits" section. Do not paraphrase the diff stat; paste the tool output verbatim. **Why:** agents have repeatedly written commit messages and reports claiming additions that don't exist (e.g., "added AC2.4 test to file X" while the diff stat shows file X was never touched). The diff stat is objective, copy-pasted from a tool, and impossible to embellish — if it doesn't show the file you claimed to modify, the executor sees the gap immediately. **How to apply:** after the final commit and before drafting the summary, run `git show --stat HEAD~N..HEAD` (or `git diff --stat <task-base>..HEAD` for a single-commit task) and paste the unedited output into the report. If the stat shows fewer files than your claimed deliverable, do not submit the report — go fix the gap first.

13. **Prefer `task-implementor` over `task-implementor-fast` for multi-task subcomponents** — the `-fast` variant trades self-reporting thoroughness for speed and has been observed silently dropping individual tasks within a bundle (e.g., implementing tasks 1 and 3 of a three-task subcomponent while reporting all three as complete). Use the regular `ed3d-plan-and-execute:task-implementor` for any subcomponent that bundles two or more tasks. Single-task dispatches and trivial fixes can still use `-fast`.
