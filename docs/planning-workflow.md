# Planning Workflow

Last verified: 2026-05-04

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

**Use researcher subagents for codebase investigation.** Don't read source files inline in the planning context — dispatch a codebase-investigator or combined-researcher subagent to answer specific questions and return a summary. This keeps the planning context lean for the conversation ahead.

### 2. Clarification

Resolve ambiguity before designing. For any non-trivial feature, identify and resolve:
- Technical term ambiguity (e.g., "auth" → authentication or authorization?)
- Scope boundaries (e.g., "users" → human users, service accounts, or both?)
- Version and API assumptions (e.g., "integrate with X" → which version?)
- Constraint origins (e.g., "must use Y" → regulatory? performance? preference?)

Use codebase investigation and external research to resolve unknowns directly. Ask the user only for things that cannot be looked up.

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

Write the full design plan to the file created in Phase 3. Required sections:
- Summary
- Definition of Done (carried from Phase 3)
- Architecture (with rejected alternatives and rationale)
- Implementation Phases (≤8; phases are checkpoints, not padding targets — do not add phases to reach 8)
- Acceptance Criteria (success and failure cases for each DoD item)
- Documents to Update (see below)
- Implementation Guidance: `docs/implementation-guidance.md` governs all implementation work for this plan
- Glossary

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
