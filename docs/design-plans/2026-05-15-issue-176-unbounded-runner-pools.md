# Issue #176 — Unbounded-Capacity Runner Pools

## Context

Issue #176 extends operator-declared runner-pool capacity so a pool can be marked *unbounded* — i.e. no renderable ceiling. Two real operational cases produce pools whose capacity is not a meaningful ceiling: ARC v0.9+ `AutoscalingRunnerSet` deployed without `maxRunners`, and GitHub-hosted runner pools (per-account concurrency limits don't translate to a per-label-set ceiling). With the v1 schema (`capacity: u32, >= 1`), operators must either fake a high number (saturation bar permanently `<70%`, conveys nothing) or omit the pool (loses the "unbounded by design" signal). Neither is right.

The UX axis is **bounded vs unbounded**, not "autoscaling vs fixed-size" — an ARC pool capped at `maxRunners: 10` and a static pool of 10 registered runners render identically as `running/10`. So `capacity` and "elastic" are mutually exclusive at the operator-config level: one nullable field, not two fields.

This also retires the unreliable `runner_group_id == 0` heuristic for elasticity (confirmed unreliable in #143; bundled into the #16 PR's chip-label fix). With operator-declared unboundedness, the frontend gets the same signal from a trustworthy source and the heuristic disappears at its origin.

**Today (as of `main` post-#16):**
- `backend/crates/atc-server/src/config.rs:53-58` defines `RunnerPoolConfig { labels: Vec<String>, capacity: u32 }`. Validation at lines 149-178 rejects `capacity == 0`, empty `labels`, duplicate canonicalized label sets.
- `backend/crates/atc-core/src/types.rs:115-136` defines `RunnerPoolStats` with `is_elastic: bool` (set frontend-side from `groupId === 0n` — unreliable) and `total: Option<u32>` (set by the frontend merge in `runners.svelte.ts:54-56`).
- `RunnerPoolCapacity { labels: LabelSet, capacity: u32 }` is the wire-snapshot type composed in `routes::state_handler`.
- `frontend/src/lib/components/RunnerPool.svelte:57-69` has two rendering branches (`pool.total !== null` ⇒ bar + count; else count only). `isElastic` is plumbed through but never read.
- `deploy/helm/atc/values.schema.json:363-388` declares `capacity: integer, minimum 1`, with `capacity` in the `required` array.

## Definition of Done

### Schema

```yaml
# /etc/atc/config.yaml — operator-supplied; mounted from a Helm-rendered ConfigMap
runner_pools:
  - labels: [self-hosted, linux, x64]
    capacity: 10           # bounded, declared ceiling
  - labels: [ubuntu-latest]
    capacity: null         # operator-declared unbounded
```

Both `labels` (non-empty after dedup) and `capacity` (key required; value is integer `>=1` OR `null`) are required keys. Key omission is rejected; `capacity: null` is the canonical way to declare a pool unbounded. `capacity: 0` remains a validation error.

### Deliverables

1. **`RunnerPoolConfig.capacity: Option<u32>`** with a struct-level custom `Deserialize` impl that requires the `capacity` key to be present and rejects unknown keys (preserving `deny_unknown_fields` semantics). The public field type stays `Option<u32>` — the strictness lives in the `Deserialize` impl, not the field type.
2. **`RunnerPoolCapacity.capacity: Option<u32>`** (wire snapshot type) mirrors the same shape and strictness.
3. **`RunnerPoolStats` wire type changes:**
   - Drop `is_elastic: bool`.
   - Replace `total: Option<u32>` with `total: RunnerPoolTotal`, a three-variant adjacently-tagged enum.
4. **`RunnerPoolTotal`** new type in `atc-core/src/types.rs` with `#[derive(Serialize, Deserialize, TS)]` + `#[ts(export)]` + `#[serde(tag = "kind", content = "value")]`. Variants: `Bounded(u32)`, `Unbounded`, `Undeclared`. Locked wire / TS shape: `{ kind: "Bounded", value: number } | { kind: "Unbounded" } | { kind: "Undeclared" }`.
5. **`computePoolStats` rework:** stop deriving from `groupId === 0n`. Emit `RunnerPoolTotal::Bounded(n)` when the capacity declaration is `Some(n)`, `Unbounded` when `None`, `Undeclared` when there's no matching declaration. The `groupId === 0n` block at `runners.svelte.ts:54-56` is removed entirely.
6. **`RunnerPool.svelte` three-branch rendering** keyed on `pool.total.kind`. Bar + count for `Bounded`; count + distinct affordance for `Unbounded`; count-only for `Undeclared`. Affordance visual treatment selected via `impeccable` agent at implementation time. The affordance MUST distinguish `Unbounded` from `Undeclared` via text/icon semantics — not styling alone (WCAG SC 1.4.1).
7. **Helm chart `values.schema.json`:** `runnerPools[].capacity` becomes `"type": ["integer", "null"]`, `minimum: 1` retained (does not apply when value is `null`). `capacity` stays in the per-item `required` array (forces explicit key — matches the Rust contract).
8. **Helm chart `values.yaml`:** example comment block extended to show a `capacity: null` line.
9. **Tests** as enumerated in the Implementation Phases below.
10. **Documentation** updates per the Documents to Update section.

### Success criteria

- Operator declares `capacity: null` for `ubuntu-latest` → `helm upgrade` → fresh browser session shows the pool with the unbounded affordance and no saturation bar; running count still updates from observed jobs.
- Operator declares `capacity: 10` for `self-hosted,linux,x64` → renders as `running/10` with the saturation bar (no behavior change from #16).
- A pool that appears in observed jobs but is NOT declared in the config renders as count-only with no affordance (no behavior change from #16 for that case).
- Operator who omits the `capacity` key entirely (e.g. `labels: [a]` with no `capacity:` line) gets a startup error: "capacity is required (use `capacity: null` for an unbounded pool)."
- `capacity: 0` remains a startup error.
- `just lint`, `just test`, `just types`, `just test-e2e`, `helm lint`, `helm unittest` all pass.

## Locked Decisions

1. **One nullable field, not two fields.** `capacity` and "elastic" are mutually exclusive at the operator level. A separate `elastic: bool` would let operators write contradictory configs and force runtime validation to forbid the combination. A nullable `capacity` expresses the exclusivity in the type.
2. **Tagged sum on `RunnerPoolStats.total`, not a second flag.** The frontend needs to distinguish three states (bounded / unbounded / undeclared). `total: Option<u32>` alone can't express that — `null` collides between "unbounded" and "undeclared". A tagged sum keeps the discrimination on a single field.
3. **Adjacent tagging (`tag = "kind", content = "value"`).** Internal tagging breaks on unit variants in ts-rs 12.x; external tagging produces a less ergonomic shape (`{"Bounded": 10}`) and breaks if a variant carries multiple fields later. Adjacent tagging is the only representation that handles mixed payload/unit variants cleanly across serde + ts-rs.
4. **`is_elastic` retires from the wire entirely.** With the tagged sum, `total.kind === 'Unbounded'` *is* the elastic signal. Keeping `is_elastic` as a parallel bool would be redundant and re-create the two-source-of-truth problem the tagged sum solves. The `groupId === 0n` derivation at `runners.svelte.ts:54-56` is removed.
5. **Explicit `capacity: null` required in YAML.** Key omission is rejected. Matches #16's strictness about explicit operator intent — declaring a pool unbounded should be a deliberate choice visible in the YAML, not a side effect of forgetting the key.
6. **Struct-level custom `Deserialize` for `RunnerPoolConfig` and `RunnerPoolCapacity`.** Field-level `Option<u32>` alone cannot distinguish missing from explicit-null; field-level `deserialize_with` runs after extraction and can't tell either. The struct-level impl walks the input map, tracks which keys were seen, and errors when `capacity` is absent. Public field stays `Option<u32>`. Rejected the `serde_with` double-option pattern because it would leak `Option<Option<u32>>` into the public API and downstream call sites.
7. **Same strictness on `RunnerPoolCapacity`.** Symmetric contract: the wire-snapshot type round-trips through the same Deserialize as the config type so the snapshot can be parsed by the frontend (TypeScript) and any future Rust consumer with the same guarantees.
8. **Runtime-exhaustive switch in the frontend.** `frontend/CLAUDE.md` documents the sharp edge: typed-union switches need a `default: const _: never = value; throw new Error(...)` branch. The `RunnerPool.svelte` branch dispatch on `pool.total.kind` uses a script-side helper or `{:else}` fallthrough that throws. No template-only `#if/#else if/#else` chain.
9. **Unbounded affordance carries text/icon semantics, not just style.** Visual treatment deferred to `impeccable`, but the constraint is locked: `Unbounded` and `Undeclared` MUST be distinguishable by content, not by color/border/spacing alone. Otherwise the two no-bar branches collapse into a WCAG 1.4.1 regression.
10. **No new ADR.** ADR 0004's "operator config is additive over frontend-derived stats" rail is unchanged — operator-declared elasticity flows through the same path. Extend the existing footnote rather than write 0005. The substantive shift is *retiring the `groupId === 0n` derivation*; that's a footnote claim, not a new architectural boundary.
11. **No backend re-derivation of `RunnerPoolTotal`.** Stays consistent with ADR 0004: the backend ships `RunnerPoolCapacity` declarations on the snapshot; the frontend composes `RunnerPoolStats.total` during the merge. `RunnerPoolTotal` is a frontend-emitted shape; the wire only carries declared capacities.

## Architecture

### Backend — config and validation

- **Files:** `backend/crates/atc-server/src/config.rs`, `backend/crates/atc-core/src/types.rs`.
- **Changes:**
  - Replace `pub capacity: u32` with `pub capacity: Option<u32>` on both `RunnerPoolConfig` and `RunnerPoolCapacity`.
  - Implement struct-level `impl<'de> Deserialize<'de> for RunnerPoolConfig` (and same for `RunnerPoolCapacity`) via a `Visitor` that:
    1. Walks the input map.
    2. Rejects unknown keys (preserving `deny_unknown_fields` behavior).
    3. Errors when the `capacity` key is absent, with the message: `"capacity is required (use \`capacity: null\` for an unbounded pool)"`.
    4. Accepts `capacity: null` → `None`, `capacity: <int>` → `Some(int)`.
  - Update `validate_runner_pools()`:
    - Keep "capacity must be >= 1" check, fire only when `Some(0)`. Update message to: `"capacity must be >= 1 (use null for unbounded pools)"`.
    - Empty-labels and duplicate-canonicalized-label-set checks unchanged.

### Wire transport

- **Files:** `backend/crates/atc-core/src/types.rs`.
- **Changes:**
  - Drop `pub is_elastic: bool` from `RunnerPoolStats`.
  - Replace `pub total: Option<u32>` with `pub total: RunnerPoolTotal`.
  - Add new type:
    ```rust
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
    #[serde(tag = "kind", content = "value")]
    #[ts(export)]
    pub enum RunnerPoolTotal {
        Bounded(u32),
        Unbounded,
        Undeclared,
    }
    ```
  - Run `just types` from `backend/` to regenerate:
    - `frontend/src/lib/types/generated/RunnerPoolStats.ts` (no `isElastic`, `total: RunnerPoolTotal`)
    - `frontend/src/lib/types/generated/RunnerPoolCapacity.ts` (`capacity: number | null`)
    - `frontend/src/lib/types/generated/RunnerPoolTotal.ts` (new file)
- **Locked TS shape (verify after `just types`):**
  ```typescript
  export type RunnerPoolTotal =
    | { kind: "Bounded"; value: number }
    | { kind: "Unbounded" }
    | { kind: "Undeclared" };
  ```

### Frontend — store merge

- **Files:** `frontend/src/lib/stores/runners.svelte.ts`, `frontend/src/lib/stores/runners.test.ts`.
- **Changes:**
  - Remove the `groupId === 0n` block.
  - Initialize `entry.total = { kind: "Undeclared" }` (no longer `null`).
  - During the capacities merge, emit `{ kind: "Bounded", value: n }` when the declaration is `Some(n)`, `{ kind: "Unbounded" }` when `None`.
  - `RunnerPoolStats` no longer carries `isElastic`; remove from initializer.

### Frontend — component rendering

- **Files:** `frontend/src/lib/components/RunnerPool.svelte`, `frontend/src/lib/components/TopBar.svelte`, `frontend/src/lib/components/RunnerBar.svelte`.
- **Changes:**
  - `RunnerPoolDisplay` interface: drop `isElastic`; change `total: number | null` to `total: RunnerPoolTotal`.
  - Add a script-side helper function or factor the branch dispatch so that a runtime-exhaustive `switch (pool.total.kind)` with a `never`-assertion default lives in one place.
  - Three rendering branches:
    - `Bounded`: keep current `CapacityBar` + `running/total.value` rendering.
    - `Unbounded`: no `CapacityBar`; render `running` + an affordance element (icon + accessible text) that says "unbounded" or equivalent.
    - `Undeclared`: render `running` count only.
  - `TopBar.svelte`: pass `total` (now `RunnerPoolTotal`) instead of `(total, isElastic)` pair. Drop `isElastic` from the upstream pool map.

### Helm chart

- **Files:** `deploy/helm/atc/values.schema.json`, `deploy/helm/atc/values.yaml`, `deploy/helm/atc/README.md`, `deploy/helm/atc/tests/unit/runner-pools.yaml`.
- **`values.schema.json` change:**
  - `"type": "integer"` → `"type": ["integer", "null"]`.
  - `"minimum": 1` retained.
  - `"capacity"` stays in the per-item `required` array.
  - Update the description: `"Declared upper-bound runner count for the pool. Integer >= 1, or null to declare the pool unbounded (no renderable ceiling)."`
- **`values.yaml`:** extend the comment block to show a `capacity: null` example below the integer example.
- **`README.md`:** update the `runnerPools` section to document the `null` value.

### Why these choices over the alternatives

- **Struct-level Deserialize over `serde_with::DoubleOption`:** keeps the public field as `Option<u32>`, centralizes strictness logic, avoids leaking `Option<Option<u32>>` into every call site.
- **`RunnerPoolTotal` as a new type over reusing `Option<u32>` + an `is_elastic` bool:** retires two correlated bits that consumers had to reconcile, makes the frontend rendering branches a one-line exhaustive switch, kills the unreliable `groupId === 0n` heuristic at its source.
- **Adjacent tagging over internal or external:** only representation that handles mixed payload/unit variants cleanly across both serde and ts-rs.

## Implementation Phases

### Phase 0 — Branch and plan handoff

Create feature branch `feat/176-unbounded-runner-pools` from `main`. Copy this plan to `docs/design-plans/2026-05-15-issue-176-unbounded-runner-pools.md`. Commit with `docs(design): plan unbounded runner pools (#176)`.

### Phase 1 — Backend types and strictness

**Step 1: Failing tests** in `backend/crates/atc-server/src/config.rs`:
- `unbounded_capacity_via_null_is_accepted` — YAML with `capacity: null` parses to `RunnerPoolConfig { capacity: None, ... }`.
- `unbounded_capacity_via_key_omission_is_rejected` — YAML with `labels: [a]` and no `capacity:` line returns `Err`; error message contains `"capacity is required"` and `"capacity: null"`.
- `mixed_pool_list_validates` — config with one bounded pool (`capacity: 10`) and one unbounded pool (`capacity: null`) passes validation; both round-trip.
- Update `zero_capacity_is_a_validation_error` — verify the new error message wording.
- Update `missing_capacity_is_a_deserialization_error` — rewrite assertion to match the new custom-Deserialize error path.
- Keep `unknown_field_in_pool_is_rejected` as-is; add a one-line `// #176` comment.
- Add `atc-core` unit test: `runner_pool_total_round_trips_all_three_variants`.

**Step 2: Implementation.** Custom `Deserialize` on `RunnerPoolConfig` and `RunnerPoolCapacity`. `validate_runner_pools()` for `Option<u32>`. Add `RunnerPoolTotal`. Drop `is_elastic`.

### Phase 2 — Wire type regeneration and snapshot rail

**Step 1: Failing tests.** Update `state_tests.rs`, `tests/integration/common/mod.rs`, `ws_tests.rs`.

**Step 2: Implementation.** Run `just types` from `backend/` to regenerate TS bindings.

### Phase 3 — Frontend store and component rendering

**Step 1: Failing tests.**
- Extend `runners.test.ts`:
  - `merges_unbounded_capacity_to_total_unbounded`
  - `merges_bounded_capacity_to_total_bounded`
  - `pool_without_declaration_is_undeclared`
  - `removed_group_id_heuristic_does_not_set_unbounded`
- Extend `RunnerPool.test.ts`:
  - Rewrite the "elastic variant" test to test the `Unbounded` branch.
  - Add a test for the `Undeclared` branch.
  - Update the "known-capacity variant" test to use the new shape.
  - Add an accessibility test for the unbounded affordance.

**Step 2: Implementation.** Update `computePoolStats`, `RunnerPool.svelte`, `TopBar.svelte`. Use `impeccable` agent for the affordance visual treatment.

### Phase 4 — Helm chart schema and tests

**Step 1: Failing tests.** Add to `deploy/helm/atc/tests/unit/runner-pools.yaml`:
- `it: accepts a pool with capacity null`.

**Step 2: Implementation.** Update `values.schema.json`, `values.yaml`, `README.md`.

### Phase 5 — End-to-end and docs

**Step 1: Failing tests.** Extend `frontend/e2e/runner-pool-capacity.test.ts` with an unbounded fixture.

**Step 2: Implementation.** Update docs per the Documents to Update section.

## Acceptance Criteria

- **AC1.** `RunnerPoolConfig::capacity` and `RunnerPoolCapacity::capacity` are `Option<u32>`. The struct-level `Deserialize` impl rejects YAML that omits the `capacity` key with an error containing `"capacity is required"`.
- **AC2.** `capacity: null` parses to `None`; `capacity: <positive int>` parses to `Some(int)`; `capacity: 0` is a validation error with message containing `"must be >= 1"`.
- **AC3.** `RunnerPoolStats` no longer carries `is_elastic`. The `groupId === 0n` block in `runners.svelte.ts` is removed. `git grep "isElastic" -- frontend/src backend/crates` returns no hits.
- **AC4.** `RunnerPoolStats.total` is `RunnerPoolTotal` (tagged sum) with three variants: `Bounded(u32)`, `Unbounded`, `Undeclared`. The generated TS shape is exactly `{ kind: "Bounded"; value: number } | { kind: "Unbounded" } | { kind: "Undeclared" }`.
- **AC5.** `computePoolStats` produces `Bounded` for declared+integer, `Unbounded` for declared+null, `Undeclared` for non-declared. A pool whose only observed runner has `groupId === 0n` but no matching capacity declaration produces `Undeclared`.
- **AC6.** `RunnerPool.svelte` renders three distinct branches. The script-side switch on `pool.total.kind` includes a `default: const _: never = kind; throw new Error(...)` exhaustiveness assertion.
- **AC7.** The unbounded affordance is distinguishable from the undeclared rendering via text or icon semantics. Verified by a unit test.
- **AC8.** Helm `values.schema.json` accepts `capacity: null`, accepts `capacity: <positive int>`, rejects `capacity: 0`, rejects missing `capacity` key, rejects unknown sibling keys. All four cases covered by helm-unittest specs.
- **AC9.** Helm chart with `capacity: null` renders a ConfigMap whose `data."config.yaml"` contains the entry; the operator-side YAML round-trips through the ConfigMap to the binary's parsed `RunnerPoolConfig`.
- **AC10.** `just lint`, `just test`, `just types`, `just test-e2e`, `helm lint deploy/helm/atc`, `helm unittest deploy/helm/atc` all pass. `scripts/check-docs-lefthook.sh` reports no stale docs.
- **AC11.** Existing operator configs with integer `capacity` values are unaffected.

## Documents to Update

| Doc | Change |
|---|---|
| `docs/architecture/backend-server.md` (Runner Pool Stats section) | Replace the `is_elastic` line and the `total: Option<u32>` line with a description of `RunnerPoolTotal` and its three variants. Note that `total` is now the canonical bounded/unbounded/undeclared signal and that no backend re-derivation of `RunnerPoolTotal` occurs. |
| `docs/architecture/frontend-app.md` (Data Flow + RunnerPoolDisplay / store contract sections) | Update the data-flow paragraph for the three-branch rendering. Update the `RunnerPoolDisplay` shape doc. Update the store-contract section. |
| `docs/architecture/deployment.md` § File-based configuration | Add a paragraph documenting `capacity: null` for unbounded pools, the JSON Schema accept rules, and the explicit-key strictness. |
| `docs/architecture-decisions/0004-frontend-derived-pool-stats.md` (Footnote) | Extend the existing operator-config footnote: operator-declared elasticity flows through the same rail; the `groupId === 0n` derivation in `runners.svelte.ts` is retired; no new ADR required. |
| `backend/crates/atc-core/CLAUDE.md` | Note the new `RunnerPoolTotal` export and the `is_elastic` drop on `RunnerPoolStats`. |
| `backend/crates/atc-server/CLAUDE.md` | Note the custom `Deserialize` strictness on `RunnerPoolConfig` and `RunnerPoolCapacity`. |
| `frontend/CLAUDE.md` | Update the `RunStore` / `computePoolStats` line in the Key Files table to mention `RunnerPoolTotal`. |
| `deploy/helm/atc/CLAUDE.md` (Runner-pool capacities gating bullet) | Update the schema description (`capacity: integer minimum=1 OR null`). |
| `deploy/helm/atc/README.md` (runnerPools section) | Document `capacity: null` as the way to declare an unbounded pool. |
| `deploy/helm/atc/values.yaml` (example comment) | Extend the example to show a `capacity: null` line. |

## Out of Scope

- Migration tooling for operators currently using high-arbitrary `capacity` values to "fake" unbounded pools.
- Per-account GitHub-hosted concurrency limits as a separate signal.
- Hot-reload of `runnerPools` (already deferred to #172).
- GitHub API runner discovery (already deferred to #174).
- Env-encoded `runner_pools` override (already deferred to #175).

## Glossary

- **Bounded pool** — operator-declared with an integer `capacity`. Renders `running/N` plus saturation bar.
- **Unbounded pool** — operator-declared with `capacity: null`. Renders `running` plus a distinct affordance; no saturation bar.
- **Undeclared pool** — observed in webhook traffic but absent from the `runner_pools` config. Renders count-only.
- **`RunnerPoolTotal`** — the tagged-sum type on `RunnerPoolStats` that encodes the three states. Wire shape: `{ kind: "Bounded"; value: number } | { kind: "Unbounded" } | { kind: "Undeclared" }`.
- **Adjacent tagging** — serde representation where the discriminator and the payload are siblings: `{"kind": "Bounded", "value": 10}`.
