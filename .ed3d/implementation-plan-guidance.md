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

12. **Implementor + bug-fixer dispatches require model override and memory injection** — `task-implementor-fast` and `task-bug-fixer` both default to Haiku 4.5 in their frontmatter; `code-reviewer` and `test-analyst` default to Opus. Running write agents on Haiku while reviewing on Opus produces a chronic asymmetry: every review cycle finds a fresh batch of tests-that-pass-but-don't-catch-regressions (tautological tests, `if (el)` silent no-ops, AC labels fabricated to match the plan), runtime regressions (e.g., `onchange` vs `oninput` semantics), and shortcut patterns (`(window as any)` casts, bulk lint suppressions). When dispatching either of these agents, the orchestrator must:
    - Pass `model: "sonnet"` on the Agent tool call. (No non-fast `task-implementor` variant exists; Sonnet override is the only knob.)
    - Inject the patterns-to-avoid memory `feedback_subagent_shortcut_patterns.md` as EXTRA_CONTEXT on EVERY dispatch. This memory is the subagent-facing list of recurring shortcuts (`as any` casts, fabricated AC labels, "pre-existing" dismissals, "token budget" bail-out language, missing test tally output, uncommitted artifact leaks) — the subagent enforces these on itself.
    - Inject other relevant project feedback memories from `~/.claude/projects/<project>/memory/` as EXTRA_CONTEXT alongside the patterns memory. Always pass at least `feedback_dont_skip_runtime_verification.md` and `feedback_no_source_grep_tests.md`. Add task-specific memories (e.g., `feedback_no_split_ts_test_files.md` for TS test work, `feedback_exhaustive_switches_at_boundaries.md` for boundary code).

	**How to apply:** when calling the Agent tool with `subagent_type: ed3d-plan-and-execute:task-implementor-fast` or `task-bug-fixer`, set `model: "sonnet"` and prepend the EXTRA_CONTEXT block (read each memory file, paste its body into the prompt) before any task-specific instructions. Build a small reusable text block; do not rely on the agent's own system prompt to load these.

	**Division of responsibility:** the orchestrator's job is to inject the memories once per dispatch and then dispatch the per-phase code-reviewer to catch issues that slipped through. The orchestrator does NOT independently audit every commit, grep every diff for `as any`, or re-verify every test tally — that creates expensive context bloat in the main thread and duplicates the reviewer's job. Trust the memory to constrain the subagent and trust the reviewer to catch what the subagent missed. If the same shortcut pattern keeps slipping past both, the memory needs sharpening, not the orchestrator.
