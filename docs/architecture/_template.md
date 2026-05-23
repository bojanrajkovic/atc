# [Replace with module / topic title]

Last verified: [YYYY-MM-DD — set when first written; update on every later modification]

> _Status line. Examples: "Living document. Updated whenever the X module evolves." or "Stable contract; revisit on major refactor." Delete this blockquote and replace with the real status line._

## Purpose

> _Author guidance — delete after filling in. One paragraph answering "why does this exist as a separate concern?" The audience is someone debugging six months from now: assume they can read code but don't yet know the design intent._
>
> _**If you're tempted to add an integration recipe ("how to use this in your service") or a feature list, stop** — that content belongs in an operator runbook or `CONTRIBUTING.md`, not here. Purpose is the *why*; how-to is the *how*, and they have different homes._

[Replace this line with the real Purpose paragraph.]

## Key Decisions

> _Author guidance — delete after filling in. For each significant architectural choice, write a Decision / Alternatives considered / Rationale block. The format makes the choice and its trade-offs scannable._
>
> _**If a decision needs ≥3 paragraphs of alternatives or context, promote it to an ADR** in `docs/architecture-decisions/NNN-<title>.md`. Replace the inline block with a one-line summary that links to the ADR. The arch doc holds the *current* state; the ADR holds the *decision* with its full history._

**Decision:** [Replace]
**Alternatives considered:** [Replace]
**Rationale:** [Replace]

## Boundaries

> _Author guidance — delete after filling in. Boundaries answers "what is this component *not* responsible for, that the reader might mistakenly think it is." Owns / Does not own / Prohibitions cover the three angles._
>
> _**If you're tempted to add a `uses` or `consumes` list, stop** — that's a dependency relationship, which is Architecture content, not Boundaries. Boundaries is about responsibility, not data flow._

**Owns:** [Replace]
**Does not own:** [Replace]
**Prohibitions:** [Replace — e.g., "Never store session state in the WebSocket handler. Sessions rebuild on reconnect, so cached state would diverge silently."]

## Files

> _Author guidance — delete after filling in. 3–5 entries naming the source files this doc canonically covers, each with a one-line description of role._
>
> _**If the list is growing past 5 entries with deep paths, the doc is probably covering too much — split it.** This section is a roadmap for new readers, not an inventory; `rustdoc`, IDE outlines, and `ls -la` answer "what's in this directory" with no rot risk._

- `path/to/file.rs` — [single-clause description of role]

## [Optional] Architecture / Data Model / Schema / Contracts / Invariants

> _Author guidance — delete after filling in. Use whichever subsections fit the topic. Each one is optional; keep only the ones that earn their keep for this particular component._
>
> _Mermaid diagrams shine where topology, sequence, or state machines are non-obvious. A single diagram often does what three paragraphs of prose try to do, without the rot._
>
> _**If you're tempted to paste a struct definition or component-prop interface block, stop** — LSP and `rustdoc` / JSDoc surface those without rot risk. Replace the dump with a Mermaid diagram of the data shape, or a one-paragraph summary of the field roles. The struct definition will outlive any prose copy of it._
>
> _**If you're tempted to enumerate every metric name, every span attribute, every animation timing, or every component prop, stop** — that catalog belongs in its dedicated ops doc (e.g., `docs/operator/metric-interpretation-guide.md` for metrics). The architecture doc summarizes the *shape* and links to the catalog._

[Replace with the real subsections that fit this component.]

## [Optional] Operator Surface

> _Author guidance — delete after filling in. If this component exposes operator-facing configuration, env vars, runtime knobs, or alarms, this section names them at the *concept* level and points at the operator runbook for procedures._
>
> _**If you're tempted to write a configuration recipe ("set X to Y for use case Z") here, stop** — recipes are how-to and belong in `docs/operator/`. Link to the runbook from here instead._

[Replace with concept-level operator surface, or delete this section.]

---

## How to use this template

1. Copy this file to `docs/architecture/<topic>.md` (do not modify `_template.md` itself).
2. Replace `[Replace with module / topic title]` with the topic title.
3. Set `Last verified:` to today's date in ISO format (YYYY-MM-DD).
4. Delete the optional sections that don't apply.
5. Replace each `[Replace]` placeholder with real content.
6. **Delete each `> _Author guidance — delete after filling in._` blockquote** after using it to decide what content goes in the section. The blockquote is for the author at write-time; published docs shouldn't carry it.
7. Add a mapping in `scripts/doc-mapping.yaml` from the source files this doc canonically covers to the new doc's path.
8. Update `Last verified:` whenever the doc is later reviewed or modified.
