# Documentation System

Last verified: 2026-05-23

## Six-Layer Documentation Model

Each piece of information has exactly one home (**non-duplication rule**). The six layers are:

| Layer | Location | Purpose | Audience |
|-------|----------|---------|----------|
| Architecture docs | `docs/architecture/` | Canonical source of truth for implemented features — *why* and *what*: design, decisions, contracts, current-state truth | All |
| Operator runbooks | `docs/operator/` | How-to content for the production surface around ATC — auth-proxy recipes, integration patterns, upgrade procedures, troubleshooting. *How*, not *why* | Operators |
| Contributing guide | `CONTRIBUTING.md` | Human workflows, conventions, setup | Human developers |
| AI agent index | `CLAUDE.md` (root) | Compact pointers, commands, project status | AI agents |
| Directive extracts | `<subdir>/CLAUDE.md` | Sharp-edge warnings for high-risk directories | AI agents |
| Ideation docs | `docs/ideation/` | Living documents for planned-but-unbuilt features | All |

**Architecture vs operator runbook split:** architecture docs describe how ATC's own internals work and why they were designed that way (`docs/architecture/deployment.md` explains why we picked RollingUpdate, why the `preStop sleep` matters, what the multi-replica invariants are). Operator runbooks describe how to operate ATC against external infrastructure that isn't part of ATC itself (`docs/operator/authentication.md` shows how to wire Pomerium / oauth2-proxy / etc. in front of it). When a runbook needs to reference an architectural decision, it links rather than restating.

**Non-duplication rule:** Do not copy content between layers. CLAUDE.md points to docs/ — it doesn't summarize them. README.md links to CONTRIBUTING.md — it doesn't repeat setup instructions. Architecture docs link to operator runbooks for how-to content rather than embedding recipes. When information changes, it changes in one place.

**When a feature ships:** The ideation doc archives (add "Shipped — see `docs/architecture/<topic>.md`" header) and the architecture doc becomes the source of truth.

## Architecture Doc Template

Every architecture doc in `docs/architecture/` must include these four required anchor sections:

### Purpose
What this component does and why it exists.

### Key Decisions
Architectural choices with rejected alternatives and rationale. Format:

```markdown
### Key Decisions

**Decision:** Use WebSockets for real-time updates
**Alternatives considered:** SSE (Server-Sent Events), long polling
**Rationale:** Bidirectional communication needed for future interactive features. SSE is receive-only.
```

### Boundaries
What this component owns, what it does NOT own, and explicit prohibitions.

```markdown
### Boundaries

**Owns:** WebSocket connection lifecycle, message serialization, reconnection logic
**Does not own:** Authentication (handled by auth middleware), business logic (handled by domain services)
**Prohibitions:** Never store session state in the WebSocket handler. Never bypass auth middleware.
```

### Files
Which source files this doc covers.

```markdown
### Files

- `backend/src/ws/mod.rs` — WebSocket handler and connection manager
- `backend/src/ws/messages.rs` — Message types and serialization
- `frontend/src/lib/ws.ts` — Client-side WebSocket wrapper
```

### Additional Sections
Module-specific middle sections as needed: Architecture, Data Model, Schema, Contracts, Invariants.

### Timestamp
All docs carry a `Last verified: YYYY-MM-DD` timestamp at the top, updated whenever the doc is reviewed or modified.

### Fillable template

A copy-and-fill template at [`docs/architecture/_template.md`](architecture/_template.md) embeds the required structure with anti-rationalization placeholders at the temptation points where authors tend to drift — pasting struct definitions, enumerating every metric name, embedding integration recipes, or growing the Files section into a directory listing. Each section's author-guidance blockquote names the canonical home for the content that doesn't belong, and instructs the author to delete the guidance after filling in real content.

When creating a new arch doc, copy the template (not modify it in place) into `docs/architecture/<topic>.md`, fill in the placeholders, then delete the `> _Author guidance — delete after filling in._` blockquotes so the published doc carries only the real content.

## ADR Convention

Architecture Decision Records live in `docs/architecture-decisions/`. When a significant architectural decision is made or changed:

1. Create a new ADR file: `docs/architecture-decisions/NNN-<title>.md`
2. Include: context, decision, consequences, alternatives considered
3. If the ADR supersedes behavior described in existing documents, annotate those documents:

```markdown
> **Revised by ADR-NNN:** [Brief description of what changed]. See `docs/architecture-decisions/NNN-<title>.md` for full context.
```

This keeps historical documents readable while marking what changed.

## Updating SQL Queries

All SQL queries in `atc-server` use the `sqlx::query!` / `sqlx::query_as!` macros for compile-time type checking. The Cargo workspace root is `backend/`, and the offline query cache lives at `backend/.sqlx/` — committed to the repository so CI can build without a live database.

**When to regenerate the cache:** any time you add, remove, or change a `query!` / `query_as!` macro call, or modify a migration in `backend/crates/atc-server/migrations/`.

**How to regenerate:**

