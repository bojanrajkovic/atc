# Planning Workflow

Last verified: 2026-05-06 (lessons folded in from Phase 4 planning session: coupling-site enumeration in Phase 1, AskUserQuestion evidence rule in Phase 2, required-sections list in Phase 5, new Phase 5.5 plan review)

## Purpose

This document defines Claude Code's **Plan Mode behavior** for this project. When entering plan mode to design or plan a feature, follow the phases below. The goal is to produce a committed design plan that a fresh implementation context can execute from.

For execution-time rules (after the plan is approved and context is cleared), see [`docs/implementation-guidance.md`](implementation-guidance.md).

## Workflow Phases

Feature design follows six phases in order. Each has an explicit gate before the next.

### 1. Context Gathering

Collect before designing anything:
- What you're building, and why
- Goals or success criteria
- Known constraints or requirements
- Relevant code paths, architecture docs, or prior decisions already in place

If extending existing functionality, identify which patterns apply and which architectural decisions are already locked in.

**Use researcher subagents for codebase investigation.** Don't read source files inline in the planning context — dispatch a research subagent to answer specific questions and return a summary. This keeps the planning context lean for the conversation ahead.

Agent preference, in order:

1. **Project-specific researcher agents if available** (`ed3d-research-agents:codebase-investigator`, `ed3d-research-agents:combined-researcher`, `ed3d-research-agents:internet-researcher`, `ed3d-research-agents:remote-code-researcher`). These have system prompts tuned for design-time investigation and return summaries calibrated to planning needs.
2. **Fall back to the built-in `Explore` agent** when the project-specific agents are not installed in this environment. `Explore` is always available; it's a generic read-only search agent that locates code without the deeper analytical scaffolding of the project-specific researchers, but it's sufficient for most file/symbol/keyword lookups.

**Coupling-site enumeration.** When a plan removes or renames a file, symbol, or values key, the Explore prompt MUST explicitly enumerate every coupling surface to inspect — not just the obvious source tree. Researcher agents return what they were asked about; ask explicitly. The standard checklist:

