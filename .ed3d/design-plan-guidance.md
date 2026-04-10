# Design Plan Guidance

Architectural principles and conventions that affect brainstorming and design decisions for the ATC project.

## Principles

1. **Phases evolve justfile recipes** — stubs become real implementations as code lands. Each phase updates only the recipes relevant to its deliverables.

2. **Lefthook hooks are pre-configured** — new phases should NOT need to modify `lefthook.yml` unless adding an entirely new tool category.

3. **Module-level CLAUDE.md files are directive extracts** — only for high-risk directories. Always reference canonical source in `docs/architecture/`. Derive from architecture docs when agents encounter sharp edges — do not pre-create speculatively.

4. **Non-duplication rule** — each piece of information has exactly one home across the five documentation layers. CLAUDE.md and README.md point to docs/, they don't duplicate content.

5. **Architecture docs use the required-anchor template** — Purpose, Key Decisions (with rejected alternatives), Boundaries, Files, plus module-specific sections. All carry a "Last verified: YYYY-MM-DD" timestamp.

6. **Design plans include a "Documents to Update" table** — before coding, list every architecture doc, CLAUDE.md, and skill file that must change alongside the implementation.

7. **ADRs carry retroactive annotations** — when creating an ADR, annotate all existing documents that describe superseded behavior with `> **Revised by ADR-NNN:** ...`

8. **PR titles reflect the implementation, not the design doc** — this repo uses squash merges, so the PR title becomes the commit message on main. Title the PR for what the branch will deliver when complete (e.g., `feat: add core domain model and in-memory state store`), not for the first commit on the branch (e.g., `docs: add core domain design plan`). The design doc is just the first step; the PR carries the full implementation.
