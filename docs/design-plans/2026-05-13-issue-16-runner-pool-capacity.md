# Issue #16 — Configurable Runner Pool Capacity (closes #143)

## Context

GitHub Issue #16 asks operators to declare known runner-pool capacities per label set so the frontend can render utilization (`running/capacity`) and color-coded saturation via `CapacityBar.svelte`. ATC derives `running` and `queued` counts from observed webhook events, but capacity is not in the webhook payload — operators have to supply it from their topology.

**Issue #143 is folded into this PR.** It's a one-line fix in `TopBar.svelte:81` (treat the string `"Default"` as null when picking the chip label, falling back to the label set). Bundling resolves a narrative awkwardness: the v1 design decision to omit `elastic` from the operator config schema rests on `isElastic` having no UI consumer. That's already true today (the prop flows through TopBar → RunnerBar → RunnerPool but isn't read for any display branch), and #143's fix codifies the principle by treating the chip label as a function of `(groupName, labels)` only.

The wire model and UI are already mostly in place:
- `backend/crates/atc-core/src/types.rs:111–134` defines `RunnerPoolStats` with `total: Option<u32>` as a forward-compatible placeholder (always `None` today).
- `frontend/src/lib/components/CapacityBar.svelte` exists and conditionally renders inside `RunnerPool.svelte:56–61` when `pool.total !== null`. Color thresholds use the existing semantic tokens (`--success` <70%, `--running` 70–99%, `--failed` ≥100%; fill clamps at 100%).
- `frontend/src/lib/stores/runners.svelte.ts:18–26` initializes `total: null` hardcoded; this is the merge point.
- `backend/crates/atc-server/src/config.rs` uses `figment 0.10.19` (env layer only, despite the `toml` feature already being enabled) to load `Config`. ADR 0004 moved pool-stats derivation to the frontend; no backend metric exposes pool counts today.

So issue #16 is a "plumb a value in" feature: design an operator-config delivery path, surface it to the frontend on the snapshot rail, and let the existing UI light up.

## Definition of Done

### Schema

```yaml
# /etc/atc/config.yaml — operator-supplied; mounted from a Helm-rendered ConfigMap
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10
  - labels: [ubuntu-latest]
    capacity: 20
```

Both `labels` (`Vec<String>`, non-empty) and `capacity` (`u32`, `>= 1`) are required. Deserialization normalizes `labels` into the canonical `LabelSet` (sort + dedup, matching the `BTreeSet<String>` invariant). Duplicate labels *within* a single pool's array are tolerated (deduped); duplicate *pools* (two entries that canonicalize to the same `LabelSet`) fail startup with a clear error.

### Deliverables

