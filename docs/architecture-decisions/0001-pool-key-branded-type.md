# 0001 — `PoolKey` as the first branded TypeScript type

**Status:** Accepted (implemented in Sub-Phase 5, 2026-04-29)

## Context

Sub-Phase 5 introduced `PoolKey` (a canonical identifier for a runner pool, derived
by sorting and joining a pool's label array) as the type used in `uiStore.activePoolFilter`
and the `filterRunsByPool` helper. The naive choice would be a plain `string` alias.
Without nominal typing, calling sites could accidentally pass an unsorted label-join,
a single label, or any string at all — and the compiler would not catch it.

ts-rs-generated ID types (`RunId`, `JobId`, `StepId`) currently remain plain `bigint`
aliases — branding them would require ts-rs configuration changes or post-processing
that's out of scope for this phase.

## Decision

Introduce a phantom-property branded type pattern in `frontend/src/lib/filters/pool.ts`:

```ts
export type PoolKey = string & { readonly __brand: 'PoolKey' }
export function poolKey(labels: readonly string[]): PoolKey {
  return [...labels].sort().join('|') as PoolKey
}
```

Raw `string` cannot be assigned to `PoolKey` without going through `poolKey()`
or an explicit `as PoolKey` cast. The brand is enforced at compile time only;
at runtime, a `PoolKey` is an ordinary string.

Test coverage uses a `@ts-expect-error` directive to prove the brand contract:

```ts
// @ts-expect-error -- raw string is not assignable to PoolKey
const fromRawString: PoolKey = 'linux|x86'
```

If the brand is removed (PoolKey becomes a plain string alias), TypeScript emits
"Unused @ts-expect-error directive" and `pnpm check` fails.

## Consequences

**Positive:**
- Compile-time prevention of accidental string→PoolKey assignment.
- Sets a precedent for future TS-only domain types (e.g., a `RunDisplayKey` if such a
  derived identifier emerges).
- Zero runtime cost — branded types compile away to plain strings.

**Negative:**
- Tests that need to construct `PoolKey` values without going through `poolKey()`
  (e.g., E2E test setup) require `as any` casts. We accept this as a localized
  test-only escape hatch.
- ts-rs IDs remain plain `bigint` aliases (`RunId`, `JobId`, `StepId`). This is
  inconsistent: PoolKey is branded, RunId is not. A future ADR may revisit ts-rs
  configuration to introduce branded ID types if the inconsistency becomes painful.

## Related

- Implementation: `frontend/src/lib/filters/pool.ts`, `frontend/src/lib/filters/pool.test.ts`
- Design plan: `docs/design-plans/2026-04-25-interactivity.md`