- Chart-internal docs (chart's `README.md`, `NOTES.txt`, in-chart `CLAUDE.md`)
- CI workflow files (`.github/workflows/*.yml`) — grep for filenames by exact string, not pattern
- `justfile` / `package.json` recipes — verify which recipes actually run on the changed surface; don't cite recipes from memory (per `feedback_verify_just_recipes_before_citing.md`)
- `scripts/doc-mapping.sh` entries — does the change cross a doc-staleness boundary that needs a new mapping?
- ADR cross-references — does any ADR cite the removed surface by name?
- Existing tests, fixtures, snapshot files — exact-string grep, not pattern grep

Pre-listing these in the prompt's Canonical Context block (see "Prompt Structure" below) is preferred over relying on the researcher to discover them.

### 2. Clarification

Resolve ambiguity before designing. For any non-trivial feature, identify and resolve:
- Technical term ambiguity (e.g., "auth" → authentication or authorization?)
- Scope boundaries (e.g., "users" → human users, service accounts, or both?)
- Version and API assumptions (e.g., "integrate with X" → which version?)
- Constraint origins (e.g., "must use Y" → regulatory? performance? preference?)

Use codebase investigation and external research to resolve unknowns directly. Ask the user only for things that cannot be looked up.

**Reserve `AskUserQuestion` for genuine preference choices.** Do not escalate facts that are answerable in under five minutes via grep, `gh issue list`, or a focused Explore. Before marking any option `**(Recommended)**`, gather and cite the evidence the recommendation turns on — name the grep, the issue search, or the file:line reference inside the option's description. If the recommendation depends on a fact you haven't verified, drop the rank: present options unranked or as cost/benefit pairs. The `**(Recommended)**` mark is a claim about the world, not a conservatism hedge.

### 3. Definition of Done

Lock in the *what* before brainstorming the *how*. Confirm explicitly:
- Primary deliverable(s)
- Success criteria — how you will know it's done
- Key exclusions (anything the user or prior context has put out of scope)

**Do not proceed to brainstorming until DoD is confirmed.** Brainstorming that begins without a fixed goal explores the wrong solution space.

Once DoD is confirmed, create the design plan file at `~/.claude/plans/YYYY-MM-DD-{slug}.md` — Plan Mode restricts writes to that directory. The file will be copied into the project in Phase 6.

### 4. Brainstorming

Propose 2–3 architectural alternatives. For each, identify: the approach, the hazards, and fit against existing codebase patterns. Use codebase investigation to verify assumptions; use external research for library comparisons or API designs. Validate the chosen approach before writing the full design document.

### 5. Design Documentation

Write the full design plan to the file created in Phase 3. Required sections, in this order:

1. **Context** — what's true in the repo today, what changed since the prior phase, why this work matters now. Distinct from Summary; this is the load-bearing situational read that an implementation agent uses to orient.
2. **Definition of Done** — carried from Phase 3.
3. **Locked Decisions** — decisions established in prior phases, ADRs, or earlier in the planning session that are NOT open for re-evaluation. Cite source by file path.
4. **Architecture** — design decisions with rejected alternatives and rationale; include file:line citations where relevant.
5. **Implementation Phases** — TDD-ordered (≤8; phases are checkpoints, not padding targets — do not add phases to reach 8). Step 1 should be "write failing tests"; Step 2 should be "make them pass."
6. **Acceptance Criteria** — success AND failure cases for each DoD item, numbered (AC1, AC2, …) so the implementation context can check them off.
7. **Documents to Update** — every architecture doc, `CLAUDE.md`, and `scripts/doc-mapping.sh` entry that must change alongside the implementation, with the specific change.
8. **Implementation Guidance** — explicitly call out which rules from `docs/implementation-guidance.md` apply to this plan, and any project-memory feedback files (`feedback_*.md`) that bite for this scope. The opening blockquote pointing readers at `implementation-guidance.md` is not a substitute for this section.
9. **Out of Scope** — explicitly deferred items, with the issue/phase number that owns each.
10. **Glossary** — if the plan introduces or relies on non-obvious terminology.

A "Summary" section is optional; Context usually carries it. If included, keep it to a paragraph.

### 5.5. Plan Review

Before handing off, run two gates against the plan file:

**Self-consistency check** (always required, takes seconds). Run every grep-based or string-match acceptance criterion in the plan against the plan file itself. If the plan defines an AC of the form `git grep "X" returns zero hits in Y`, verify the plan file does not contain `X` in a position that would land in `Y` after implementation — e.g., replacement-copy snippets, code blocks meant to ship verbatim into a file under `Y`. Self-defeating ACs are a recurring class of bug, and the 10-second grep is cheaper than a multi-minute external review round-trip.

**External codex review** (required for non-trivial plans). Plans with multi-file edit sets, ADR-coupled changes, or operational/deployment-surface changes MUST go through a codex `xhigh` review before exiting plan mode. Single-file fixes and doc-only edits MAY skip.

A passing review satisfies these principles:

1. **Use `codex exec` with a custom prompt.** A custom prompt targets the risk surface this plan introduces. The generic `codex review` template applies a fixed checklist that rarely overlaps with what's actually risky here.
2. **Declare executor context in the prompt.** State that the implementation will be executed by an AI agent with access to `feedback_*.md` memories, agent tooling (`project-claude-librarian`, `codebase-investigator`, etc.), and `CLAUDE.md`/`AGENTS.md` files. Without this, codex flags those resources as missing references and produces false-positive blockers.
3. **Name specific scrutiny points** (typically 10–15) that target concrete mechanisms, ACs, or design choices in this plan. Generic "review for quality" produces generic output; specific questions like "does the crossfade fallback work under burst?" surface real bugs.
4. **Require tiered, structured output** (Blockers / Important / Minor / Strengths). Tiering lets triage take minutes; an undifferentiated essay burns the whole review cycle on parsing.
5. **Constrain the reviewer to reviewing.** Forbid redesigning, restating the plan, hedging ("consider maybe…"), and flagging agent tooling as unavailable. Without these constraints, codex defaults to long, exploratory essays.
6. **Run with `xhigh` reasoning** (set in `~/.codex/config.toml` — there is no CLI flag), sandboxed read-only, with output captured to a unique temp directory per run so prior artifacts don't leak in.

When codex returns findings:

- **Verify each blocker against the codebase before applying fixes.** Codex can be wrong about file paths, line numbers, or whether a file exists at all.
- **Discount findings that flag agent-resolved resources** — `feedback_*.md`, `CLAUDE.md`/`AGENTS.md`, agent tooling — as missing references. Those resolve at runtime for the AI executor.
- **Re-run the self-consistency check** after applying fixes — fixes can introduce new contradictions.

Reusable prompt scaffolds satisfying these principles are kept by maintainers as personal scratch — they're one valid implementation, not the contract. Any prompt that satisfies the principles above passes the gate.

### 6. Finalize and Hand Off

Copy the plan file from `~/.claude/plans/YYYY-MM-DD-{slug}.md` into the project at `docs/design-plans/YYYY-MM-DD-{slug}.md`, then commit it to the feature branch. This is the artifact the implementation context will read from and the commit that makes the branch concrete.

**The handoff is triggered by the user.** When the user says "clear context and bypass permissions," that is the explicit signal to exit plan mode. They will then start a new context in bypass-permissions mode and begin implementing from the committed plan. At that point, `docs/implementation-guidance.md` governs behavior — not this document.

Do not attempt to begin implementation within the planning context.

---

## Prompt Structure for Multi-Phase Features

When entering plan mode to continue work from a prior phase, structure the prompt with these four sections. Claude will read them during Phase 1 (Context Gathering) and Phase 3 (Definition of Done) to anchor the design correctly.

### Locked Decisions

List decisions already committed in prior phases that are **not open for re-evaluation**. Prevents brainstorming from re-litigating settled choices.

```
**Established from Phase N (PR #NN — do not re-open these decisions):**
- PG client: sqlx 0.8 with compile-time query verification
- Schema: `runs` and `jobs` tables with BIGSERIAL surrogate keys
- Write path: predicated UPSERTs via ON CONFLICT DO UPDATE
```

### Open Decisions

Number the questions that brainstorming should actually resolve. These become the agenda for Phase 4.

```
**Open decisions for Phase N+1 brainstorming:**
1. Write placement — emit from webhook handler or outbox worker?
2. Error handling policy — fail-open vs. fail-closed on outbox insert failure?
3. Cursor strategy — BIGSERIAL offset vs. timestamp-based?
```

### Canonical Context

File paths that provide ground-truth for this feature. Claude reads these during Phase 1 rather than searching the codebase blind.

```
**Canonical context:**
- ADR 0002 — docs/architecture-decisions/0002_outbox.sql
- Rollout doc — docs/architecture-decisions/plan-phase-2b-shadow-current-state-writes-of-the-state.md
- Migration — backend/crates/atc-server/migrations/0002_outbox.sql
- State store — backend/crates/atc-core/src/store/mod.rs
```

### Out of Scope

What is explicitly deferred to a later phase. Phase boundaries are easy to blur — naming them prevents scope creep.

```
**Out of scope for this sub-phase:**
- NOTIFY emission (Phase 2d)
- Frontend subscription to live updates (Phase 3)
- Backfill of historical runs (separate initiative)
```

---

## Design Conventions

### Documents to Update

Every design plan must include a "Documents to Update" section listing every architecture doc, `CLAUDE.md`, and `scripts/doc-mapping.sh` entry that must change alongside the implementation. Fill this in before coding begins.

### Branch and PR Workflow

Design docs go on a feature branch, not `main`. The implementation lands on the same branch. This repo uses squash merges — title the PR for the full deliverable (e.g., `feat: add core domain model`), not for the design commit (e.g., `docs: add design plan`).

### ADR Annotation Sweep

When creating an ADR, sweep **all** existing documents, tests, and code comments for superseded behavior — not only the docs you authored. Annotate each location with:

```markdown
> **Revised by ADR-NNN:** [Brief description]. See `docs/architecture-decisions/NNN-<title>.md`.
```

### Justfile Evolution

`just` recipes evolve as code lands. Each phase should update only the recipes relevant to its deliverables. Do not pre-stub commands for functionality that doesn't exist yet.

---

## Cross-References

These conventions are defined in `CONTRIBUTING.md > Documentation Conventions` and apply to design work without exception:
- **Non-duplication rule** — each piece of information has exactly one home across the five documentation layers
- **Architecture doc template** — required anchor sections (Purpose, Key Decisions, Boundaries, Files) with timestamps
- **Directory-level CLAUDE.md files** — created only for high-risk directories; always reference canonical architecture docs