1. Start a local Postgres with migrations applied:
   ```bash
   docker run -d --rm --name atc-pg -e POSTGRES_PASSWORD=postgres -p 5432:5432 postgres:17-alpine
   DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
     cargo sqlx migrate run --source backend/crates/atc-server/migrations
   ```

2. Regenerate the cache from the **`backend/` directory** (the Cargo workspace root):
   ```bash
   cd backend
   DATABASE_URL="postgres://postgres:postgres@127.0.0.1:5432/postgres" \
     cargo sqlx prepare --workspace -- --tests
   ```
   The `--tests` flag includes queries in `#[cfg(test)]` code. `--workspace` writes the cache to `backend/.sqlx/`.

3. Commit the updated `backend/.sqlx/` files in the **same commit** as the SQL change:
   ```bash
   git add backend/.sqlx/
   git commit -m "feat(server): <description of SQL change>"
   ```

**Why CI doesn't need `DATABASE_URL`:** sqlx 0.8 automatically uses the committed `backend/.sqlx/` offline cache when no `DATABASE_URL` is set in the build environment. The existing `.github/workflows/ci.yml` requires no changes.

## Planning-Artifact Labels

Design plans, ADRs, and implementation tickets use numbering schemes to coordinate work — phases (`Phase 2c`, `Sub-Phase 4`), acceptance criteria (`AC2.1`, `AC10.3`), test sequences (`T1`, `T6b`, `T11`), and bare ADR refs (`per ADR-0005`). These belong in the **historical record only**: ADRs, design plans, ideation, commit messages, CHANGELOG, and the `Last verified:` line at the top of CLAUDE.md / AGENTS.md files. They must NOT survive into current-state artifacts.

**Why:** a future maintainer reading a test failure or scanning a comment will not have the design plan in their head. A test name like `t11_concurrent_same_entity_commits_in_seq_order` or a comment `// AC6.7: reconnect silence during buffered drain` couples the code to a planning document and adds nothing the behavioral text alone does not already say.

**Strip from:**

- **Test function and file names** — `phase_NX_*`, `ac<N>_<M>_*`, and `t<N>[a-z]?_*` prefixes. Rename to describe the invariant being verified.
- **Code comments** — module docs, doc comments on items, inline comments, section banners (e.g., `// ===== ... (AC5.1–AC5.4) =====`). Describe what the code does, not which planning artifact it satisfies.
- **Test report labels** — `describe(...)` / `test.describe(...)` strings. Behavioral text after the prefix usually already exists; preserve it and drop the tag.
- **Module-level docstrings that enumerate test cases** (e.g., `T1 — does X / T2 — does Y`) — rewrite as a description of what the file covers as a whole, not a numbered list.
- **User-visible strings** — chart-time `{{ fail }}`, `tracing::error!`, `NOTES.txt`, README, Prometheus metric description strings.
- **Architecture docs and CLAUDE.md / AGENTS.md** — these describe what IS, not what HAS BEEN. Planning-artifact references almost always live inside changelog-flavored content that itself doesn't belong; trim the content, not just the labels.

**Keep in:**

- ADRs (`docs/architecture-decisions/`), design plans (`docs/design-plans/`), ideation (`docs/ideation/`) — these documents *are* the historical record. Acceptance criteria belong in design plans by design; the numbers are useful inside the plan, they just shouldn't escape into the code.
- Commit messages and CHANGELOG.
- The `Last verified: YYYY-MM-DD (#N closed: …)` line at the top of CLAUDE.md / AGENTS.md (authorship metadata).
- Captured external history — e.g., webhook fixture commit messages from real GitHub Actions output (data, not authored content).
- Definitional references that explain what the term means rather than using it (e.g., the `(AC1, AC2, …)` parenthetical in `docs/planning-workflow.md`).

**Audit hint:** when stripping one class, sweep the others at the same time. The starter regex `rg 'Phase \d|AC[0-9]|\bT[0-9]+[a-z]?\b|fn (phase|ac|t)[0-9]'` catches all four common patterns; tune as new schemes appear.

