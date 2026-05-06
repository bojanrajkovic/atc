# Phase 4 — Multi-Replica Enablement (Helm Chart + Operator Runbook)

> **Implementation Guidance:** Before writing any code for this plan, read [`docs/implementation-guidance.md`](../implementation-guidance.md). That document — not this plan — governs how implementation is executed (TDD discipline, branch/PR conventions, doc-mapping updates, ADR annotation sweeps, generated-file rules).

**PR title (squash-merged commit subject):** `feat(helm): gate multi-replica on postgres, remove sqlite/persistence (#7)`

## Context

PR #54 (commit `f367353`, Phase 3c) left the runtime fully symmetric-replica-ready: each replica runs its own LISTEN/NOTIFY listener and drain task, REPEATABLE READ snapshots cite a per-replica `broadcast_watermark` loaded with Acquire ordering before the snapshot tx is opened, the drain is the sole writer to the broadcast channel in PG mode, ring-buffer dedup gives single-delivery without frontend-side dedup, and `/readyz` 503s when the drain heartbeat goes stale. All Phase 3c invariants were verified against the current code (see file:line citations in §Architecture).

The runtime is ready. The deployment artifact is not. The Helm chart still pins `replicaCount: 1` operationally — `replicaCount > 1` renders today, but only because the chart predates Phase 1–3. There's no precondition that ties multi-replica to the existence of a Postgres URL, so an operator can render a multi-replica chart that immediately diverges in-memory state across pods.

Phase 4 is the deployment-surface work that closes issue #7 in substance:

