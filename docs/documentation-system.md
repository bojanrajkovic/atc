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

## Drift-Resistance Principles

The doc-system rules above are the *what*. These five principles are the *why* — the failure modes that motivate each rule, surfaced by the 2026-05-22 audit of the ATC doc system.

### Non-duplication applies to itself

The non-duplication rule binds every doc, including the docs about docs. CLAUDE.md does not paraphrase CONTRIBUTING.md; CONTRIBUTING.md does not restate `docs/documentation-system.md`; ADRs do not narrate the architecture docs they decide. Two copies of any rule means one of them is wrong — the audit catches that, but only at audit cadence, and the wrong copy in the meantime is the one a contributor reads first.

### Source-can-answer-it dies

If the source code (or `rustdoc` / JSDoc, or LSP, or `ls -la`) answers the question, the doc doesn't get to. Struct-field dumps in arch docs, component-prop tables in CLAUDE.md, Files-section directory listings, animation timing catalogs — these all rot the moment the source moves, and the source moves faster than the doc gets re-read. Prefer a Mermaid diagram of the *shape*, a one-paragraph summary of the *roles*, or a pointer to the canonical type.

### Size is diagnostic

A 20 KB CLAUDE.md is a symptom of duplication or inventory growth, not a goal in itself. Don't budget size; budget *what's in the file*. When a slim CLAUDE.md outgrows two pages, the right move is usually "extract the catalog content to its dedicated home" rather than "split the CLAUDE.md by section." Size targets miss the actual concern; size measurements expose it.

### Audit sharp edges

Every time a CLAUDE.md sharp-edges section gets rewritten, re-evaluate every entry: is the foot-gun still load-bearing, or did the problem ship a fix? Sharp edges accumulated reactively are good; sharp edges that outlive the bug they warned about are noise. The rewrite is the audit opportunity — take it.

### Atomic commits

Each logical change is one commit, even when a sub-phase contains many of them. A reviewer should be able to scan a commit's diff in seconds and understand what landed. PR-tab summaries describe what shipped *overall*; the commit history is what someone running `git blame` will read months later.

## Observability

ATC exports metrics and spans through one OpenTelemetry pipeline. When `OTEL_EXPORTER_OTLP_ENDPOINT` is set, the SDK initializes and pushes OTLP/HTTP to the configured collector; with the env var unset, no provider, exporter, or background task is initialized. Two contributor-facing rules apply when adding or modifying observability surfaces.

**Naming and attribute conventions** (metrics and spans):
- `atc_` project prefix on every metric; snake_case names; `_total` for counters; `_seconds` for time-valued; `_bytes` for byte-valued.
- Lowercase keys for metric attributes; no high-cardinality values; no PII; no replica/pod labels (target attributes are injected at the collector).
- Span names use a dotted hierarchy that names the boundary (`webhook.handler`, `persist.apply.run_event`, `drain.broadcast`). Late-bound span attributes use `tracing::field::Empty` at construction and `Span::current().record(...)` once the value is known.

**Authoring contract:** every new metric ships with the seven-element interpretation block (name, type, attributes with source, semantics, per-replica scope, aggregation guidance, example PromQL); every new span boundary lands in the span inventory. Both surfaces are canonically documented in [`docs/architecture/metrics.md`](architecture/metrics.md) § "Metric and span authoring contract" — this section codifies the rule that contributors who add either MUST extend that doc before merge. The doc-staleness gate (`scripts/check-docs.sh`) blocks the push if backend telemetry changes land without the matching `metrics.md` update.