1. **Backend `Config`** gains `runner_pools: Vec<RunnerPoolConfig>`. `figment` chain becomes `defaults → Yaml::file(config_path) → Env::prefixed("ATC_").split("__")`. `Yaml::file()` is already auto-optional when the file is missing; the env layer carries scalar overrides only (`ATC_HTTP_ADDR`, `ATC_DATABASE_URL`, etc.). `runner_pools` is file-only — env-encoding a structured array via figment is not supported by `Env::prefixed("ATC_").split("__")` and adding a JSON shim is out of scope.
2. **Validation at load:** reject empty `labels` arrays, reject `capacity == 0`, reject duplicate canonicalized label sets across the array. All three are fatal at startup.
3. **`StateSnapshot`** gains `runner_pool_capacities: Vec<RunnerPoolCapacity>` (where `RunnerPoolCapacity = { labels: LabelSet, capacity: u32 }`). `AppState` holds `Vec<RunnerPoolCapacity>` (single source of truth), built once at startup from `Config`. `routes::state_handler` reads from `AppState` and clones the vec into each `StateSnapshot` response. `PersistentStore` is **not** modified — capacity is config, not state. ts-rs regenerates `RunnerPoolCapacity.ts` and `StateSnapshot.ts`.
4. **Frontend snapshot consumer:** `runStore.loadSnapshot()` (called from `connection.ts:105`) gains a third argument or accepts the full snapshot object; runner store holds `runnerPoolCapacities: RunnerPoolCapacity[]` as a state slice, defaulting to `[]` (so older snapshots during rolling deploys don't break). `computePoolStats()` in `frontend/src/lib/stores/runners.svelte.ts` accepts the capacities array, looks up each derived pool by its canonical label-set key, and populates `total` accordingly. Pools without declarations stay `total: null` and render exactly as today. **`dispatcher` is not touched** — it routes `SeqEvent`s, not snapshots.
5. **Helm chart**: `values.yaml` gains a `runnerPools` block defaulting to `[]`; `values.schema.json` gains the matching `runnerPools` JSON Schema entry (required since the root sets `additionalProperties: false`); the chart renders a `ConfigMap` and a read-only `volumeMount` at `/etc/atc/config.yaml` when `runnerPools` is non-empty. Empty list ⇒ no ConfigMap, no volume, no behavior change.
6. **Issue #143 chip-label fix** — `TopBar.svelte:79–87` introduces a `displayGroupName(pool)` helper that maps both `null` and `'Default'` to `null`, used in both the count loop (`70–75`) and the label decision tree.
7. Tests as described in the Implementation Phases below.
8. Documentation updates per the Documents to Update section.

### Success criteria

- Operator declares two pools in `values.yaml` → `helm upgrade` → pod renders ConfigMap → ATC reads the file at startup → fresh browser session shows `running/capacity` with `CapacityBar` at the appropriate color threshold for each declared pool.
- Pools not declared in config continue to render as today (running count only, no bar).
- A snapshot delivered without the `runner_pool_capacities` field (e.g., from an older replica during a rolling deploy) is tolerated: the frontend treats it as `[]` and no pool gains a capacity.
- In-memory dev mode and single-replica deployments work unchanged: the file is optional.
- `just lint`, `just test`, and `just types` pass; e2e green; `helm lint` and the helm-unittest suite green.

## Locked Decisions

1. **Frontend display-only.** Capacity flows config → server-side load → snapshot wire field → frontend merge. No `atc_runner_pool_*` Prometheus gauges; that path would force re-introducing server-side pool derivation and undo [ADR 0004](../architecture-decisions/0004-frontend-derived-pool-stats.md). Verified by `rg "runner_pool"` returning nothing under `backend/crates/atc-server/src/` and no entry in `docs/architecture/metrics.md`.
2. **YAML, not TOML.** Matches Kubernetes / Helm conventions; the chart's `runnerPools` values block renders directly into the ConfigMap data.
3. **Canonical mount path: `/etc/atc/config.yaml`.** POSIX-conventional, plays cleanly with `readOnlyRootFilesystem: true`. Path overridable via `ATC_CONFIG_FILE` env (helps tests).
4. **figment layering: `defaults → file → env`.** File is the structured-config surface. Env overrides remain for **scalars only** — `runner_pools` is not env-overridable, by design. Adding a JSON-decoding env shim is deferred (out of scope).
5. **Schema: `capacity` is required, `>= 1`.** Per internet research, "unbounded elastic" pools exist in spec (ARC v0.9+ allows omitting `maxRunners`) but no production deployment configures them that way. `capacity: null` and `capacity: 0` are both rejected at load.
6. **No operator-declared `elastic` flag in v1.** Verified by reading `TopBar.svelte` end-to-end: the `isElastic` prop flows through to `RunnerBar` and `RunnerPool` but no component reads it for any display branch. Issue #143's fix (bundled here) codifies the principle by making the chip label a pure function of `(groupName, labels)`. Ship the schema for what v1 actually reads; add `elastic` later as an additive change when a real consumer arrives.
7. **Snapshot rail, not a separate endpoint.** Capacity is small, static at startup, and already needs to be present on first paint. Inlining on `StateSnapshot` avoids a second round-trip.
8. **Capacity lives in `AppState`, not `PersistentStore`.** Capacity is operator config, not observed state. The `PersistentStore` trait (Postgres + InMemory) stays untouched. `routes::state_handler` composes the snapshot from `(PersistentStore::snapshot(), AppState::runner_pool_capacities)`.
9. **Duplicate canonicalized pools fail startup.** Silently last-one-wins would be a deployment-time footgun. Fail loud.
10. **Frontend defaults missing `runner_pool_capacities` to `[]`.** Rolling-deploy tolerance: a snapshot from an older replica without the field decodes to `[]` and no capacity is applied.

## Architecture

### Backend config

- **File:** `backend/crates/atc-server/src/config.rs`
- **Today:** `Config` struct at `src/config.rs:31–78` carries `http_addr`, `database_url`, `database_listener_url`, `log_filter`, `log_format`, `github`. `Config::load()` at `src/config.rs:54–77` runs `Figment::from(Serialized::defaults(...)).merge(Env::prefixed("ATC_").split("__")).extract()`.
- **Changes:**
  - Add `runner_pools: Vec<RunnerPoolConfig>` field with default `Vec::new()`.
  - Add `RunnerPoolConfig { labels: Vec<String>, capacity: u32 }` (private to `config.rs`, with `#[serde(deny_unknown_fields)]`).
  - Add `Yaml::file(config_path)` between defaults and env in the figment chain. Missing-file behavior is already optional in figment 0.10.19 — no `.required(false)` needed.
  - `config_path` defaults to `/etc/atc/config.yaml`, overridable via `ATC_CONFIG_FILE`.
  - Post-extraction validation step:
    - Reject if any `RunnerPoolConfig::labels` is empty after dedup.
    - Reject if any `RunnerPoolConfig::capacity == 0`.
    - Canonicalize labels (`Vec<String>` → sorted, deduped `Vec<String>`) and detect duplicate canonicalized label sets across the array → fatal error.
  - Each test that mutates `ATC_*` env or `ATC_CONFIG_FILE` uses `serial_test::serial` (already in the workspace tree, see `backend/crates/atc-server/Cargo.toml:54`) to prevent parallel-test races.
- **Cargo.toml:** Add `"yaml"` feature to `figment` (already at `Cargo.toml:18`).

### Wire transport

- **Files:** `backend/crates/atc-server/src/state.rs:51–73`, `backend/crates/atc-core/src/types.rs:61–134`, `backend/crates/atc-server/src/routes.rs` (or wherever `state_handler` lives).
- **Today:** `StateSnapshot { last_seq: u64, runs: Vec<WorkflowRun>, jobs: Vec<Job> }`, returned by `routes::state_handler` from `PersistentStore::snapshot()`.
- **Changes:**
  - Define `pub struct RunnerPoolCapacity { pub labels: LabelSet, pub capacity: u32 }` in `atc-core/src/types.rs`. ts-rs derive macros match neighboring types (`#[derive(Serialize, Deserialize, TS)]` + `#[ts(export)]`).
  - Add `pub runner_pool_capacities: Vec<RunnerPoolCapacity>` to `StateSnapshot`. Default-`[]` via `#[serde(default)]` so a missing field deserializes as empty (helps the WS client and any older replica).
  - `AppState` (`backend/crates/atc-server/src/state.rs` or wherever it's constructed) gains a `runner_pool_capacities: Vec<RunnerPoolCapacity>` field populated once at startup from `Config::runner_pools`.
  - `routes::state_handler` composes the response: `let mut snapshot = persist.snapshot()?; snapshot.runner_pool_capacities = state.runner_pool_capacities.clone(); Ok(snapshot)`.
  - `PersistentStore` and its impls (`pg.rs:427`, `in_memory.rs:183`) are **not** touched. The composition happens at the route layer.
  - Run `just types` to regenerate `frontend/src/lib/types/generated/RunnerPoolCapacity.ts` and `StateSnapshot.ts`.

### Frontend merge

- **Files:** `frontend/src/lib/connection.ts:105` (snapshot loader call site), `frontend/src/lib/stores/runners.svelte.ts:9–52` (computePoolStats), `frontend/src/lib/stores/runs.svelte.ts` (the snapshot loader target — `loadSnapshot`).
- **Today:** `connection.ts:105` calls `runStore.loadSnapshot(snapshot.runs, snapshot.jobs)`. `computePoolStats(runs, jobs)` walks jobs, groups by `LabelSet`, derives `running`/`queued`, initializes `total: null`.
- **Changes:**
  - Change `runStore.loadSnapshot(runs, jobs)` to `runStore.loadSnapshot(snapshot)` (or add a third arg; full-snapshot is cleaner). The store keeps the existing `runs`/`jobs` state plus a new `runnerPoolCapacities: RunnerPoolCapacity[]` slice, defaulting to `[]`. When loadSnapshot is called, all three are replaced.
  - Add a third parameter to `computePoolStats(runs, jobs, capacities)`. Build a `Map<PoolKey, number>` from the capacities array using `poolKey()` (ADR 0001) for the key. **Note:** `poolKey()` sorts but does not dedupe (`frontend/src/lib/filters/pool.ts:7`); the backend canonicalization is what guarantees the wire payload is already sorted+deduped, so on the frontend side we don't need to defensively dedupe before keying.
  - When a pool's derived label-set produces the same `poolKey()` as a capacities entry, set `entry.total = caps.get(key)!`.
  - **Do not touch `frontend/src/lib/dispatcher.ts`** — it routes `SeqEvent`s and is not on the snapshot path.
- **Important:** `CapacityBar.svelte` is already gated on `pool.total !== null`; once the merge sets `total`, the bar appears with no component change required. Existing clamp-at-100% / red-at-≥100% behavior covers the `running > capacity` case naturally.

### Chip-label fix (issue #143)

- **File:** `frontend/src/lib/components/TopBar.svelte:69–101`.
- **Change:** introduce a `displayGroupName(pool): string | null` helper that maps `'Default'` → `null` (and passes through everything else). Use it in both the count loop and the label decision tree so the set of names we count matches the set of names we'd actually use as a display label.
- **Optional cleanup:** drop the `isElastic` field from the `pools` array shape (line 95) and the downstream `RunnerPoolDisplay` interface in `RunnerBar.svelte` / `RunnerPool.svelte`. Update test fixtures in `TopBar.browser.test.ts`, `RunnerBar.test.ts`, `RunnerPool.test.ts`. The wire type `RunnerPoolStats.isElastic` stays as-is — it's still computed in `runners.svelte.ts:36–38` for any non-display future consumer.

### Helm chart

- **Files:** `deploy/helm/atc/values.yaml`, `deploy/helm/atc/values.schema.json` (required — root has `additionalProperties: false`), `deploy/helm/atc/templates/deployment.yaml:73–159`, new `deploy/helm/atc/templates/configmap.yaml`, `deploy/helm/atc/tests/unit/` (the existing helm-unittest suite — add new test files there).
- **Changes:**
  - `values.yaml`: add a `runnerPools` block, defaulting to `[]`, with a comment showing the structure.
  - `values.schema.json`: add a `runnerPools` property with item shape `{labels: array<string, minItems=1, uniqueItems=true>, capacity: integer minimum=1}`, `uniqueItems` on the outer array won't catch label-set collisions but the backend will (Decision 9).
  - New `templates/configmap.yaml`: when `gt (len .Values.runnerPools) 0`, render a `ConfigMap` with `data."config.yaml"` being `runner_pools:` followed by `toYaml .Values.runnerPools | nindent 2`.
  - `templates/deployment.yaml`: conditional `volume` + `volumeMount` at `/etc/atc/config.yaml` (subPath: `config.yaml`, readOnly: true). Distroless `:nonroot` and `readOnlyRootFilesystem: true` are compatible.
  - helm-unittest fixtures asserting the empty-list and populated-list cases per Phase 4.

### Why these choices over the alternatives

- `figment` with YAML + scalar env overrides is the lowest-friction option: dependency already present (just enable the `yaml` feature); no new crate; no controller/CRD/Postgres surface; no admin write API.
- ConfigMap mount with empty-list default keeps the in-memory dev mode and existing single-replica deployments byte-identical.
- Inlining capacity on `StateSnapshot` (not a new endpoint) keeps initial paint to a single request.
- Composing capacity at the route layer (not in `PersistentStore`) keeps the store trait single-purpose: it owns event-derived state, nothing else.

## Implementation Phases

Each phase has Step 1 (failing tests) and Step 2 (make them pass).

### Phase 1 — Backend config plumbing

**Step 1: Failing tests.** Add unit tests in `backend/crates/atc-server/src/config.rs` (each test annotated `#[serial_test::serial]`):
- `load_with_no_file_succeeds_with_empty_runner_pools` — figment chain produces `Vec::new()` when neither file nor env is set.
- `load_from_yaml_file_parses_runner_pools` — write a temp YAML file via `tempfile`, set `ATC_CONFIG_FILE` to its path, assert two pools deserialize with the right labels and capacities.
- `label_canonicalization_sorts_and_dedups_within_a_pool` — input `["x64", "self-hosted", "linux", "self-hosted"]` becomes `{"linux", "self-hosted", "x64"}`.
- `missing_capacity_is_a_deserialization_error` — YAML with `labels: [a]` and no `capacity` returns `Err`.
- `zero_capacity_is_a_validation_error` — YAML with `capacity: 0` returns `Err`.
- `empty_labels_is_a_validation_error` — YAML with `labels: []` (or `labels` whose dedup yields empty) returns `Err`.
- `duplicate_canonicalized_pools_fail_startup` — two entries that canonicalize to the same `LabelSet` return `Err`.

**Step 2: Implementation.** Add `"yaml"` to `figment`'s features in `backend/crates/atc-server/Cargo.toml`. Extend `Config` and `RunnerPoolConfig` per the Architecture section. Add the YAML provider to `Config::load()`. Implement label canonicalization + validation in a `Config::validate()` (or post-extract) helper.

### Phase 2 — Wire transport

**Step 1: Failing tests.**
- `atc-core` unit test: `RunnerPoolCapacity` serializes round-trip via `serde_json`, and `StateSnapshot` deserializes correctly when `runner_pool_capacities` is absent (default `[]`).
- `atc-server` integration test in `tests/integration/` (likely a new file `runner_pool_capacities_test.rs` plus updates to `common/mod.rs:368` and `ws_tests.rs:38` test fixtures to thread the new field through): end-to-end fixture loads a config with one pool, hits the state handler, asserts `runner_pool_capacities` is non-empty with the right labels and capacity. A second case with empty config asserts `runner_pool_capacities` is `[]`.

**Step 2: Implementation.** Add `RunnerPoolCapacity` to `atc-core/src/types.rs` with ts-rs export. Add `runner_pool_capacities: Vec<RunnerPoolCapacity>` to `StateSnapshot` with `#[serde(default)]` for the missing-field tolerance. Wire `AppState` to hold a `Vec<RunnerPoolCapacity>` built at startup from `Config`. `routes::state_handler` clones it into each response. Update integration test fixtures across `common/mod.rs` and `ws_tests.rs`. Run `just types`.

### Phase 3 — Frontend merge

**Step 1: Failing tests.** Extend `frontend/src/lib/stores/runners.test.ts` (or similar):
- `merges_declared_capacity_into_total` — given runs/jobs and a capacities array with a matching pool, `computePoolStats(…).total` is the declared number.
- `pool_without_declaration_stays_total_null` — derived pool not in capacities map → `total === null`.
- `label_canonicalization_matches_on_unsorted_input` — capacities declared as `[x, linux, self-hosted]` match a derived pool with labels in any order (relies on `poolKey()` sorting).
- `running_over_capacity_renders_failed_color` — a fixture with `running: 12, capacity: 10` asserts the bar uses the `--failed` token (regression on the existing `>= 100%` branch).

Also update `frontend/src/lib/__tests__/connection-test-helpers.ts:57` so synthetic snapshots include `runnerPoolCapacities: []` (or omit and rely on the `#[serde(default)]` tolerance).

**Step 2: Implementation.** Change `runStore.loadSnapshot()` to accept the full snapshot or a third argument. Add a `runnerPoolCapacities` slice to the runner store, default `[]`. Add a third parameter to `computePoolStats`. Build a `Map<PoolKey, number>` using `poolKey()`. Look up each derived pool's labels and assign `total`. **Do not touch `dispatcher.ts`** — it isn't on the snapshot path.

### Phase 4 — Helm chart values + schema + ConfigMap + volume mount

**Step 1: Failing tests.** Add helm-unittest specs under `deploy/helm/atc/tests/unit/`:
- With `runnerPools: []`: no `ConfigMap` resource is rendered; deployment has no `runner-pool-config` volume or mount.
- With two pools: a `ConfigMap` is rendered with `data."config.yaml"` containing `runner_pools:` and both pools as YAML.
- With two pools: the deployment has a `runner-pool-config` volume mounted at `/etc/atc/config.yaml` (read-only, `subPath: config.yaml`).
- Schema rejection test (if helm-unittest supports `helm lint`-style schema validation; otherwise add a `helm lint`-based check in `just test`): a values file with `runnerPools: [{labels: [], capacity: 1}]` fails schema validation; `{labels: [a], capacity: 0}` fails; missing `capacity` fails.

**Step 2: Implementation.** Add `runnerPools: []` to `values.yaml`. Add `runnerPools` to `values.schema.json` with the item shape `{labels: array<string, minItems=1, uniqueItems=true>, capacity: integer minimum=1}` and `required: [labels, capacity]`. Create `templates/configmap.yaml` gated on `gt (len .Values.runnerPools) 0`. Update `templates/deployment.yaml` to add the conditional `volume` and `volumeMount` blocks. Update the chart's NOTES.txt and/or README.

### Phase 5 — Chip-label fix (issue #143)

**Step 1: Failing tests.** Add to `frontend/src/lib/components/TopBar.browser.test.ts`:
- `chip_label_falls_back_to_labels_when_group_name_is_default` — fixture pool with `groupName: 'Default'` and labels `[self-hosted, linux, amd64]` renders the chip text as `self-hosted, linux, amd64`, not `Default`.
- `chip_label_uses_group_name_for_non_default_single_pool` — regression guard: a pool with `groupName: 'GPU Runners'` still renders `GPU Runners`.
- `chip_label_disambiguates_with_labels_when_two_pools_share_a_non_default_group` — regression guard for the `groupName · labels` case.

If pursuing the optional `isElastic`-prop cleanup, also update `RunnerBar.test.ts` and `RunnerPool.test.ts` fixtures to drop `isElastic`; those tests will fail to compile until the prop is removed.

**Step 2: Implementation.**
- Edit `TopBar.svelte:69–87` per the Architecture section (the `displayGroupName` helper + use in both count loop and label decision).
- Optional: drop the `isElastic` field from the `pools` array shape and the downstream `RunnerPoolDisplay` interface. Verify `pnpm check` is clean.

### Phase 6 — E2E + Documentation

**Step 1: Failing tests.** Add one e2e test under `frontend/e2e/`:
- `runner-pool-capacity.test.ts` — fixture seeds the `/v1/state` snapshot (the initial GET, not a WS event) with a pool at `running == 3, capacity == 10`. Assert `CapacityBar` is rendered, the count text reads `3/10`, and the status reflects the green (`<70%`) threshold. Add a second case for the amber threshold and a third for `running == 12, capacity == 10` (`--failed`). **Use the `/v1/state` HTTP mock pattern, not `e2e/lib/ws-mock.ts`** — that file is for WS events. If a `makeSnapshot` helper doesn't exist yet, write one in this phase under `e2e/lib/`.

**Step 2: Implementation.** Update:
- `docs/architecture/deployment.md` — new section documenting the file-based config layer, canonical mount path, the `runnerPools` Helm values block, and figment's `defaults → file → env` order (scalars-only env override).
- `docs/architecture/backend-server.md` — note `StateSnapshot.runner_pool_capacities` in the wire contract section; note `AppState::runner_pool_capacities` and that `PersistentStore` is untouched.
- `docs/architecture/frontend-app.md` — note the third-argument extension to `computePoolStats()` and the merge from the snapshot. Document the chip-label rule (`'Default'` is treated as null). Note that the runner store gained a `runnerPoolCapacities` state slice.
- `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` — append a short footnote linking to this design and noting operator-declared capacity now feeds the `total` field on the snapshot rail.

## Acceptance Criteria

- **AC1.** `Config::load()` accepts `runner_pools: Vec<RunnerPoolConfig>` from `/etc/atc/config.yaml` (path overridable via `ATC_CONFIG_FILE`). Missing file is not an error.
- **AC2.** Scalar config (`ATC_HTTP_ADDR`, `ATC_DATABASE_URL`, etc.) remains env-overridable. `runner_pools` is **file-only** — env-encoding a structured array is not supported by the figment env provider as used here, and a JSON shim is out of scope.
- **AC3.** `RunnerPoolConfig::labels` is canonicalized to sorted + deduped form during config load.
- **AC4.** Validation rejects: (a) entries with `capacity: 0`, (b) entries with empty `labels` (post-dedup), (c) duplicate canonicalized label sets across the array. Each is a startup-fatal error with a clear message.
- **AC5.** `StateSnapshot.runner_pool_capacities` is non-empty on responses when pools are declared; `[]` otherwise. ts-rs regenerates the matching TS type. The field has `#[serde(default)]` so an older replica's snapshot without it decodes to `[]`.
- **AC6.** Frontend `computePoolStats()` accepts the capacities array and populates `RunnerPoolStats.total` for declared pools; undeclared pools have `total === null`.
- **AC7.** A pool with `running: 3, capacity: 10` renders the `CapacityBar` at the `--success` threshold. `running: 8, capacity: 10` renders amber. `running: 12, capacity: 10` renders `--failed`, fill clamped at 100%. Undeclared pools render without a bar.
- **AC8.** Helm chart with `runnerPools: []` produces no `ConfigMap` and no volume mount. With ≥1 pool, the chart renders a `ConfigMap` and a `volumeMount` at `/etc/atc/config.yaml` (read-only, `subPath: config.yaml`). `values.schema.json` rejects malformed entries (`capacity: 0`, empty labels, missing capacity).
- **AC9.** `just lint`, `just test`, `just types`, and the full e2e suite pass. `helm lint` and the helm-unittest suite pass. The pre-push doc-staleness gate (`scripts/check-docs-lefthook.sh`) passes — i.e., every source file change has a corresponding architecture doc update. Note: `scripts/doc-mapping.sh` already covers all touched files via existing wildcards (verified — no additions needed).
- **AC10.** Two follow-up issues are filed before opening the PR: hot-reload, configured-but-idle pools. (Filed: #172 hot-reload, #173 configured-but-idle, #174 GitHub API runner discovery, #175 env-encoded runner_pools, #176 operator-declared elasticity.)
- **AC11.** Chip-label fix: a pool with `groupName === 'Default'` renders its label set in the TopBar chip, not `"Default"`. Non-`'Default'` group names still render as today. Issue #143 closes with this PR.
- **AC12.** Rolling-deploy tolerance: a frontend that connects to a backend whose snapshot lacks `runnerPoolCapacities` does not crash; the field decodes to `[]` and no pool gains a capacity.

## Documents to Update

| Doc | Change |
|---|---|
| `docs/architecture/deployment.md` | New "File-based configuration" section: figment chain, canonical mount path, `runnerPools` Helm values block, scalar-only env override behavior. |
| `docs/architecture/backend-server.md` | Wire contract section: add `runner_pool_capacities` to the `StateSnapshot` shape; note `AppState::runner_pool_capacities` and that `PersistentStore` is unmodified. |
| `docs/architecture/frontend-app.md` | Document the third-argument extension to `computePoolStats()`, the merge from the snapshot, the runner store's new `runnerPoolCapacities` slice, the chip-label rule (`'Default'` treated as null), and that `isElastic` has no display consumer. |
| `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` | Append a footnote: operator-declared capacity now feeds `RunnerPoolStats.total` via the snapshot rail (composed at the route layer, not in `PersistentStore`). |
| `backend/crates/atc-core/CLAUDE.md` | Note the new `RunnerPoolCapacity` export in the types surface. |
| `backend/crates/atc-server/CLAUDE.md` | Note the YAML file layer in figment, the `AppState::runner_pool_capacities` field, and that capacity composition happens at the route layer. |
| `frontend/CLAUDE.md` | Note the new `runnerPoolCapacities` state slice in the runner store and the `computePoolStats` signature change. |
| `deploy/helm/atc/CLAUDE.md` | Note the `runnerPools` values block, the ConfigMap rendering, and that `values.schema.json` is the canonical contract surface. |
| `deploy/helm/atc/README.md` (or NOTES.txt) | Document the `runnerPools` values block. |

`scripts/doc-mapping.sh` does **not** need new entries — existing wildcards already cover `frontend/src/*`, `backend/crates/atc-server/src/*`, `backend/crates/atc-core/src/*`, and `deploy/helm/atc/*`.

## Out of Scope

Tracked as follow-up issues:

- **#172 — Hot reload via `notify` + WS broadcast.** Operators edit ConfigMap, kubelet syncs the file (~60s), ATC re-reads, broadcasts a `ConfigUpdate` WS event, open browsers pick up the new capacity without reload. Requires watching the ConfigMap mount's parent directory (K8s `..data` symlink swap pattern), a new WS message type, and a typed-union case in the frontend's dispatcher.
- **#173 — Configured-but-idle pool surfacing.** Pools declared in config but with zero observed jobs should appear in the UI rendered as `0/N`. Requires the frontend pool list to be the union of observed + declared, not purely observed.
- **#174 — GitHub API runner discovery.** Query `/orgs/{org}/actions/runners`, aggregate `ephemeral`/`status`/`labels` to provide signals like inferred elasticity, currently-online count, and discovery of registered-but-quiet pools. Requires a new GitHub App permission (`organization_self_hosted_runners` or `administration: read`), a polling loop with conditional-GET caching, and per-deployment configuration of which orgs/repos to scan. Research confirmed this can't authoritatively answer either capacity or ARC elasticity — it's complementary, not a replacement.
- **#175 — Env-encoded `runner_pools` override.** A JSON-decoding env shim for figment, or porting to figment's TOML-like env syntax. Deferred; declared file-only in v1.
- **#176 — Operator-declared elasticity.** Adding `elastic: Option<bool>` to the config schema. Deferred until a UI consumer needs it.

Not tracked as a follow-up:

- **Backend Prometheus gauges for runner pools** — would force re-introducing server-side pool derivation and undo ADR 0004. Explicitly off the table.

## Glossary

- **LabelSet** — `BTreeSet<String>` in `atc-core/src/types.rs:61–103`. Canonical key for a runner pool: sorted, deduped, order-independent.
- **Bounded pool** — a runner pool with a fixed set of registered runners; capacity is the count of those runners.
- **Elastic pool** — a runner pool whose size scales with workload (GitHub-hosted, Actions Runner Controller, AWS autoscaling group). Capacity, when declared, is the ceiling (`maxRunners` / `maxReplicas` / `max_size`).
- **Capacity** — the declared upper bound for a pool, supplied by operator config. Distinct from "currently online runner count" and "currently running jobs".
- **figment** — the Rust config-loading crate ATC uses for layered config. Chain: `Serialized::defaults() → Yaml::file() → Env::prefixed("ATC_").split("__")`. `Yaml::file()` is auto-optional when the file is missing; the env layer carries scalars only.
