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

## Architecture Doc Guidance

An architecture doc's job is to give a reader a mental model of how a system is structured, how it does what it does, and how the pieces fit together. The reader gets oriented; they can then navigate the source with that context in hand.

Architecture docs are **not** a fill-in form. Different components have different shapes. A library crate's doc may have different sections than an executable's. Write what serves comprehension for this particular component.

### What belongs in an architecture doc

**Topology and data-flow prose.** An opening paragraph that explains how the pieces are wired and what passes between them. Keep it tight — the diagram carries the structure.

**Mermaid diagrams.** Reach for a diagram whenever prose would have to hand-wave: crate dependency graphs, webhook-to-WebSocket pipeline flowcharts, schema ER diagrams, state machines, and supervision/shutdown sequences all compress cleanly into Mermaid. One diagram often does what three paragraphs struggle to, without the rot.

**Contract sections.** Any guarantee the source cannot express on its own earns a section: snapshot/stream reconciliation rules, storage-mode invariants, durable-cursor monotonicity, placeholder semantics for out-of-order events. Pair a prose explanation with a Mermaid sequence or table where the protocol or failure behavior matters.

**Schema descriptions.** When a component owns a schema, describe the key columns, relationships, and non-obvious constraints. Pair with an `erDiagram`. Migration mechanics or column-level trivia live in the migration files; the arch doc captures the shape and why it was designed that way.

**Inline ADR links.** When a decision shaped the architecture, link to the ADR inline — at the point in the doc where the reader would want context — rather than collecting decisions into a standalone `## Key Decisions` section. Decisions belong in ADRs; the arch doc describes the result.

**Boundaries (library crates only).** For library crates, a brief Boundaries section can be useful: it declares what the public API owns and explicitly doesn't own, which is information the crate name alone doesn't convey. For executable crates this is usually not useful — the executable owns its routes and nothing else, and what it delegates to is already visible from the crate dependency graph.

### What does not belong in an architecture doc

**Purpose statements and project framing.** The H1 title and the codebase's existence already state what the component is. An introductory paragraph should orient the reader to structure, not restate the obvious.

**Key Decisions sections.** ADRs are the canonical home for decisions with alternatives and full rationale. An arch doc that collects decisions into its own section is a second home for content that already has one.

**Files inventories.** Never enumerate source files. `rustdoc`, IDE outlines, and `ls -la` answer "what files are here" without rot risk. An arch doc that lists files is one refactor away from being wrong.

**Operator Surface sections.** How operators configure or run the component belongs in `docs/architecture/deployment.md` or `docs/operator/`. Architecture docs describe how the system is built; deployment and runbook docs describe how it is run.

**Struct definitions and code dumps.** LSP and `rustdoc` surface these without maintenance burden. Replace a struct dump with a Mermaid data-shape diagram or a one-paragraph description of field roles.

**Metric and span catalogs.** The full catalog lives in `docs/architecture/metrics.md`. The arch doc describes the observability shape and links; it does not enumerate every metric name.

### Timestamp

All docs carry a `Last verified: YYYY-MM-DD` timestamp at the top, updated whenever the doc is reviewed or modified.

### Worked example

`docs/architecture/backend-server.md` demonstrates the shape in practice. It opens with a topology paragraph and a crate dependency graph, then sections for the webhook-to-WebSocket pipeline, config hot-reload, Postgres schema (with an `erDiagram`), snapshot/stream reconciliation protocol (with a sequence diagram), storage-mode invariants (with a startup-behavior table), and supervision/shutdown ordering (with a sequence diagram). It has no Purpose section, no Key Decisions section, no Files inventory, and no Operator Surface section — those jobs are handled elsewhere. Read it before writing a new arch doc.

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

**Tier 2 — Sharp-edges sections.** Added **reactively** when agents encounter friction in that directory — costly mistakes, non-obvious testing gotchas, foot-guns, file-specific guidance. Do not pre-author sharp edges speculatively; let them accrete from observed agent failures. When a CLAUDE.md gets rewritten, re-evaluate every existing Sharp-edges entry: foot-guns whose underlying bug shipped a fix have become noise, and the rewrite is the natural opportunity to drop them. The root `CLAUDE.md` invariant "Slim CLAUDE.md in every domain directory (two-tier)" is the authoritative version of this rule.

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
- [`.github/CLAUDE.md`](../.github/CLAUDE.md) — Tier 1 only after the latest trim. Purpose paragraph naming the directory's role + Key References pointing at the arch docs. Shows that non-crate directories don't automatically need Workflow tables or Contracts sections — when `ci-pipeline.md` and `release-pipeline.md` already cover the contracts, the CLAUDE.md is just a pointer.

When in doubt, copy `atc-wire/CLAUDE.md` as the starting shape and expand only when friction shows up.

## Observability — where the catalog lives

Metrics naming + attribute conventions, span naming + late-bound-field patterns, and the per-metric / per-span catalog itself live in [`docs/architecture/metrics.md`](architecture/metrics.md). The doc-system rule the audit cares about is the authoring contract: **every new metric ships with the seven-element interpretation block; every new span boundary lands in the span inventory; both extensions go in the same commit cluster as the source change.** The doc-staleness gate (`scripts/check-docs.sh`) blocks the push if backend telemetry changes land without the matching `metrics.md` update, so the rule is mechanically enforced rather than convention-only.
