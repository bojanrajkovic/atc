# ATC Post-Audit Execution Plan

## Context

The 2026-05-22 architecture-documentation audit of ATC produced a final report and a walkthrough decision set. This plan sequences the post-audit cleanup as a **single PR** with **sub-phase checkpoint reviews** on the same branch — atomic commits per logical change, group push after each sub-phase, pause for review, then proceed.

This plan does not include source-code changes beyond what the doc rewrites incidentally touch (`lefthook.yml`, `scripts/`, `helm` chart). The audit was about doc-system hygiene; product features are not in scope.

## Execution principles

- **One PR, atomic commits within it.** Branch from `main`; push after each sub-phase ends; pause for brief review on the existing PR before proceeding. Merge once everything lands.
- **Each logical change is one commit.** ~95 commits expected, each describable in one sentence.
- **No automated gates beyond the cheap ones.** Mechanical lefthook checks (AGENTS.md symlink, staleness grep) earn their keep. Size budgets, ADR-scope checks, CLAUDE.md format gates: language + audit cadence instead.
- **Apply principle #3 (source-can-answer-it dies) uniformly.** Many audit recommendations of "shrink + relabel" sharpen to "shrink + delete."
- **Audit sharp edges during every rewrite** (principle #10): every existing foot-gun gets a load-bearing check during its file's rewrite.

## Branch + PR structure

- Branch: `docs/post-audit-cleanup` (one branch off `main`).
- Single PR titled `docs: post-audit cleanup`.
- After each sub-phase ends: push, comment on the PR with what just landed, pause for review. Resume when review clears.
- Sub-phase letters mark major theme transitions; numbered sub-phases (A, B1, B2, …) mark commit clusters that share enough context to review together.
- Final merge: when sub-phase H closes and the PR has been reviewed end-to-end. Merge strategy: squash (per repo default).

## How to execute

**Subagent delegation for rewrites.** Substantial content rewrites are delegated to subagents, not done by the orchestrator directly. For each non-trivial commit in this plan, the orchestrator:

1. Spawns a subagent (default: `ed3d-basic-agents:sonnet-general-purpose`) with the commit description as the task brief, the source file(s) as input, and references to the relevant template / principles docs.
2. Receives the rewrite as a file edit + summary.
3. Reviews the subagent's output before staging.
4. Stages, commits with the planned commit message, and moves to the next commit in the sub-phase.

**Exceptions — orchestrator handles directly.** Some changes are too mechanical or too small to warrant a subagent:

- Quick-wins (Sub-phase A): single-line fixes, stale-reference removals, layer-count typo corrections.
- `doc-mapping.sh` cleanups (parts of B1): mechanical deletions.
- Any rewrite under ~20 lines net change.

**Review stops at every sub-phase boundary.** STOP after every sub-phase. The orchestrator:

1. Pushes the branch.
2. Reports the sub-phase boundary explicitly (e.g., "Sub-phase B1 complete — N commits pushed, awaiting review").
3. Does NOT proceed to the next sub-phase until the user has reviewed and given explicit go-ahead.

## Sub-phase A: Quick Wins

**Goal:** Close trivial-but-high-value findings before bigger work begins.

Commits:

1. `docs: correct documentation-system layer count to six` — edits covering root `CLAUDE.md` and `planning-workflow.md`. `CONTRIBUTING.md` table already says six and stays canonical.
2. `fix(atc-server): remove pre-ADR-0008 PgStore staleness in CLAUDE.md` — `backend/crates/atc-server/CLAUDE.md` Purpose paragraph.
3. `fix(atc-core): remove pre-ADR-0008 staleness in CLAUDE.md Purpose + Modules` — `backend/crates/atc-core/CLAUDE.md` Purpose paragraph and Modules table `persist` entry.
4. `chore(doc-mapping): delete dead atc-server/src/persist/{pg,listener}.rs entries` — post-ADR-0008 ghosts in `scripts/doc-mapping.sh`.
5. `chore(doc-mapping): delete shipped 'Until issue #169 phase 2' comment` — stale historical comment in `scripts/doc-mapping.sh`.
6. `docs(implementation-guidance): replace rule 16 with pointer to planning-workflow §1` — `docs/implementation-guidance.md` rule 16 becomes one line pointing at `docs/planning-workflow.md` §1.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint A.

## Sub-phase B: Process Foundation

**Goal:** Set up the rails for content rewrites: new doc-mapping infrastructure, lefthook checks, governance doc extraction, template authoring.

### B1: Migrate `doc-mapping.sh` to YAML manifest + driver script

1. `feat(doc-mapping): add scripts/doc-mapping.yaml manifest` — YAML data file with source→arch-doc mappings.
2. `feat(doc-mapping): add scripts/check-docs.sh driver consuming YAML` — small driver script reading the YAML, emitting same lefthook-friendly output as the current bash script.
3. `feat(doc-mapping): add docs/operator/* mapping (precondition for runbook extraction)` — YAML entry for `docs/operator/*` → `docs/architecture/deployment.md` (and friends).
4. `feat(doc-mapping): add no-mapping-required allow-list comment` — explicit allow-list for `docs/architecture-decisions/`, `CONTRIBUTING.md`, root `CLAUDE.md`, `docs/planning-workflow.md`, `docs/implementation-guidance.md`, the script itself.
5. `chore(lefthook): switch pre-push doc-staleness check to scripts/check-docs.sh` — updates `lefthook.yml`.
6. `chore: delete scripts/doc-mapping.sh` — old bash script removed.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint B1.

### B2: Add lefthook staleness grep

1. `feat(lefthook): add staleness pattern grep (warn-only)` — pre-push lefthook entry grepping `until X lands`, `until issue #N ships`, `in-tree until`, `for now` across `CLAUDE.md`, `AGENTS.md`, `docs/architecture-decisions/*.md`. Warn on match; do not block.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint B2.

### B3: Add AGENTS.md symlink check

1. `feat(lefthook): add AGENTS.md symlink check` — pre-push, verifies every `CLAUDE.md` has a matching `AGENTS.md` symlink in the same directory. Block on miss.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint B3.

### B4: Extract `docs/documentation-system.md` from `CONTRIBUTING.md`

1. `docs: create docs/documentation-system.md from CONTRIBUTING Documentation Conventions section` — new file, moved content, no prose changes yet.
2. `docs(contributing): remove extracted Documentation Conventions, add pointer to docs/documentation-system.md` — `CONTRIBUTING.md` slimmed.
3. `docs(claude.md root): point Documentation Framework reference at docs/documentation-system.md` — root `CLAUDE.md` cross-reference update.
4. `docs(documentation-system): add architecture-doc template with rationalization-resistant placeholders` — template authored using `writing-claude-directives` conventions. Section placeholders include explicit warnings at the temptation point (e.g., "If you're tempted to add a metric-name catalog or component-prop list here, stop — that content belongs in the canonical source").
5. `docs(documentation-system): add CLAUDE.md template citing atc-wire / atc-persist / .github as exemplars` — replaces current `atc-core` exemplar citation.
6. `docs(documentation-system): codify principles surfaced from audit` — non-duplication-applies-to-itself, source-can-answer, size-is-diagnostic, audit-sharp-edges, atomic-commits.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint B4.

### B5: Triage root `CLAUDE.md` Invariants section

1. `docs(claude.md root): triage Invariants section` — agent-only invariants stay; human-facing rules become one-line pointers to `CONTRIBUTING.md` / `docs/documentation-system.md`; deletable invariants removed outright.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint B5.

## Sub-phase C: ADR Cleanup + Backfill

**Goal:** Stabilize the decision-record series.

### C1: ADR cleanups (one commit per ADR)

1. `adr(0002): shrink, remove Implementation Status, add multi-replica topology Mermaid` — `docs/architecture-decisions/0002-state-externalization-postgres-outbox.md`.
2. `adr(0003): drop Phase 3c/4 implementation notes` — `0003-state-cursor-contract-and-operator-policy.md`.
3. `adr(0005): replace pasted function signatures with file:line citations` — `0005-persistentstore-trait-relocation.md`.
4. `adr(0006): trim signatures, add lifecycle Mermaid` — `0006-stores-own-background-task-lifecycle.md`.
5. `adr(0008): upgrade ASCII dep-graph table to Mermaid` — `0008-persistence-crate-split.md`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint C1.

### C2: ADR backfill

1. `adr(0010): document rust-embed embedding decision` — alternatives considered (separate static-asset CDN, runtime FS), why it won.
2. `adr(0011): document GitHub Actions + release-please toolchain decision` — alternatives (raw cargo-release, custom scripts, semantic-release), why GHA + release-please.
3. `adr(0012): document frontend framework stack decision` — Svelte 5 over SvelteKit/React, Tailwind v4 with Vite plugin, OKLCH single-hue model, Biome / eslint-plugin-svelte split.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint C2.

## Sub-phase D: CLAUDE.md Rewrites

**Goal:** Bring all `CLAUDE.md` files in line with slim-pointer + sharp-edges template. Each rewrite audits the existing sharp edges per principle #10.

### D1: Backend crate `CLAUDE.md`

1. `docs(atc-server): rewrite CLAUDE.md from atc-wire exemplar template` — ~20KB → ~2KB. Sharp edges audited; `OrbStack DOCKER_HOST` dropped (environment concern). Testing foot-guns deferred to `docs/testing.md` (sub-phase E1).
2. `docs(atc-core): shrink CLAUDE.md, keep new contracts, drop duplicated invariants` — ~6KB → ~2.5KB. `completed_at` preserve-first, conclusion↔status invariant, predecessor predicate stay. Five state-machine invariants from arch doc become pointer.
3. `docs(atc-store-pg): shrink file map in CLAUDE.md` — 8-row file map → 2-3 conceptual groupings. Sharp Edges (6 entries) audited.
4. `docs(atc-github): halve Contracts section in CLAUDE.md` — keep foot-guns (SHA-1 rejection, runner_group_name normalization, `deny_unknown_fields=false`), drop arch-doc paraphrase.
5. `docs(atc-store-mem): add metrics.md cross-link in CLAUDE.md` — single line pointing at `metrics.md` for `eviction.sweep` span semantics.
6. `docs(atc-wire,atc-persist): verify exemplar CLAUDE.md files current` — no-op or minor polish only.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint D1.

### D2: Frontend `CLAUDE.md`

1. `docs(frontend): trim three Key Files rows in CLAUDE.md` — `src/lib/stores/`, `src/lib/dispatcher.ts`, `src/lib/connection.ts` rows from 600+200+250 words to 1-2 sentence pointers. Sharp Edges audited (typed-union switches, MSW fixture typing, URL↔selectedRunId guards).

**🛑 STOP — sub-phase end.** Push, request review at checkpoint D2.

### D3: Helm `CLAUDE.md`

1. `docs(helm): rewrite CLAUDE.md from .github/CLAUDE.md template` — ~12KB → ~3KB. Sharp edges audited (3 Helm-authoring foot-guns: `{{ if }}` whitespace, `subPath` vs directory mount, 0.2 schema-breaking removals migration).

**🛑 STOP — sub-phase end.** Push, request review at checkpoint D3.

### D4: `.github/CLAUDE.md` verification pass

1. `docs(.github): verify CLAUDE.md current state` — small updates if anything drifted; otherwise no-op.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint D4.

## Sub-phase E: Auxiliary Docs Created

**Goal:** Stand up the docs that will receive content extracted in Sub-phase F.

### E1: `docs/testing.md`

1. `docs: create docs/testing.md with backend testing conventions` — Docker required, `#[serial_test::serial]`, `OnceLock` harness. Each foot-gun audited for current load-bearing-ness.
2. `docs(contributing): link to docs/testing.md` — cross-reference from CONTRIBUTING.md.
3. `chore(doc-mapping): add docs/testing.md → backend-server.md mapping` — YAML entry.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint E1.

### E2: `docs/operator/multi-replica-smoke-test.md`

1. `docs: create docs/operator/multi-replica-smoke-test.md from deployment.md` — content extracted, no prose changes.
2. `docs(deployment): remove smoke test section, link to operator runbook` — `deployment.md` slimmed.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint E2.

### E3: `docs/operator/metric-interpretation-guide.md`

1. `docs: create docs/operator/metric-interpretation-guide.md from metrics.md interpretation entries` — only interpretation-bearing entries (NaN sentinel, parity/transient split, alerting heuristic). Mechanical entries (name + type + label) do not survive.
2. `docs(metrics): remove interpretation catalog, link to operator guide` — `metrics.md` slimmed.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint E3.

## Sub-phase F: Major Arch-Doc Rewrites

**Goal:** The biggest content commits. Each doc gets multiple atomic commits.

### F1: `backend-server.md` rewrite

1. `docs(backend-server): apply new arch-doc template skeleton` — section structure matches `docs/architecture/_template.md`.
2. `docs(backend-server): delete verbatim AppState/StateSnapshot/WireFrame struct blocks` — LSP territory.
3. `docs(backend-server): delete step-by-step handler narration` — code reads better than prose for this.
4. `docs(backend-server): delete Files section` — rustdoc + `ls -la` territory.
5. `docs(backend-server): add Mermaid crate dependency graph` — 7 nodes.
6. `docs(backend-server): add Mermaid webhook → outbox → drain → broadcast → WS flow`.
7. `docs(backend-server): add Mermaid WorkflowRun + Job state machines`.
8. `docs(backend-server): add Mermaid snapshot/stream reconciliation diagram`.
9. `docs(backend-server): add Mermaid shutdown sequence` — replaces current indented text tree.
10. `docs(backend-server): cross-link to ci-pipeline.md and metrics.md`.
11. `docs(backend-server): decide otel-wiring.md split` — extract to `docs/architecture/otel-wiring.md` if section feels like its own doc; otherwise leave inline.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F1.

### F2: `frontend-app.md` rewrite

1. `docs(frontend-app): apply new arch-doc template skeleton + add TOC`.
2. `docs(frontend-app): delete component prop interface dumps` — LSP territory.
3. `docs(frontend-app): delete Animation Inventory section` — grep territory.
4. `docs(frontend-app): delete Sort Strategies enumeration` — code territory.
5. `docs(frontend-app): delete Component Contracts table` — LSP territory.
6. `docs(frontend-app): delete 130-line Files section` — rustdoc/JSDoc + `ls -la` territory.
7. `docs(frontend-app): add Mermaid macro data-flow` — WS → dispatcher → store → derived → DOM.
8. `docs(frontend-app): add Mermaid connection-protocol startup/reconnect sequence`.
9. `docs(frontend-app): add Mermaid app lifecycle state machine`.
10. `docs(frontend-app): keep + relocate three ASCII component trees as needed`.
11. `docs(frontend-app): cross-link to metrics.md (WS instrumentation) and deployment.md (rolling-update affects reconnect UX)`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F2.

### F3: `metrics.md` rewrite

1. `docs(metrics): apply new arch-doc template skeleton`.
2. `docs(metrics): drop sqlx span table (crate-internal)`.
3. `docs(metrics): drop low-information per-metric PromQL where standard form suffices`.
4. `docs(metrics): move stable-names warning into span inventory heading`.
5. `docs(metrics): add Mermaid OTel pipeline diagram` — source → SDK → exporter → collector → Tempo/Mimir.
6. `docs(metrics): add Mermaid span hierarchy tree`.
7. `docs(metrics): cross-link to backend-server.md and frontend-app.md`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F3.

### F4: `deployment.md` rewrite

1. `docs(deployment): apply new arch-doc template skeleton`.
2. `docs(deployment): shrink env-var tables (keep smallest, most-interpreted)`.
3. `docs(deployment): shrink knobs tables`.
4. `docs(deployment): delete Files section`.
5. `docs(deployment): add Mermaid k8s topology diagram` — Deployment + Service/Ingress/HTTPRoute/HPA/PDB/NetworkPolicy + external CNPG + optional collector.
6. `docs(deployment): add Mermaid graceful-shutdown sequence` — replaces 5-row timeline table.
7. `docs(deployment): cross-link to ci-pipeline.md and frontend-app.md`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F4.

### F5: `ci-pipeline.md` rewrite

1. `docs(ci-pipeline): apply new arch-doc template skeleton`.
2. `docs(ci-pipeline): trim Files section`.
3. `docs(ci-pipeline): trim Renovate config to paragraph + pointer to renovate.json`.
4. `docs(ci-pipeline): add Mermaid CI job-dependency DAG with path-filter branching`.
5. `docs(ci-pipeline): add ADR back-references (ADR-0007 testcontainers, ADR-0008 --locked)`.
6. `docs(ci-pipeline): cross-link to backend-server.md (testcontainers shared PG)`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F5.

### F6: `release-pipeline.md` rewrite

1. `docs(release-pipeline): apply new arch-doc template skeleton`.
2. `docs(release-pipeline): trim Files section`.
3. `docs(release-pipeline): delete Historical sections covering deleted ARC infrastructure`.
4. `docs(release-pipeline): add Mermaid two-phase flow diagram` — conventional commits → release-please → human merge → tag → release.yml DAG.
5. `docs(release-pipeline): add ADR back-reference to ADR-0011 (GHA + release-please toolchain)`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint F6.

### F7 (optional): `docs/architecture/otel-wiring.md` extraction

Conditional on the decision made in F1's last commit. If extracted: own template instance, own Mermaid diagrams for OTel init sequence and shutdown ordering.

## Sub-phase G: Final Meta-Doc Pass

**Goal:** Updates to meta-docs after everything else has landed.

### G1: `planning-workflow.md`

1. `docs(planning-workflow): add Mermaid phase-flow diagram` — Phase 1 → 7 with DoD gate.
2. `docs(planning-workflow): incorporate any process refinements from execution`.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint G1.

### G2: `CONTRIBUTING.md` final pass

1. `docs(contributing): update CLAUDE.md exemplar references to atc-wire / atc-persist / .github` — replaces current `atc-core` exemplar citation now that `atc-core` is shrunk.
2. `docs(contributing): final cross-link audit pass` — verify all cross-references current.

**🛑 STOP — sub-phase end.** Push, request review at checkpoint G2.

## Sub-phase H: Audit Cadence

**Goal:** Make sure the next audit happens.

### Manual step H1

1. `gh issue create --title "Run doc-system audit (next cadence)" --body "..."` — issue scheduled for 6 months from execution start. Reminder for the next audit pass.

This is the only step that lives outside the PR.

## Critical files modified

Roughly: 30+ files modified, 5+ files created.

- **Modified:** root `CLAUDE.md`, `CONTRIBUTING.md`, `docs/planning-workflow.md`, `docs/implementation-guidance.md`, `scripts/doc-mapping.sh` (deleted in B1), `lefthook.yml`, all 6 architecture docs, all 11 CLAUDE.md files, 5 existing ADRs, 1 metrics doc, 1 deployment doc.
- **Created:** `scripts/doc-mapping.yaml`, `scripts/check-docs.sh`, `docs/documentation-system.md`, `docs/architecture/_template.md`, `docs/testing.md`, `docs/operator/multi-replica-smoke-test.md`, `docs/operator/metric-interpretation-guide.md`, 3 new ADRs (0010, 0011, 0012), optionally `docs/architecture/otel-wiring.md`.

## Verification

After each commit:

- `just lint` passes.
- `just check` passes.
- `just test` passes (most commits don't change source; doc-mapping check needs to pass).
- Pre-push lefthook hooks fire without false-positives.

After each sub-phase (at the checkpoint):

- Visual review of rendered Markdown on the PR (especially Mermaid diagrams render correctly on GitHub).
- Cross-link sanity: clicking links lands at the expected target.
- For CLAUDE.md rewrites: verify the agent context still loads cleanly in a fresh Claude Code session in the project root.

After Sub-phase B (process foundation) lands:

- Brief design review on the new rails (template, lefthook gates, doc-mapping YAML) before launching Sub-phase D–G content work.

After Sub-phase F (arch-doc rewrites):

- Read each rewritten doc cold (simulate a new contributor). The "shape question" must be answerable.
- Grep for LSP/grep-discoverable content that survived. If anything mechanical remains, file a follow-up commit before closing F.

After Sub-phase H (audit cadence issue opened):

- Confirm the issue is on the calendar.

Final merge:

- Squash-merge the PR per repo default.

## What this plan does not include

- Implementation of new ATC features. The audit was about doc-system hygiene; no source code changes are in scope beyond what doc rewrites incidentally touch (`lefthook.yml`, `scripts/`).
- Re-audit of the doc system. Deferred to the cadence scheduled in Sub-phase H.

## Rough size

- Total commits: ~95 atomic commits in one PR.
- Total sub-phases: 19 named (A, B1–B5, C1–C2, D1–D4, E1–E3, F1–F7, G1–G2, H).
- Heaviest single sub-phase by commit count: F1 (`backend-server.md` rewrite, 11 commits).
- Earliest principle-validation: B4 (extracting `docs/documentation-system.md`) tests whether the principles survive contact with the doc system itself.