- Gate `replicaCount > 1` on the presence of a Postgres URL via a template-time `{{ fail }}` guard.
- Remove SQLite mode (per ADR 0003 D3) and its only consumer — the chart's `persistence.*` machinery — because an audit confirms no application code references Kubernetes PVCs.
- Collapse the chart's storage-mode story from three modes to two (ephemeral, external-Postgres).
- Document a multi-replica smoke-test runbook in `docs/architecture/deployment.md` so operators can validate against any cluster (kind/k3d/homelab) without expanding CI infrastructure (kind-in-CI is tracked separately as issue #12).
- Unblock issues #8 (HPA), #9 (PDB), #10 (anti-affinity), which all depend on Phase 4 closing.

## Definition of Done

1. `replicaCount > 1` without a configured Postgres URL fails template rendering with an explicit, remediation-naming error.
2. SQLite mode is removed from `values.yaml`, `templates/deployment.yaml`, `values.schema.json`, and the test fixtures.
3. `persistence.*` values, `templates/pvc.yaml`, persistence-conditional volume mounts in `deployment.yaml`, and the persistence+replicaCount>1 guard are removed. `additionalProperties: false` in `values.schema.json` will reject `persistence:` from operator values files; release notes call this out.
4. `helm-unittest` covers the new precondition guard (positive + negative) and the post-removal renders.
5. `docs/architecture/deployment.md` has a "Multi-replica smoke test" section with copy-pasteable `kubectl`/`curl`/`wscat` commands; the section explicitly states sticky sessions are NOT required and discouraged outside specific cost-tuning scenarios.
6. Architecture docs aligned: `deployment.md`, `state-externalization-research/rollout-and-implementation.md` (Phase 4 marked Done), ADR 0003 (implementation-note appendix on persistence-machinery removal), `deploy/helm/atc/CLAUDE.md`, root `CLAUDE.md` status line.
7. Issue #7 closed with summary comment; issues #8/#9/#10 re-triaged (label `blocked` removed).

## Locked Decisions (carried from Phases 1–3c — not open for re-evaluation)

- **Symmetric replicas, no leader.** Each replica serves `/v1/state` and `/v1/ws` independently. Per ADR 0002 Decision 5 (`docs/architecture-decisions/0002-state-externalization-postgres-outbox.md:147–176`).
- **Per-replica `broadcast_watermark`, drain is sole writer to broadcast channel in PG mode.** Verified at `backend/crates/atc-server/src/main.rs:119` (per-process `Arc<AtomicI64>`), `listener.rs:214` (Release store after successful drain pass), `routes.rs:113–115` (Acquire load before tx open), `listener.rs:344` (sole `webhook_tx.send` site in PG branch), `routes.rs:292–304` (handler is silent in PG mode).
- **Frontend has no `highestAppliedSeq` dedupe.** Backend ring-buffer dedup is the single-delivery contract. Per ADR 0003.
- **`/readyz` drain heartbeat staleness threshold = 30s.** `routes.rs:22–24` (`READYZ_HEARTBEAT_STALENESS_MS`), heartbeat refresh sites at `listener.rs:174,207`, init at `main.rs:118`.
- **Listener URL plumbing already supports pgbouncer split.** `ATC_DATABASE_LISTENER_URL` falls back to `ATC_DATABASE_URL`. Pool can be transaction-mode pgbouncer; listener must be session-mode-compatible (`config.rs:31–40`, `main.rs:149–153`, `listener.rs:54–60`). Helm wiring already in place (`templates/deployment.yaml:69–88`), documented in `deploy/helm/atc/CLAUDE.md` "PgBouncer + listener compatibility" contract.
- **In-memory mode (`pg_pool: None`) preserved for `just dev` and tests; no longer a documented production deployment shape.** Per ADR 0003 Decision 3.
- **SQLite-as-state-backend rejected.** Per ADR 0003 Decision 3 (`docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md:87–114`).
- **PG-side TTL eviction deferred to Phase 5.** Per ADR 0003 Decision 4 — current eviction loop runs only against the in-memory `StateStore` (`main.rs:103–104`) and is acceptable per-replica-racing for that store.
- **Server-side leader election rejected as the multi-replica enabling mechanism.** May revisit later as a cost-reduction (not enabling) measure. Per ADR 0002 Decision 5.

## Architecture

### D1 — Template-time `{{ fail }}` guard for `replicaCount > 1` ⇒ Postgres URL required

The new guard sits at the top of `templates/deployment.yaml`, mimicking the existing pattern (current lines 1–7). It MUST evaluate `replicaCount > 1` AND require that **either** `config.databaseUrl` is non-empty **or** `existingSecret.name` is set with a non-empty `existingSecret.databaseUrlKey`. The render-time check cannot read Secret contents, so the precondition is "an operator-claimed Postgres URL is present in one of the two configured paths"; whether the URL actually points at Postgres is enforced at runtime by the binary (out of chart scope per existing convention — same as today's SQLite URL handling).

The expression uses the project's existing `| default ""` idiom (already established at `templates/deployment.yaml:1` for the SQLite check). Avoid `| toString` on a nullable value — `toString nil` returns the literal string `"<nil>"` in Helm, which is not equal to `""` and would make the guard pass falsely on a `null` `databaseUrl` (the chart's default).

```gotemplate
{{- $dbUrl := .Values.config.databaseUrl | default "" -}}
{{- $hasDbUrl := or (and .Values.existingSecret.name .Values.existingSecret.databaseUrlKey) (ne $dbUrl "") -}}
{{- if and (gt (int .Values.replicaCount) 1) (not $hasDbUrl) -}}
{{- fail "replicaCount > 1 requires a PostgreSQL connection string. Set either config.databaseUrl=postgres://... or existingSecret.name (with existingSecret.databaseUrlKey). Per ADR 0003: ephemeral in-memory mode is single-replica only." -}}
{{- end -}}
```

**Alternatives considered.** JSON Schema 2020-12 `if-then-else` cross-field validation. **Rejected:** project convention is template-time `{{ fail }}` for cross-field constraints (see existing guards at `templates/deployment.yaml:2–7` and `templates/httproute.yaml:2–4`). `deployment.md` lines 43–49 document this convention explicitly. Template-time error messages can include remediation guidance; schema errors are typically less actionable.

### D2 — Remove SQLite AND `persistence.*` machinery entirely

ADR 0003 Decision 3 says SQLite is "removed entirely" — settled. The user-confirmed extension: `persistence.*` machinery goes too. An audit (`grep -rn -i "persistence|persistent_volume|PersistentVolume|PVC"` over `backend/`, `frontend/src/`, and non-Helm `docs/`) returned zero hits referring to Kubernetes PVCs — every match was about in-memory persistence, sessionStorage, localStorage, or the PostgreSQL persistence layer. There is no current consumer and no roadmapped consumer that requires a PVC over a ConfigMap/env-var configuration model.

**Removed surface:**
- `values.yaml` — entire `persistence:` block, all SQLite docs (lines 106–132 storage-mode block, the line 182 sqlite-required note).
- `values.schema.json` — `persistence` property + sub-schema; update the `replicaCount` description to reference the new guard contract instead of persistence.
- `templates/pvc.yaml` — entire file.
- `templates/deployment.yaml` — persistence-conditional strategy block (current lines 15–24; strategy becomes constant `RollingUpdate {maxSurge: 1, maxUnavailable: 0}`), the SQLite + persistence fail-guard branches (current lines 2–7), AND the `data` PVC `volumeMounts` entry (current lines 111–114) AND `volumes` entry (current lines 118–122). **Important:** the `tmp` `emptyDir` (current lines 109/115–117 — `volumeMounts.tmp` mounting `/tmp`, plus the `volumes.tmp` `emptyDir`) is part of the chart's restricted-PSS contract (read-only rootfs needs a writable temp dir, exercised by `tests/unit/security.yaml:40`) and stays. Only the `data` volume goes.
- `deploy/helm/atc/README.md` — the "Local SQLite with Persistence" section (current lines 39–51), the "Ephemeral SQLite (/tmp)" section (current lines 53+), and the `persistence` line in the values reference (current line 100). Replace with a "Multi-replica with external Postgres" section. Update the `replicaCount` line (current line 97) to drop the persistence-incompatibility note and reference the new guard.
- `tests/values-persistence.yaml`.
- `tests/unit/pvc-invariant.yaml`.
- SQLite + persistence cases in `tests/unit/fail-guards.yaml` and `tests/unit/strategy.yaml` (replace with cases for the new guard and the now-constant strategy).
- `.github/workflows/ci.yml` — replace the `values-persistence.yaml` matrix entry (current line 293) with `values-multi-replica.yaml`.
- `docs/architecture/ci-pipeline.md` — update the matrix description (current line 24-area) from "persistence coverage" to "multi-replica coverage".

**Kept:**
- `_helpers.tpl` (no persistence-specific helpers).
- `templates/NOTES.txt` (no persistence/SQLite references — only the plain-text-DB-URL warning, which stays).
- `templates/deployment.yaml` — the `tmp` `emptyDir` and `/tmp` `volumeMount` (PSS read-only-rootfs contract).
- The `existingSecret` + listener-URL plumbing — orthogonal and still required.
- The Service / Ingress / HTTPRoute / ServiceMonitor / ServiceAccount templates — unchanged.

**Backwards-compatibility note.** `additionalProperties: false` (current `values.schema.json` setting) means operators with any `persistence:` keys in their existing values files will fail schema validation on `helm upgrade`. Release notes (and the PR description's "Operator upgrade notes" section) MUST document this. Mitigation is one-line: remove the `persistence:` block from operator values files. There is no programmatic migration tool — this is a deliberate breaking change in a 0.x chart.

**Alternatives considered.**
- *Keep `persistence.*` and PVC template per literal ADR 0003 D3 reading.* **Rejected:** ADR 0003 D3 says "the existing **guard** ... stays in place" (about RWO PVC semantics being orthogonal); it does not commit to keeping the persistence-the-feature surface. With zero current/planned consumers, retaining a templated PVC is dead code with maintenance cost (rendering, tests, schema validation, doc surface).
- *Deprecate persistence for one release cycle.* **Rejected:** ADR 0003 D3 says "removed", not "deprecated then removed". A deprecation cycle implies pre-1.0 chart users are running production loads on this; per project memory the chart is a homelab + early-access surface. Cleaner break.

### D3 — No URL-identity guard for pgbouncer / direct-PG split

The user's open decision asked whether to add a chart-time check that `databaseUrl != databaseListenerUrl` when both are set. **Rejected.** Identical URLs are a valid configuration: when an operator is not running pgbouncer at all, both URLs legitimately point at the same direct-PG endpoint. A guard would produce false positives. The contract is *documented* in `deploy/helm/atc/CLAUDE.md` ("PgBouncer + listener compatibility") and *enforced at runtime* by the listener task — when LISTEN registrations get dropped (as transaction-mode pgbouncer does silently), the listener fails loudly at startup, surfacing the misconfiguration via logs and `/readyz`. No chart change.

### D4 — Operator runbook recommends NOT using sticky sessions

Per ADR 0002 Decision 5, both replicas serve `/v1/state` and `/v1/ws` without distinguishing. Reconnect-then-snapshot (`/v1/state` for the watermark, then `/v1/ws?lastSeq=N`) is the design. Sticky cookies are not required for correctness and can mask gap-healing regressions during development (a client that always lands on the same replica will never exercise the reconnect-across-replicas code path).

The operator runbook in `deployment.md` recommends explicitly NOT configuring sticky sessions on the Ingress / HTTPRoute / Service (the chart defaults already do not configure them). Operators with specific cost-tuning needs (e.g., reducing reconnect storms during rolling updates) can add sticky-cookie annotations themselves; the runbook will mention this is possible but discouraged.

### D5 — Validation gate is helm-unittest + manual smoke-test runbook

The rollout doc says "Operationally test: deploy with replicaCount = 2 against a shared Postgres; verify both replicas serve snapshots, both forward events." It says "deploy" — not "automate in CI." Issue #12 already tracks adding kind-based chart-testing to CI; Phase 4 honors that scope boundary by staying out of CI infrastructure work.

The validation gate has two parts:
1. **`helm-unittest` covers chart rendering** — the new `replicaCount > 1` guard (positive: with PG URL, renders; negative: without, fails with the expected message), the post-removal absence of persistence/PVC, and the strategy block now being constant `RollingUpdate`.
2. **Manual smoke test against a real cluster** — a runbook in `docs/architecture/deployment.md` with copy-pasteable commands for `kind`/`k3d`/homelab. The runbook is the artifact. Execution is a one-time gate before closing #7; it is not re-run on every PR (issue #12 will own that automation).

**Alternative considered.** Add kind+helm-install+wscat to CI now. **Rejected:** issue #12 owns this scope. Coupling Phase 4 to a separate initiative would expand both scopes and slow #7 closure.

### D6 — No replica-id code changes

Backend research found no `replica_id` / `instance_id` / `node_id` / `hostname` symbol in `backend/crates/atc-server/`. In Kubernetes, `kubectl logs -l app.kubernetes.io/name=atc -f --prefix` automatically tags log lines with pod names — sufficient for "which replica did what" attribution during the smoke test. Per-process identifier emission in tracing/metrics is Phase 5 (operational metrics) scope per ADR 0002 "Out of scope." No code change in Phase 4.

## Implementation Phases

> Phase 4 follows TDD discipline per `docs/implementation-guidance.md` Rule 2. The order below matters: Step 1 writes failing tests, Step 2 makes them pass, Step 3+ extend coverage and docs.

### Step 1 — Helm test fixtures (write failing tests first)

**New / updated test cases (run `helm-unittest` after each — every case in this step should be RED):**

- `tests/unit/fail-guards.yaml` — remove the SQLite-without-persistence and persistence-with-replicaCount>1 cases. Add three new cases (using `failedTemplate.errorPattern` per the existing pattern at `tests/unit/fail-guards.yaml:12`):
  1. `replicaCount: 2` + `config.databaseUrl: postgres://...` → renders successfully (no `failedTemplate`).
  2. `replicaCount: 2` + `existingSecret.name: foo` + `existingSecret.databaseUrlKey: database_url` → renders successfully.
  3. `replicaCount: 2` with neither set → fails; `errorPattern` matches a substring naming both `config.databaseUrl` and `existingSecret.databaseUrlKey` as remediation paths.
- `tests/unit/strategy.yaml` — remove the "switches to Recreate when persistence enabled" case. Replace with a single positive case asserting strategy is `RollingUpdate` with `maxSurge: 1, maxUnavailable: 0` regardless of values.
- `tests/values-multi-replica.yaml` (new) — `replicaCount: 2` + `config.databaseUrl: postgres://atc:atc@postgres:5432/atc`. Will replace `values-persistence.yaml` in the CI matrix (Step 4).

**Test-fixture removals (made consistent with the new chart shape in Step 2):**

- `tests/values-persistence.yaml` — delete.
- `tests/unit/pvc-invariant.yaml` — delete.

**No-op verifications:** `tests/unit/listener-url.yaml`, `tests/unit/security.yaml` should still pass post-removal. The security suite specifically asserts the `tmp` emptyDir remains (`tests/unit/security.yaml:40`), which we honor in Step 2.

### Step 2 — Helm chart edits (turn the new tests green)

- `templates/deployment.yaml`:
  - Replace the SQLite/persistence guards (current lines 1–7) with the new `replicaCount > 1 ⇒ Postgres URL required` guard from §D1. Keep the `$dbUrl` capture pattern at line 1; reuse it for the new `$hasDbUrl` expression.
  - Remove the persistence-conditional strategy block (current lines 15–24). Strategy becomes a single constant block: `RollingUpdate` with `maxSurge: 1, maxUnavailable: 0`.
  - Remove the `data` `volumeMounts` entry (current lines 111–114) AND the `data` `volumes` entry (current lines 118–122). **Keep** the `tmp` `volumeMounts` entry (current lines 109–110) and the `tmp` `volumes` entry (current lines 115–117) — these are PSS-contract assets independent of persistence.
- `templates/pvc.yaml` — delete file.
- `values.yaml`:
  - Remove the entire `persistence:` block and the line 182 sqlite-required note.
  - Replace lines 106–132 storage-mode docs with the two-mode block in §"Storage modes appendix" below.
- `values.schema.json`:
  - Remove the `persistence` property and its sub-schema.
  - Update the `replicaCount` description to drop the persistence reference and point at the new guard contract: e.g., `"Number of replicas. Values greater than 1 require a PostgreSQL connection string via config.databaseUrl or existingSecret. Enforced at chart render time."`
- `templates/_helpers.tpl` — verify no persistence-specific helpers; expected: no changes.
- `templates/NOTES.txt` — verify no SQLite/persistence references; expected: no changes.

After Step 2, all `helm-unittest` cases from Step 1 should be GREEN.

### Step 3 — Chart-shipped README and CLAUDE.md

- `deploy/helm/atc/README.md`:
  - Remove the "Local SQLite with Persistence" section (current lines 39–51).
  - Remove the "Ephemeral SQLite (/tmp)" section (current lines 53+).
  - Remove the `persistence` line from the values reference (current line 100).
  - Update the `replicaCount` line (current line 97) to drop the "incompatible with persistence when > 1" note and reference the new guard.
  - Add a new "Multi-replica with external Postgres" section that mirrors `deployment.md`'s multi-replica section (preconditions, sticky-session guidance, smoke-test pointer).
- `deploy/helm/atc/CLAUDE.md` — add a "Multi-replica" subsection under Contracts:

> **Multi-replica precondition:** `replicaCount > 1` requires a PostgreSQL connection string via either `config.databaseUrl` or `existingSecret.name`+`existingSecret.databaseUrlKey`. Enforced at template-render time. Per ADR 0003 D3.
>
> **Sticky sessions are NOT required.** Reconnect-then-snapshot via `/v1/state`+`lastSeq` is the design (ADR 0002 D5). Configuring sticky cookies is discouraged outside specific cost-tuning scenarios — it can mask gap-healing regressions in development.
>
> **Anti-affinity / PDB / HPA defaults are not provided.** Tracked as #10 / #9 / #8 — unblocked after Phase 4.

Stamp `Last verified:` on both files with the date the implementation actually lands (do not pre-fill from this plan).

### Step 4 — CI workflow + ci-pipeline doc

- `.github/workflows/ci.yml` — replace the `values-persistence.yaml` matrix entry (current line 293) with `values-multi-replica.yaml`. Keep the rest of the matrix intact.
- `docs/architecture/ci-pipeline.md` — update the matrix description (current line 24-area) from "persistence coverage" to "multi-replica coverage". Stamp `Last verified:` with the landing date.

### Step 5 — Architecture docs

- `docs/architecture/deployment.md`:
  - Replace the three-mode storage decision (current lines 43–46 area) with a two-mode story: ephemeral (no DB URL, single-replica only) and external-Postgres (any replica count).
  - Remove the strategy-by-persistence-flag decision (current lines 47–49). The new doc states strategy is constant `RollingUpdate`.
  - Add a "Multi-replica" section: precondition guard, sticky-session guidance (D4), Phase 3c invariants summary (broadcast_watermark, REPEATABLE READ snapshot, ring-buffer dedup, drain heartbeat) cross-linked to `state-externalization-research/`.
  - Add a "Multi-replica smoke test" section with copy-pasteable runbook (kind/k3d/homelab). The runbook MUST follow the AC11 measurement protocol verbatim:
    1. Provision a Postgres (in-cluster via bitnami chart, or an existing instance).
    2. `helm install atc deploy/helm/atc --set replicaCount=2 --set config.databaseUrl=postgres://...`. Wait for both pods Ready.
    3. `kubectl port-forward` each pod to a distinct local port. Open a `wscat` session to each pod's `/v1/ws` (capture each session to a logfile).
    4. POST a webhook (use the admin-only test endpoint if added later, or replay a captured GitHub Actions webhook with valid HMAC).
    5. Within 5 seconds: `curl :portA/v1/state | jq .lastSeq` and `curl :portB/v1/state | jq .lastSeq` repeatedly until equal or timeout. Assert equality before timeout.
    6. Assert each `wscat` logfile shows exactly one `SeqEvent` for the webhook (single-delivery contract).
    7. `curl :portA/readyz` and `curl :portB/readyz` throughout — must return 200 the entire time.
  - Update Files section: PVC template entry removed.
  - Append a "Storage-mode evolution" note recording that SQLite mode and `persistence.*` chart machinery were removed in Phase 4 per ADR 0003 D3. Keep this historical context HERE (the canonical home for storage-mode evolution); do not duplicate into `values.yaml` or chart README — that would break AC4/AC5's zero-hit grep under `deploy/helm/`.
  - Stamp `Last verified:` with the implementation landing date (do not pre-fill from this plan).
- `docs/architecture/state-externalization-research/rollout-and-implementation.md`:
  - Mark the Phase 4 section "Done" with a date stamp.
  - Cross-link to `deployment.md`'s smoke-test runbook.
- `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md`:
  - Append a "Phase 4 implementation note": persistence-machinery (PVC template, `persistence.*` values, persistence guard) removed alongside SQLite. Audit found no application-code consumers; future PVC needs would re-introduce a tighter, purpose-specific surface.
- `CLAUDE.md` (root):
  - Update the Status paragraph: Phase 4 done; multi-replica deployment available with external Postgres.
  - Stamp `Last verified:` with the landing date.

**ADR-driven stale-content sweep** (per `docs/implementation-guidance.md` Rule 6 — Phase 4 implements ADR 0003 D3 + ADR 0002 D5, so the sweep applies). Search the repo for superseded language and annotate or update each site:
- `docs/design-plans/2026-04-08-helm-chart.md` — original chart plan with three-mode story + persistence-strategy decision; annotate at the top with `> **Revised by Phase 4 (ADR 0003 D3):** SQLite and persistence machinery removed; chart now supports two storage modes (ephemeral, external-Postgres). See docs/design-plans/2026-05-06-phase-4-multi-replica-enablement.md.`
- `docs/architecture/state-externalization-research/README.md` — search for the "Helm blocks all replicaCount > 1" passage; update to reflect the new precondition guard.
- ADR 0002 — verify out-of-scope sections that delegate operator-surface decisions to ADR 0003 still read correctly.
- Any other doc returning hits for "three modes" / "Mode 2" / "persistence guard stays" — annotate or update; do not silently leave.

### Step 6 — `scripts/doc-mapping.sh`

Verify (no changes expected): `deploy/helm/atc/*` already maps to `docs/architecture/deployment.md`; `.github/workflows/*` mappings already exist. The doc-staleness gate in `scripts/check-docs-lefthook.sh` will fire if any chart or CI change lands without the corresponding doc update — that's exactly what Phase 4 needs. No new mappings.

### Step 7 — Verification

Use specific recipes (`just test` runs cargo+pnpm and does NOT include Helm; `just lint` runs cargo clippy only):

- `just helm-lint` — chart lints clean.
- `just helm-unittest` — all helm-unittest cases pass (the new fail-guard cases, the simplified strategy assertion, and the unchanged listener-url + security suites).
- `just helm-template` — render under representative values; should succeed for defaults and for `values-multi-replica.yaml`.
- `just helm-check` — kubeconform validation against the Kubernetes API schemas.
- `just lint` and `just test` — full Rust + frontend suites stay green (no regressions; chart changes shouldn't touch them, but run defensively).
- Spot-check `helm template` invocations:
  - `helm template atc deploy/helm/atc` → default render, single replica, no DB URL, ephemeral mode → succeeds.
  - `helm template atc deploy/helm/atc --set replicaCount=2` → fails with the new guard; message names both `config.databaseUrl` and `existingSecret.databaseUrlKey` as remediation paths.
  - `helm template atc deploy/helm/atc --set replicaCount=2 --set config.databaseUrl=postgres://x:y@z:5432/atc` → succeeds; `replicas: 2` in the rendered Deployment.
  - `helm template atc deploy/helm/atc --set replicaCount=2 --set existingSecret.name=foo --set existingSecret.databaseUrlKey=database_url` → succeeds; env var resolves via `secretKeyRef`.
  - `helm template atc deploy/helm/atc --set persistence.enabled=true` → fails with **schema** validation (additionalProperties: false). This is the intentional breaking-change signal for upgrading operators.
- Manual smoke test against a kind/k3d/homelab cluster (the `deployment.md` runbook), executed once before closing #7. Capture the output (`kubectl logs`, `curl /v1/state` diffs, `wscat` event counts) in the PR's first-comment test plan as closure evidence.

### Step 8 — Issue closure and downstream unblocks

- Close issue #7 manually via `gh issue close 7 --reason completed -c '<summary linking to deployment.md multi-replica section>'`. (release-please does not auto-close design-track issues.)
- Re-triage issues #8 (HPA), #9 (PDB), #10 (anti-affinity): remove the `blocked` label via `gh issue edit <N> --remove-label blocked`; confirm titles still describe scope correctly.

## Acceptance Criteria

| ID | Type | Criterion |
|----|------|-----------|
| **AC1** | Success | `helm template atc deploy/helm/atc --set replicaCount=2 --set config.databaseUrl=postgres://x:y@z/atc` renders without error; `replicas: 2` appears in the rendered Deployment. |
| **AC2** | Success | `helm template atc deploy/helm/atc --set replicaCount=2 --set existingSecret.name=foo --set existingSecret.databaseUrlKey=database_url` renders without error; the env var pulls from the secret. |
| **AC3** | Failure | `helm template atc deploy/helm/atc --set replicaCount=2` fails with a `{{ fail }}` template error message that names BOTH `config.databaseUrl` and `existingSecret.name`/`existingSecret.databaseUrlKey` as the remediation paths. |
| **AC4** | Success | `git grep -i sqlite deploy/helm/` returns zero hits after Phase 4 lands. |
| **AC5** | Success | `git grep -in -E "persistence|persistent_volume|PersistentVolume|PVC" -- deploy/helm/` returns zero hits after Phase 4 lands. (Historical context lives in `docs/architecture/deployment.md`, not in `deploy/helm/`.) |
| **AC6** | Failure | `helm template atc deploy/helm/atc --set persistence.enabled=true` fails with a JSON-Schema validation error citing `additionalProperties: false` and an unknown `persistence` field. |
| **AC7** | Success | `helm template atc deploy/helm/atc` renders the Deployment with `strategy: RollingUpdate` and `maxSurge: 1, maxUnavailable: 0` regardless of any flag. |
| **AC8** | Success | `just helm-unittest` passes; new fail-guard cases exist in `tests/unit/fail-guards.yaml` (using `failedTemplate.errorPattern` per the existing convention); `tests/unit/pvc-invariant.yaml` is absent; `tests/values-persistence.yaml` is absent. |
| **AC9** | Success | `docs/architecture/deployment.md` has a "Multi-replica smoke test" section with `kubectl`/`curl`/`wscat` commands; the section explicitly states sticky sessions are NOT required. |
| **AC10** | Success | `docs/architecture/deployment.md` storage-mode story is two modes (ephemeral, external-Postgres); no doc returns hits for "three modes" / "Mode 2" / "persistence guard stays" except the per-doc `> **Revised by Phase 4...**` annotation calls in superseded design plans. |
| **AC10b** | Success | `docs/architecture/state-externalization-research/rollout-and-implementation.md` Phase 4 section has a "Done" marker with the landing date. |
| **AC10c** | Success | ADR 0003 has a "Phase 4 implementation note" appendix recording the persistence-machinery removal rationale. |
| **AC10d** | Success | `deploy/helm/atc/CLAUDE.md` includes a Multi-replica Contracts subsection covering the precondition, sticky-session guidance, and the #8/#9/#10 unblock note. |
| **AC10e** | Success | Root `CLAUDE.md` Status paragraph reflects Phase 4 done with the landing-date `Last verified:` stamp. |
| **AC10f** | Success | `.github/workflows/ci.yml` Helm matrix lists `values-multi-replica.yaml` (not `values-persistence.yaml`); `docs/architecture/ci-pipeline.md` matrix description matches. |
| **AC10g** | Success | `deploy/helm/atc/README.md` has the "Multi-replica with external Postgres" section and zero references to SQLite or `persistence.*`. |
| **AC11** | Success (manual smoke test) | Two-replica deploy against shared Postgres, executed via the `deployment.md` runbook: after a webhook is POSTed to one replica, both pod-local `/v1/state` endpoints converge on the same `lastSeq` within 5 seconds (poll-until-equal); each `wscat` session received exactly one `SeqEvent` for that webhook (single-delivery via ring-buffer dedup); both `/readyz` endpoints stayed 200 throughout. |
| **AC12** | Success | Issue #7 closed (manual `gh issue close --reason completed`). Issues #8/#9/#10 have the `blocked` label removed. |

## Documents to Update

| Doc | Update |
|-----|--------|
| `docs/architecture/deployment.md` | Two-mode storage; remove persistence/strategy decisions; add "Multi-replica" + "Multi-replica smoke test" sections; PVC template removed from Files; storage-mode-evolution historical note; sticky-session guidance |
| `docs/architecture/state-externalization-research/rollout-and-implementation.md` | Mark Phase 4 Done; cross-link runbook |
| `docs/architecture-decisions/0003-state-cursor-contract-and-operator-policy.md` | Implementation-note appendix on persistence-machinery removal |
| `docs/architecture/ci-pipeline.md` | Helm matrix description: persistence → multi-replica |
| `deploy/helm/atc/CLAUDE.md` | Multi-replica precondition; sticky-session guidance; #8/#9/#10 unblock note; Last verified date |
| `deploy/helm/atc/README.md` | Remove SQLite + persistence sections; add Multi-replica section; update replicaCount + values reference |
| `CLAUDE.md` (root) | Status paragraph: Phase 4 done; Last verified date |

**Stale-content sweep targets** (per Step 5's ADR-driven sweep): `docs/design-plans/2026-04-08-helm-chart.md` (annotate), `docs/architecture/state-externalization-research/README.md` (update). Run `git grep -inE "three modes|Mode 2|persistence guard stays in place|SQLite (mode|with Persistence)"` to find any sites missed.

`scripts/doc-mapping.sh` — verify only; no changes expected.

## Implementation Guidance

`docs/implementation-guidance.md` governs all implementation work for this plan. Specific rules that bite for Phase 4:

- **Rule 1 — feature branch + PR conventions.** Branch off `main`; squash-merge; PR title is the eventual squashed commit subject (`feat(helm): gate multi-replica on postgres, remove sqlite/persistence (#7)`); test plan goes in the FIRST PR comment, not the PR body.
- **Rule 2 — TDD discipline.** Step 1 lists the failing helm-unittest cases that anchor the chart edits in Step 2. Run `just helm-unittest` after Step 1 (RED), again after Step 2 (GREEN). Per `feedback_no_source_grep_tests.md`, do not write greps that assert source structure as if they were tests; the AC4/AC5 grep is a reviewer/CI verification, not a test in `tests/`.
- **Rule 4 — doc-mapping.** Verified: `deploy/helm/atc/*` → `docs/architecture/deployment.md` mapping already exists. No changes to `scripts/doc-mapping.sh` expected.
- **Rule 5 — GitHub Actions SHA-pinning.** The CI workflow change in Step 4 only edits the values matrix list, not any `uses:` ref; if the implementation context discovers a needed `uses:` change, pin to a full SHA.
- **Rule 6 — ADR annotation sweep.** Phase 4 implements ADR 0003 D3 + ADR 0002 D5. Step 5's stale-content sweep IS this rule's application. Don't skip it.
- **Rule 14 — use subagents.** The implementation context should orchestrate via subagents (codebase-investigator for verification reads, the project-claude-librarian for CLAUDE.md updates), not perform every read and edit inline.
- **Memory-anchored conventions** to honor: `feedback_pr_title_convention.md` (full deliverable title), `feedback_test_plans.md` (test plan as first PR comment), `feedback_verify_lefthook_installed.md` (run `just setup` at session start), `feedback_run_e2e_tests_for_frontend_changes.md` (NOT applicable here — no frontend changes), `feedback_plans_in_repo_no_review_artifacts.md` (the committed `docs/design-plans/...` copy of this plan is the final form; strip any "previous draft" / "codex blocker" narrative when copying).

## Out of Scope (deferred)

- **kind-based chart-testing CI** — issue #12 (named explicitly in user's open-decision framing).
- **Anti-affinity defaults** — issue #10 (unblocks after Phase 4).
- **PodDisruptionBudget template** — issue #9 (unblocks after Phase 4).
- **HorizontalPodAutoscaler template** — issue #8 (unblocks after Phase 4).
- **NetworkPolicy template** — issue #11.
- **Per-process replica identifier in tracing/metrics** — Phase 5 scope per ADR 0002 "Out of scope."
- **PG-side TTL eviction** — Phase 5 per ADR 0003 Decision 4.
- **Operational metrics for outbox lag, drain latency, dedup hits** — Phase 5 per ADR 0002 "Out of scope."
- **Removing in-memory mode entirely** — Phase 5 decision per ADR 0003 "Out of scope."
- **Persisting raw GitHub webhook JSON for audit** — Phase 5 per ADR 0002 "Out of scope."
- **Pool/network failure injection for backend tests** — issue #56.
- **CI runner-disk optimization** — issue #55.
- **Server-side leader election as primary multi-replica mechanism** — explicitly rejected by ADR 0002 Decision 5; not revisited.

## Glossary

- **Symmetric replica.** Replica that is functionally indistinguishable from any other; no leader election; same code paths on every node. ADR 0002 D5.
- **Broadcast watermark (`broadcast_watermark`).** Per-replica `Arc<AtomicI64>` cursor advanced by the drain task only after a successful drain pass. Loaded with `Ordering::Acquire` before opening a REPEATABLE READ snapshot tx for `/v1/state` (`backend/crates/atc-server/src/routes.rs:113–115`).
- **Drain task.** Per-replica tokio task in `backend/crates/atc-server/src/listener.rs` that polls the outbox table on NOTIFY wake-ups (level-triggered), writes broadcast events, and refreshes the readiness heartbeat.
- **NOTIFY channel.** PostgreSQL LISTEN/NOTIFY channel name `atc_outbox` (`backend/crates/atc-server/src/listener.rs:28`); payload is the BIGSERIAL `seq` as text.
- **Listener URL.** `ATC_DATABASE_LISTENER_URL` env var; falls back to `ATC_DATABASE_URL`. Required to be session-mode-compatible (cannot be transaction-mode pgbouncer).
- **Two-mode storage** (Phase 4 onward). The chart supports ephemeral (no `databaseUrl`, `replicaCount=1` only) and external-Postgres (`databaseUrl=postgres://...`, any `replicaCount`).
- **Reconnect-then-snapshot.** Browser/WS client reconnect strategy: fetch `/v1/state` for the snapshot + `lastSeq` cursor, then open `/v1/ws?lastSeq=N`. Enables clients to bounce between replicas without server-side affinity. ADR 0002 D5.

## Storage modes appendix (replacement copy for `values.yaml` lines 106–132)

This block ships in `values.yaml`. It MUST NOT contain the strings "sqlite" or "persistence" — historical context lives in `docs/architecture/deployment.md` (per Step 5) so AC4/AC5 stay clean.

```
# Storage modes
#
# Two supported modes:
#
# Mode 1 — Ephemeral (in-memory, single replica):
#   Leave config.databaseUrl unset (the default).
#   ATC holds workflow run/job state in memory; pod restart loses state.
#   Required: replicaCount = 1. The chart enforces multi-replica
#   configurations to use external Postgres; see the {{ fail }} guard in
#   templates/deployment.yaml.
#   Use case: dev, CI, "I just want to see it run" homelabs.
#
# Mode 2 — External Postgres (any replica count):
#   Set config.databaseUrl: "postgres://user:pass@host:5432/atc"
#   OR provide existingSecret.name with existingSecret.databaseUrlKey.
#   ATC writes via the transactional outbox and reads from the live PG
#   state. Multi-replica: each replica runs its own LISTEN/NOTIFY listener
#   and drain task; symmetric, no leader (per ADR 0002 D5).
#   Use case: production, multi-replica homelab/work clusters.
#
# See docs/architecture/deployment.md for the full multi-replica runbook
# and the storage-mode evolution history.
```