**The pattern:** ask "is this artifact part of the current contract / current state, or part of the historical record?" If current, strip the planning-artifact reference (and probably the surrounding sentence — these refs usually accompany changelog narration that doesn't belong in a current-state doc). If historical, keep.

## Terminology

Pick the right voice when writing about people:

- **Author** — the codebase developer (currently Bojan). Used in design plans, PR bodies, and any doc that describes development-time perspective.
- **Operator** — anyone (the author or a third party) deploying ATC from the published chart. Used in deployment / runbook / Helm-chart voice.
- **User** — the end-user of the deployed app, interacting with the ATC frontend or sending webhooks from a GitHub repo. Reserve for that voice.

Mixing these makes it unclear which voice is speaking. The literal "user" in a Claude Code conversation transcript (the live conversational party) is a separate, acceptable usage in agent prompts and memory entries.

## Committed Design Plans

Design plans in `docs/design-plans/` are committed as final documents. Future readers see only the committed file, not the conversation that produced it. When condensing toward commit, strip:

- References to drafts — "previous draft", "earlier version of this plan", "the draft I started with"
- Review-tool citations — "codex blocker #1", "the reviewer flagged", "see review concern #6"
- Decision-process narrative — "we initially proposed X, then realized Y". Keep the *current* rationale, not the path that produced it.

Exception: citing a *committed* document (rollout doc, ADR, prior design plan) by file path is fine — the reader can find it. The test is: can a future reader follow this citation to something concrete in the repo?

## Directory-Level CLAUDE.md Files

Every subdirectory that represents a distinct domain (crates, frontend, helm chart, `.github`, etc.) gets a slim `CLAUDE.md`. The file follows a **two-tier** structure:

**Tier 1 — Mandatory skeleton.** Created when the directory is established. Required content:

- Purpose statement — one or two sentences naming what this domain owns.
- Pointer to the canonical architecture doc(s) in `docs/architecture/` — never duplicate doc content.
- An `AGENTS.md` symlink (`ln -s CLAUDE.md AGENTS.md`) in the same directory; both files together or neither.
- Reference its canonical source where relevant:

  ```markdown
  <!-- Derived from docs/architecture/<topic>.md -->
  ```

**Tier 2 — Sharp-edges sections.** Added **reactively** when agents encounter friction in that directory — costly mistakes, non-obvious testing gotchas, foot-guns, file-specific guidance. Do not pre-author sharp edges speculatively; let them accrete from observed agent failures. The root `CLAUDE.md` invariant "Slim CLAUDE.md in every domain directory (two-tier)" is the authoritative version of this rule.

### Template

A new directory-level CLAUDE.md starts at Tier 1 only:

```markdown
# CLAUDE.md — <domain-name>

Last verified: YYYY-MM-DD

> Canonical documentation lives in `docs/architecture/<topic>.md`. This file provides domain-specific guidance for agents working here. Do not duplicate content from the architecture doc.

## Purpose

<One or two sentences naming what this domain owns. Identify the canonical
architecture doc by file path; downstream readers follow the link rather
than reading a duplicated summary.>

## Key References

- Architecture: `docs/architecture/<topic>.md`
- Related ADRs (if any): `docs/architecture-decisions/NNNN-<title>.md`
```

Tier 2 appends a `## Sharp edges` (or `## Testing` / `## Contracts` / domain-fit heading) section the first time an agent hits friction in that directory. Each entry names one foot-gun in one sentence, then optionally explains the *why* — agents that read the rule but skip the rationale need to know the rationale is there if they want to deviate.

### Exemplars

Three current CLAUDE.md files demonstrate the pattern at different sizes:

- [`backend/crates/atc-wire/CLAUDE.md`](../backend/crates/atc-wire/CLAUDE.md) — minimal Tier-1-only file. Header pointer, Purpose, Key References. ~15 lines. Good template for a stable crate with no friction yet.
- [`backend/crates/atc-persist/CLAUDE.md`](../backend/crates/atc-persist/CLAUDE.md) — Tier 1 + Tier 2 (Sharp edges). Three foot-guns: dependency hygiene of the trait crate, tokio-feature constraints, why tracing is a hard dep. Each entry one short paragraph with the *why*. ~25 lines.
- [`.github/CLAUDE.md`](../.github/CLAUDE.md) — domain-specific shape: a workflow table, then a Contracts list (path filtering, helm sweep, linked versions). Shows that Tier 1 / Tier 2 are content categories, not rigid section names — non-crate directories can structure differently while keeping the same slim discipline.

When in doubt, copy `atc-wire/CLAUDE.md` as the starting shape and expand only when friction shows up.

## Observability

ATC exports metrics and spans through one OpenTelemetry pipeline. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, the SDK initializes and pushes OTLP/HTTP to the configured collector; with the env var unset, no provider, exporter, or background task is initialized. Two contributor-facing rules apply when adding or modifying observability surfaces.

**Naming and attribute conventions** (metrics and spans):
- `atc_` project prefix on every metric; snake_case names; `_total` for counters; `_seconds` for time-valued; `_bytes` for byte-valued.
- Lowercase keys for metric attributes; no high-cardinality values; no PII; no replica/pod labels (target attributes are injected at the collector).
- Span names use a dotted hierarchy that names the boundary (`webhook.handler`, `persist.apply.run_event`, `drain.broadcast`). Late-bound span attributes use `tracing::field::Empty` at construction and `Span::current().record(...)` once the value is known.

**Authoring contract:** every new metric ships with the seven-element interpretation block (name, type, attributes with source, semantics, per-replica scope, aggregation guidance, example PromQL); every new span boundary lands in the span inventory. Both surfaces are canonically documented in [`docs/architecture/metrics.md`](architecture/metrics.md) § "Metric and span authoring contract" — this section codifies the rule that contributors who add either MUST extend that doc before merge. The doc-staleness gate (`scripts/check-docs.sh`) blocks the push if backend telemetry changes land without the matching `metrics.md` update.
