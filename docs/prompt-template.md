## Prompt Structure for Multi-Phase Features

This is a prompt template for "we've wrapped up a phase of a large feature, let's write the prompt to start the next phase."

The user will use this prompt when entering plan mode to continue work from a prior phase. Claude will read the four sections
here during Phase 1 (Context Gathering) and Phase 3 (Definition of Done) to anchor the design correctly.

---

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
